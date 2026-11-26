// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::borrow::Cow;
use std::fs::File;
use std::io;
use std::num::{NonZeroU64, NonZeroUsize};
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use axlauncher_spi::{DownloadProtocol, FetchError, Network};
use axlauncher_storage::AtomicFile;
use url::{Origin, Url};

use crate::ranges::{FileTransfer, TransferMode};
use crate::resources::Quotas;
use crate::retry::{Attempts, Delay, WaitBudget, cooldown_jitter};
use crate::transfer::{Transfer, TransferError, TransferResources};
use crate::verify::{Verification, already_verified, has_digest, verify};
use crate::weighted::WeightedConnections;
use crate::{DEFAULT_CONCURRENCY, DownloadJob};

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
    policy: DownloadPolicy,
    resources: TransferResources,
    hosts: Quotas<String>,
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
        Self {
            resources: TransferResources::new(
                policy.connections,
                policy.buffer_bytes,
                policy.bytes_per_second,
            ),
            hosts: Quotas::default(),
            destinations: Quotas::default(),
            http1_connections: WeightedConnections::new(policy.connections.get()),
            policy,
        }
    }

    /// The scheduler used by [`crate::download_all`] and the launcher installer.
    pub fn shared() -> &'static Self {
        static SHARED_ENGINE: LazyLock<DownloadEngine> = LazyLock::new(DownloadEngine::default);
        &SHARED_ENGINE
    }

    /// Changes global connection admission immediately for queued jobs.
    pub fn set_connection_limit(&self, limit: NonZeroUsize) {
        self.resources.set_connection_limit(limit);
        self.http1_connections.set_limit(limit.get());
    }

    /// `None` removes the shared bandwidth cap immediately.
    pub fn set_speed_limit(&self, limit: Option<NonZeroU64>) {
        self.resources.set_speed_limit(limit);
    }

    /// Takes a snapshot of active connections and received bytes.
    pub fn stats(&self) -> TransferStats {
        TransferStats {
            active_connections: self.resources.active_connections(),
            connection_limit: self.resources.connection_limit(),
            connections_per_file: self.policy.connections_per_file.get(),
            protocol: self.policy.protocol,
            bytes_received: self.resources.bytes_received(),
        }
    }

    /// Reuses a verified file or tries its sources and promotes a verified transfer.
    ///
    /// Retry delays require a Tokio time driver.
    pub async fn download(
        &self,
        network: &impl Network,
        job: DownloadJob<'_>,
    ) -> Result<(), String> {
        self.download_job(network, job, None).await
    }

    /// Applies additional integrity checks to cached files and each source before promotion.
    ///
    /// The verifier starts at byte zero and signals a mismatch with `InvalidData`.
    pub async fn download_verified(
        &self,
        network: &impl Network,
        job: DownloadJob<'_>,
        verify: impl Fn(&mut File) -> io::Result<()> + Send + Sync + 'static,
    ) -> Result<(), String> {
        let verify: Verification = Arc::new(verify);
        self.download_job(network, job, Some(&verify)).await
    }

    async fn download_job(
        &self,
        network: &impl Network,
        job: DownloadJob<'_>,
        verify: Option<&Verification>,
    ) -> Result<(), String> {
        let path = std::path::absolute(job.destination).map_err(|error| error.to_string())?;
        let exclusive = self.destinations.for_key(path, NonZeroUsize::MIN);
        let _lease = exclusive.acquire().await;
        if already_verified(job, verify)
            .await
            .map_err(|error| error.to_string())?
        {
            return Ok(());
        }
        self.try_sources(network, job, verify).await
    }

    async fn try_sources(
        &self,
        network: &impl Network,
        job: DownloadJob<'_>,
        verify: Option<&Verification>,
    ) -> Result<(), String> {
        let mut sources: Vec<_> = ordered_sources(network, job.sources, has_digest(job, verify))
            .into_iter()
            .map(Source::new)
            .collect();
        let mut wait = WaitBudget::default();
        while let Some(source) = self.ready_source(&mut sources, &mut wait).await {
            let error = match source.attempt(self, network, job, verify, &mut wait).await {
                Ok(()) => return Ok(()),
                Err(error @ TransferError::Disk(_)) => return Err(error.to_string()),
                Err(error) => error,
            };
            source.failed(error);
        }
        Err(source_failure(&sources))
    }

    async fn ready_source<'a, 'url>(
        &self,
        sources: &'a mut [Source<'url>],
        wait: &mut WaitBudget,
    ) -> Option<&'a mut Source<'url>> {
        let (mut index, mut priority) = self.next_candidate(sources)?;
        while !sources[index].ready(priority, wait).await {
            (index, priority) = self.next_candidate(sources)?;
        }
        Some(&mut sources[index])
    }

    fn next_candidate(&self, sources: &[Source<'_>]) -> Option<(usize, SourcePriority)> {
        sources
            .iter()
            .enumerate()
            .filter_map(|(index, source)| Some((index, source.priority(&self.resources)?)))
            .min_by_key(|(_, priority)| (priority.delay, priority.used))
    }
}

#[derive(Clone, Copy)]
struct SourcePriority {
    delay: Duration,
    used: bool,
    status: Option<u16>,
}

struct Source<'a> {
    url: Cow<'a, str>,
    origin: Option<Origin>,
    host: Option<String>,
    attempts: Attempts,
    delay: Option<Delay>,
    cooldown_jitter: Duration,
    error: Option<TransferError>,
    mode: TransferMode,
    staging: Option<AtomicFile>,
    integrity_retry_used: bool,
    exhausted: bool,
    done: bool,
}

impl<'a> Source<'a> {
    fn new(url: Cow<'a, str>) -> Self {
        let parsed = Url::parse(&url)
            .ok()
            .filter(|url| matches!(url.scheme(), "http" | "https"));
        Self {
            url,
            origin: parsed.as_ref().map(Url::origin),
            host: parsed.as_ref().and_then(Url::host_str).map(str::to_owned),
            attempts: Attempts::default(),
            delay: None,
            cooldown_jitter: cooldown_jitter(),
            error: None,
            mode: TransferMode::default(),
            staging: None,
            integrity_retry_used: false,
            exhausted: false,
            done: false,
        }
    }

    async fn attempt(
        &mut self,
        engine: &DownloadEngine,
        network: &impl Network,
        job: DownloadJob<'_>,
        check: Option<&Verification>,
        wait: &mut WaitBudget,
    ) -> Result<(), TransferError> {
        let (Some(host), Some(origin)) = (self.host.as_ref(), self.origin.as_ref()) else {
            return Err(TransferError::Open(FetchError::Fatal(
                "invalid download URL".into(),
            )));
        };
        let limit = engine
            .policy
            .connections_per_host
            .min(network.download_connection_limit(host));
        let host = engine.hosts.for_key(host.clone(), limit);
        let share = (engine.policy.protocol == DownloadProtocol::Http1).then(|| {
            engine
                .http1_connections
                .register(job.size, engine.policy.connections_per_file.get())
        });
        let transfer = Transfer {
            resources: &engine.resources,
            protocol: engine.policy.protocol,
            network,
            source: &self.url,
            origin,
            attempts: &self.attempts,
            host: &host,
            share: share.as_ref(),
        };
        let mut staging = match self.staging.take() {
            Some(staging) => staging,
            None => AtomicFile::new(job.destination)?,
        };
        let result = FileTransfer {
            transfer: &transfer,
            parallelism: engine.policy.connections_per_file.get(),
            has_digest: has_digest(job, check),
            probe_unknown: !job.batch,
        }
        .run(
            job.destination,
            job.size,
            &mut staging,
            &mut self.mode,
            wait,
        )
        .await;
        drop(share);
        if matches!(self.mode, TransferMode::Paused(_)) && result.is_err() {
            self.staging = Some(staging);
            return result;
        }
        result?;
        self.verify_stage(&mut staging, job, check).await?;
        staging.commit(job.destination)?;
        Ok(())
    }

    async fn verify_stage(
        &mut self,
        staging: &mut AtomicFile,
        job: DownloadJob<'_>,
        check: Option<&Verification>,
    ) -> Result<(), TransferError> {
        let result = verify(staging, job, check).await;
        if matches!(result, Err(TransferError::IntegrityMismatch(_))) {
            self.mode = TransferMode::Whole;
        }
        result
    }

    async fn ready(
        &mut self,
        SourcePriority { delay, status, .. }: SourcePriority,
        wait: &mut WaitBudget,
    ) -> bool {
        if delay.is_zero() {
            return true;
        }
        if wait.allows(delay) {
            wait.wait(delay).await;
        } else {
            self.done = true;
            self.exhausted = true;
            let error = status.map_or(TransferError::RetryWaitExceeded, TransferError::CoolingDown);
            self.error.get_or_insert(error);
        }
        false
    }

    fn priority(&self, resources: &TransferResources) -> Option<SourcePriority> {
        if self.done {
            return None;
        }
        let server = self
            .origin
            .as_ref()
            .and_then(|origin| resources.cooldown(origin, self.cooldown_jitter));
        let local = self.delay.map_or(Duration::ZERO, Delay::remaining);
        let delay = server.map_or(local, |(_, remaining)| local.max(remaining));
        Some(SourcePriority {
            delay,
            used: self.attempts.request_count() != 0,
            status: server.map(|(status, _)| status),
        })
    }

    fn failed(&mut self, error: TransferError) {
        let cooling = matches!(error, TransferError::CoolingDown(_));
        let retry_integrity = matches!(error, TransferError::IntegrityMismatch(_))
            && !std::mem::replace(&mut self.integrity_retry_used, true);
        if error.retryable() || retry_integrity || cooling {
            self.exhausted = !self.attempts.can_recover();
            self.delay = (!self.exhausted && !error.origin_throttle())
                .then(|| self.attempts.backoff(error.retry_hint()));
        } else {
            self.done = true;
            self.exhausted = matches!(
                error,
                TransferError::RangeWaitExceeded(_)
                    | TransferError::RecoveryExhausted
                    | TransferError::RangeRecoveryExhausted(_)
            );
        }
        self.done |= self.exhausted;
        if !cooling || self.error.is_none() {
            self.error = Some(error);
        }
    }

    fn failure_rank(&self) -> Option<u8> {
        Some(match self.error.as_ref()? {
            TransferError::IntegrityMismatch(_) | TransferError::Invalid(_) => 4,
            TransferError::RecoveryExhausted
            | TransferError::RangeRecoveryExhausted(_)
            | TransferError::RangeWaitExceeded(_)
            | TransferError::Body(_) => 3,
            TransferError::Open(FetchError::HttpRetryable { .. }) => 2,
            TransferError::CoolingDown(_) | TransferError::RetryWaitExceeded => 0,
            _ => 1,
        })
    }

    fn describe_failure(&self) -> String {
        let error = self.error.as_ref().expect("attempted source");
        let origin = self
            .origin
            .as_ref()
            .map_or_else(|| "invalid source".into(), Origin::ascii_serialization);
        let requests = self.attempts.request_count();
        let exhausted = if self.exhausted
            && !matches!(
                error,
                TransferError::RecoveryExhausted | TransferError::RangeRecoveryExhausted(_)
            ) {
            ", retry budget exhausted"
        } else {
            ""
        };
        format!("{error} ({origin}, {requests} requests{exhausted})")
    }
}

fn source_failure(sources: &[Source<'_>]) -> String {
    sources
        .iter()
        .filter_map(|source| Some((source.failure_rank()?, source)))
        .max_by_key(|(rank, _)| *rank)
        .map_or_else(
            || "no download source supplied".into(),
            |(_, source)| source.describe_failure(),
        )
}

fn ordered_sources<'a>(
    network: &impl Network,
    originals: &[&'a str],
    has_digest: bool,
) -> Vec<Cow<'a, str>> {
    let mut sources = Vec::with_capacity(originals.len());
    let mut fallback = Vec::new();
    for &original in originals {
        if sources.contains(&Cow::Borrowed(original)) {
            continue;
        }
        match network.download_mirror(original, has_digest) {
            Some(mirror) if mirror.preferred => push_source(&mut sources, Cow::Owned(mirror.url)),
            Some(mirror) => fallback.push(Cow::Owned(mirror.url)),
            None => (),
        }
        push_source(&mut sources, Cow::Borrowed(original));
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
