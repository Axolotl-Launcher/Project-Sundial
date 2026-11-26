// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::fmt;
use std::io::{self, Seek, SeekFrom, Write};
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axlauncher_spi::{
    ByteRange, DownloadBody, DownloadEtag, DownloadProtocol, DownloadRepresentation,
    DownloadResponse, FetchError, Network, RangeRequest, ResponseRange,
};
use axlauncher_storage::AtomicFile;
use bytes::Bytes;
use url::Origin;

use crate::resources::{BUFFER_BYTES, Bandwidth, Lease, Quota};
use crate::retry::{Attempts, Cooldowns, Recovery};
use crate::weighted::{FileShare, ShareLease};

pub(crate) struct TransferResources {
    connections: Quota,
    buffers: Quota,
    bandwidth: Bandwidth,
    bytes_received: AtomicU64,
    cooldowns: Cooldowns,
}

impl TransferResources {
    pub(crate) fn new(
        connections: NonZeroUsize,
        buffer_bytes: NonZeroUsize,
        rate: Option<NonZeroU64>,
    ) -> Self {
        let slots = (buffer_bytes.get() / BUFFER_BYTES).max(1);
        Self {
            connections: Quota::new(connections),
            buffers: Quota::new(NonZeroUsize::new(slots).unwrap()),
            bandwidth: Bandwidth::new(rate),
            bytes_received: AtomicU64::new(0),
            cooldowns: Cooldowns::default(),
        }
    }

    pub(crate) fn active_connections(&self) -> usize {
        self.connections.active()
    }
    pub(crate) fn connection_limit(&self) -> usize {
        self.connections.limit()
    }
    pub(crate) fn set_connection_limit(&self, limit: NonZeroUsize) {
        self.connections.set_limit(limit);
    }
    pub(crate) fn set_speed_limit(&self, limit: Option<NonZeroU64>) {
        self.bandwidth.set_limit(limit);
    }
    pub(crate) fn bytes_received(&self) -> u64 {
        self.bytes_received.load(Ordering::Relaxed)
    }
    pub(crate) fn cooldown(&self, origin: &Origin, grace: Duration) -> Option<(u16, Duration)> {
        self.cooldowns.remaining(origin, grace)
    }
}

pub(crate) struct Transfer<'a, Net> {
    pub(crate) resources: &'a TransferResources,
    pub(crate) protocol: DownloadProtocol,
    pub(crate) network: &'a Net,
    pub(crate) source: &'a str,
    pub(crate) origin: &'a Origin,
    pub(crate) attempts: &'a Attempts,
    pub(crate) host: &'a Quota,
    pub(crate) share: Option<&'a FileShare<'a>>,
}

#[derive(Debug)]
pub(crate) enum TransferError {
    Open(FetchError),
    Body(FetchError),
    Disk(io::Error),
    Invalid(String),
    RangeUnavailable,
    CoolingDown(u16),
    RetryWaitExceeded,
    RangeRetriesExhausted {
        error: Box<TransferError>,
        pending_delay: Option<Duration>,
    },
    RangeWaitExceeded(Box<TransferError>),
}

impl From<io::Error> for TransferError {
    fn from(error: io::Error) -> Self {
        Self::Disk(error)
    }
}

impl TransferError {
    pub(crate) fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Open(FetchError::Retryable(_) | FetchError::HttpRetryable { .. })
                | Self::Body(FetchError::Retryable(_))
                | Self::RangeRetriesExhausted { .. }
        )
    }

    pub(crate) fn retry_hint(&self) -> Option<Duration> {
        match self {
            Self::Open(FetchError::HttpRetryable { retry_after, .. }) => *retry_after,
            Self::RangeRetriesExhausted {
                error,
                pending_delay,
            } => error.retry_hint().max(*pending_delay),
            _ => None,
        }
    }

    pub(crate) fn origin_throttle(&self) -> bool {
        matches!(
            self,
            Self::CoolingDown(_) | Self::Open(FetchError::HttpRetryable { status: 429, .. })
        ) || matches!(
            self,
            Self::Open(FetchError::HttpRetryable {
                status: 503,
                retry_after: Some(_)
            })
        )
    }
}

impl fmt::Display for TransferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(FetchError::Rejected { status, .. }) => {
                write!(formatter, "opening download: HTTP {status}")
            }
            Self::Open(error) => write!(formatter, "opening download: {error}"),
            Self::Body(error) => write!(formatter, "reading download body: {error}"),
            Self::Invalid(reason) => formatter.write_str(reason),
            Self::Disk(error) => write!(formatter, "cannot write download: {error}"),
            Self::RangeUnavailable => formatter.write_str("source did not honor the byte range"),
            Self::CoolingDown(status) => write!(formatter, "HTTP {status}: origin is cooling down"),
            Self::RetryWaitExceeded => formatter.write_str("retry delay exceeds the wait budget"),
            Self::RangeRetriesExhausted { error, .. } => {
                write!(formatter, "range recovery stopped: {error}")
            }
            Self::RangeWaitExceeded(error) => write!(
                formatter,
                "range recovery exceeded the wait budget: {error}"
            ),
        }
    }
}

pub(crate) struct Connection<'a, Body> {
    pub(crate) response: DownloadResponse<Body>,
    // Closes the response body before admitting another request.
    _host: Lease,
    _global: Lease,
    _share: Option<ShareLease<'a>>,
}

impl<Net: Network> Transfer<'_, Net> {
    pub(crate) async fn whole(
        &self,
        mut connection: Connection<'_, impl DownloadBody>,
        stage: &Mutex<&mut AtomicFile>,
    ) -> Result<(), TransferError> {
        if connection.response.range.is_some() {
            return Err(TransferError::Invalid(
                "unsolicited partial download".into(),
            ));
        }
        if let Some(share) = self.share {
            share.set_remaining(None);
        }
        self.copy(&mut connection.response.body, stage, 0, None)
            .await
            .result
    }

    pub(crate) async fn open_whole(
        &self,
        stage: &Mutex<&mut AtomicFile>,
    ) -> Result<(), TransferError> {
        if let Some(share) = self.share {
            share.set_remaining(None);
            share.set_width(1);
        }
        let connection = self.open(None, None).await?;
        self.whole(connection, stage).await
    }

    async fn open(
        &self,
        range: Option<RangeRequest<'_>>,
        recovery: Option<Recovery<'_>>,
    ) -> Result<Connection<'_, impl DownloadBody>, TransferError> {
        self.check_cooldown()?;
        let share = match self.share {
            Some(share) => Some(share.acquire().await),
            None => None,
        };
        let host = self.host.acquire().await;
        let global = self.resources.connections.acquire().await;
        self.check_cooldown()?;
        if let Some(recovery) = recovery {
            recovery.dispatch();
        }
        self.attempts.record_request();
        let response = self
            .network
            .open_download(self.source, self.protocol, range)
            .await
            .map_err(|error| {
                self.resources
                    .cooldowns
                    .observe(self.origin, &error, self.attempts);
                TransferError::Open(error)
            })?;
        Ok(Connection {
            response,
            _host: host,
            _global: global,
            _share: share,
        })
    }

    fn check_cooldown(&self) -> Result<(), TransferError> {
        if let Some((status, _)) = self.resources.cooldown(self.origin, Duration::ZERO) {
            return Err(TransferError::CoolingDown(status));
        }
        Ok(())
    }

    pub(crate) async fn open_for_range(
        &self,
        range: ByteRange,
        tag: Option<&[u8]>,
        recovery: Option<Recovery<'_>>,
    ) -> Result<Connection<'_, impl DownloadBody>, TransferError> {
        self.open(
            Some(RangeRequest {
                interval: range,
                if_range: tag,
            }),
            recovery,
        )
        .await
        .map_err(|error| match error {
            TransferError::Open(FetchError::Rejected {
                status: 206 | 416, ..
            }) => TransferError::RangeUnavailable,
            other => other,
        })
    }

    pub(crate) async fn segment<Body: DownloadBody>(
        &self,
        stage: &Mutex<&mut AtomicFile>,
        expected: ResponseRange,
        validator: Option<&RangeValidator>,
        recovery: Option<Recovery<'_>>,
        initial: Option<Connection<'_, Body>>,
    ) -> CopyResult {
        if let Some(connection) = initial {
            return self
                .copy_segment(stage, expected, validator, connection)
                .await;
        }
        let tag = validator.map(RangeValidator::tag);
        let connection = match self.open_for_range(expected.interval, tag, recovery).await {
            Ok(connection) => connection,
            Err(error) => {
                return CopyResult {
                    end: expected.interval.start,
                    result: Err(error),
                };
            }
        };
        self.copy_segment(stage, expected, validator, connection)
            .await
    }

    async fn copy_segment(
        &self,
        stage: &Mutex<&mut AtomicFile>,
        expected: ResponseRange,
        validator: Option<&RangeValidator>,
        mut connection: Connection<'_, impl DownloadBody>,
    ) -> CopyResult {
        let response = &mut connection.response;
        if response.range != Some(expected)
            || !validator
                .is_none_or(|validator| validator.matches(response.representation.as_ref()))
        {
            return CopyResult {
                end: expected.interval.start,
                result: Err(TransferError::RangeUnavailable),
            };
        }
        self.copy(
            &mut response.body,
            stage,
            expected.interval.start,
            Some(expected.interval.end),
        )
        .await
    }

    async fn copy(
        &self,
        body: &mut impl DownloadBody,
        stage: &Mutex<&mut AtomicFile>,
        start: u64,
        end: Option<u64>,
    ) -> CopyResult {
        let mut position = start;
        let result = self.copy_body(body, stage, &mut position, end).await;
        CopyResult {
            end: position,
            result,
        }
    }

    async fn copy_body(
        &self,
        body: &mut impl DownloadBody,
        stage: &Mutex<&mut AtomicFile>,
        position: &mut u64,
        end: Option<u64>,
    ) -> Result<(), TransferError> {
        while let Some((chunk, _buffer)) = self.read(body).await.map_err(TransferError::Body)? {
            self.write_chunk(stage, position, end, &chunk).await?;
        }
        if end.is_some_and(|expected| expected != *position) {
            return Err(TransferError::Body(FetchError::Retryable(
                "download ended before the declared length".into(),
            )));
        }
        Ok(())
    }

    async fn read(
        &self,
        body: &mut impl DownloadBody,
    ) -> Result<Option<(Bytes, Lease)>, FetchError> {
        let buffer = self.resources.buffers.acquire().await;
        let chunk = body.next_chunk().await?;
        Ok(chunk.map(|chunk| (chunk, buffer)))
    }

    async fn write_chunk(
        &self,
        stage: &Mutex<&mut AtomicFile>,
        position: &mut u64,
        end: Option<u64>,
        chunk: &[u8],
    ) -> Result<(), TransferError> {
        let length = chunk.len() as u64;
        self.resources
            .bytes_received
            .fetch_add(length, Ordering::Relaxed);
        if end.is_some_and(|expected| expected.saturating_sub(*position) < length) {
            return Err(TransferError::RangeUnavailable);
        }
        for bytes in chunk.chunks(BUFFER_BYTES) {
            self.resources.bandwidth.acquire(bytes.len()).await;
            write(stage, *position, bytes)?;
            *position += bytes.len() as u64;
        }
        Ok(())
    }
}

pub(crate) struct RangeValidator {
    resource: String,
    tag: Bytes,
}

impl RangeValidator {
    fn tag(&self) -> &[u8] {
        &self.tag
    }

    pub(crate) fn from_response(response: &DownloadResponse<impl DownloadBody>) -> Option<Self> {
        let representation = response.representation.as_ref()?;
        let DownloadEtag::Strong(tag) = representation.etag.as_ref()? else {
            return None;
        };
        Some(Self {
            resource: representation.resource.clone(),
            tag: tag.clone(),
        })
    }

    fn matches(&self, actual: Option<&DownloadRepresentation>) -> bool {
        actual.is_some_and(|actual| {
            actual.resource == self.resource
                && actual.etag.as_ref().is_none_or(
                    |etag| matches!(etag, DownloadEtag::Strong(tag) if *tag == self.tag),
                )
        })
    }
}

fn write(stage: &Mutex<&mut AtomicFile>, position: u64, bytes: &[u8]) -> io::Result<()> {
    let mut stage = stage.lock().expect("staged download lock");
    let file = stage.file();
    file.seek(SeekFrom::Start(position))?;
    file.write_all(bytes)
}

pub(crate) struct CopyResult {
    pub(crate) end: u64,
    pub(crate) result: Result<(), TransferError>,
}
