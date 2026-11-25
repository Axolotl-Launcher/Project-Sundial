// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::borrow::Cow;
use std::fs::File;
use std::io;
use std::num::{NonZeroU64, NonZeroUsize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axlauncher_network::verify_reader_any;
use axlauncher_spi::{DownloadProtocol, FetchError, Network};
use axlauncher_storage::AtomicFile;
use url::Url;

use crate::resources::{BUFFER_BYTES, Bandwidth, Quota, Quotas};
use crate::transfer::{Transfer, TransferError};
use crate::weighted::WeightedConnections;
use crate::{DEFAULT_CONCURRENCY, SHARED_ENGINE};

const RETRY_DELAYS: [Duration; 4] = [
    Duration::ZERO,
    Duration::from_millis(400),
    Duration::from_millis(600),
    Duration::from_secs(1),
];
const DEFAULT_FILE_CONNECTIONS: NonZeroUsize = NonZeroUsize::new(16).unwrap();
const DEFAULT_BUFFER_BUDGET: NonZeroUsize = NonZeroUsize::new(16 * 1024 * 1024).unwrap();

/// Limits shared by all transfers performed by one engine.
#[derive(Debug, Clone, Copy)]
pub struct DownloadPolicy {
    pub connections: NonZeroUsize,
    pub connections_per_host: NonZeroUsize,
    pub connections_per_file: NonZeroUsize,
    pub buffer_bytes: NonZeroUsize,
    pub bytes_per_second: Option<NonZeroU64>,
    pub protocol: DownloadProtocol,
}

impl Default for DownloadPolicy {
    fn default() -> Self {
        Self {
            connections: NonZeroUsize::new(DEFAULT_CONCURRENCY).unwrap(),
            connections_per_host: NonZeroUsize::MAX,
            connections_per_file: DEFAULT_FILE_CONNECTIONS,
            buffer_bytes: DEFAULT_BUFFER_BUDGET,
            bytes_per_second: None,
            protocol: DownloadProtocol::default(),
        }
    }
}

/// The ordered sources and integrity metadata for a destination.
#[derive(Debug, Clone, Copy)]
pub struct DownloadJob<'a> {
    pub sources: &'a [&'a str],
    pub destination: &'a Path,
    /// The expected digest, verified before promotion when provided.
    pub sha1: Option<&'a str>,
    /// Additional digests accepted by the manifest for the same artifact.
    pub sha1_alternatives: &'a [String],
    pub size: Option<u64>,
    /// Unknown-size files in a batch skip Range discovery.
    pub batch: bool,
}

/// Usage across all current and completed jobs of an engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferStats {
    pub active_connections: usize,
    pub connection_limit: usize,
    pub connections_per_file: usize,
    pub protocol: DownloadProtocol,
    /// Body bytes received, including data that an attempt discarded.
    pub bytes_received: u64,
}

/// The scheduler and shared resources for full and ranged file transfers.
pub struct DownloadEngine {
    pub(crate) policy: DownloadPolicy,
    pub(crate) connections: Quota,
    pub(crate) hosts: Quotas<String>,
    pub(crate) buffers: Quota,
    pub(crate) bandwidth: Bandwidth,
    pub(crate) bytes_received: AtomicU64,
    destinations: Quotas<PathBuf>,
    http1_connections: WeightedConnections,
}

impl Default for DownloadEngine {
    fn default() -> Self {
        Self::new(DownloadPolicy::default())
    }
}

impl DownloadEngine {
    /// A policy instance whose jobs share connection, buffer and bandwidth quotas.
    pub fn new(policy: DownloadPolicy) -> Self {
        let slots = (policy.buffer_bytes.get() / BUFFER_BYTES).max(1);
        Self {
            connections: Quota::new(policy.connections),
            hosts: Quotas::default(),
            buffers: Quota::new(NonZeroUsize::new(slots).unwrap()),
            bandwidth: Bandwidth::new(policy.bytes_per_second),
            bytes_received: AtomicU64::new(0),
            destinations: Quotas::default(),
            http1_connections: WeightedConnections::new(policy.connections.get()),
            policy,
        }
    }

    /// The scheduler used by [`crate::download_all`] and the launcher installer.
    pub fn shared() -> &'static Self {
        &SHARED_ENGINE
    }

    /// Changes global connection admission immediately for queued jobs.
    pub fn set_connection_limit(&self, limit: NonZeroUsize) {
        self.connections.set_limit(limit);
        self.http1_connections.set_limit(limit.get());
    }

    /// `None` removes the shared bandwidth cap immediately.
    pub fn set_speed_limit(&self, limit: Option<NonZeroU64>) {
        self.bandwidth.set_limit(limit);
    }

    /// Takes a snapshot of active connections and received bytes.
    pub fn stats(&self) -> TransferStats {
        TransferStats {
            active_connections: self.connections.active(),
            connection_limit: self.connections.limit(),
            connections_per_file: self.policy.connections_per_file.get(),
            protocol: self.policy.protocol,
            bytes_received: self.bytes_received.load(Ordering::Relaxed),
        }
    }

    /// Reuses a verified file or tries its sources and promotes a verified transfer.
    pub async fn download(
        &self,
        network: &impl Network,
        job: DownloadJob<'_>,
    ) -> Result<(), String> {
        let path = std::path::absolute(job.destination).map_err(|error| error.to_string())?;
        let exclusive = self.destinations.for_key(path, NonZeroUsize::MIN);
        let _lease = exclusive.acquire().await;
        if already_verified(job).await {
            return Ok(());
        }
        self.try_sources(network, job)
            .await
            .map_err(|error| error.to_string())
    }

    async fn try_sources(
        &self,
        network: &impl Network,
        job: DownloadJob<'_>,
    ) -> Result<(), TransferError> {
        let mut sources = ordered_sources(network, job.sources);
        let mut last = TransferError::Invalid("no download source supplied".into());
        let mut rounds = RETRY_DELAYS.into_iter().enumerate();
        while let Some((round, delay)) = rounds.next().filter(|_| !sources.is_empty()) {
            wait_before_retry(delay).await;
            let final_round = round + 1 == RETRY_DELAYS.len();
            last = match self
                .try_round(network, job, &mut sources, final_round)
                .await?
            {
                Some(error) => error,
                None => return Ok(()),
            };
        }
        Err(last)
    }

    async fn try_round(
        &self,
        network: &impl Network,
        job: DownloadJob<'_>,
        sources: &mut Vec<Cow<'_, str>>,
        final_round: bool,
    ) -> Result<Option<TransferError>, TransferError> {
        let mut last = TransferError::Invalid("no download source supplied".into());
        for source in std::mem::take(sources) {
            let error = match self.attempt(network, job, &source, final_round).await {
                Ok(()) => return Ok(None),
                Err(error @ TransferError::Disk(_)) => return Err(error),
                Err(error) => error,
            };
            sources.extend(error.retryable().then_some(source));
            last = error;
        }
        Ok(Some(last))
    }

    async fn attempt(
        &self,
        network: &impl Network,
        job: DownloadJob<'_>,
        source: &str,
        final_round: bool,
    ) -> Result<(), TransferError> {
        let host = Url::parse(source)
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https"))
            .and_then(|url| url.host_str().map(str::to_owned))
            .ok_or_else(|| FetchError::Fatal(format!("invalid download URL: {source}")))?;
        let limit = self
            .policy
            .connections_per_host
            .min(network.download_connection_limit(&host));
        let host = self.hosts.for_key(host, limit);
        let share = (self.policy.protocol == DownloadProtocol::Http1).then(|| {
            self.http1_connections
                .register(job.size, self.policy.connections_per_file.get())
        });
        let transfer = Transfer {
            engine: self,
            network,
            source,
            host: &host,
            share: share.as_ref(),
        };
        let mut staging = AtomicFile::new(job.destination)?;
        transfer.run(job, &mut staging, final_round).await?;
        drop(share);
        verify(&mut staging, job).await?;
        staging.commit(job.destination)?;
        Ok(())
    }
}

fn ordered_sources<'a>(network: &impl Network, originals: &[&'a str]) -> Vec<Cow<'a, str>> {
    let mut sources = Vec::with_capacity(originals.len());
    let mut fallback = Vec::new();
    for &original in originals {
        if sources.contains(&Cow::Borrowed(original)) {
            continue;
        }
        match network.download_mirror(original) {
            Some(mirror) if mirror.preferred => push_source(&mut sources, Cow::Owned(mirror.url)),
            Some(mirror) => fallback.push(Cow::Owned(mirror.url)),
            None => (),
        }
        sources.push(Cow::Borrowed(original));
    }
    for mirror in fallback {
        push_source(&mut sources, mirror);
    }
    sources
}

fn push_source<'a>(sources: &mut Vec<Cow<'a, str>>, source: Cow<'a, str>) {
    if !sources.contains(&source) {
        sources.push(source);
    }
}

async fn wait_before_retry(delay: Duration) {
    if !delay.is_zero() && tokio::runtime::Handle::try_current().is_ok() {
        tokio::time::sleep(delay).await;
    }
}

async fn already_verified(job: DownloadJob<'_>) -> bool {
    if job.sha1.is_none() && job.sha1_alternatives.is_empty() {
        return false;
    }
    let Ok(file) = axlauncher_storage::open_for_read(job.destination) else {
        return false;
    };
    verify_file(file, job.sha1, job.sha1_alternatives)
        .await
        .unwrap_or(false)
}

async fn verify(staging: &mut AtomicFile, job: DownloadJob<'_>) -> Result<(), TransferError> {
    if job.sha1.is_some() || !job.sha1_alternatives.is_empty() {
        let file = staging.rewind()?.try_clone()?;
        return verify_file(file, job.sha1, job.sha1_alternatives)
            .await?
            .then_some(())
            .ok_or_else(|| {
                TransferError::Invalid(format!("SHA-1 mismatch for {}", job.destination.display()))
            });
    }
    let length = staging.file().metadata()?.len();
    if job.size.is_some_and(|expected| expected != length) {
        return Err(TransferError::Invalid(format!(
            "size mismatch for {}",
            job.destination.display()
        )));
    }
    Ok(())
}

async fn verify_file(file: File, sha1: Option<&str>, alternatives: &[String]) -> io::Result<bool> {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return verify_reader_any(file, sha1, alternatives);
    };
    let sha1 = sha1.map(str::to_owned);
    let alternatives = alternatives.to_vec();
    runtime
        .spawn_blocking(move || verify_reader_any(file, sha1.as_deref(), &alternatives))
        .await
        .map_err(io::Error::other)?
}

impl From<io::Error> for TransferError {
    fn from(error: io::Error) -> Self {
        Self::Disk(error)
    }
}
