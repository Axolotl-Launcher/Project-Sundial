// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::fmt;
use std::io::{self, Seek, SeekFrom, Write};
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use axlauncher_spi::{
    ByteRange, DownloadBody, DownloadEtag, DownloadProtocol, DownloadRepresentation,
    DownloadResponse, FetchError, Network, RangeRequest, ResponseRange,
};
use axlauncher_storage::AtomicFile;
use bytes::Bytes;
use url::Origin;

use crate::resources::{BUFFER_BYTES, Bandwidth, Lease, Quota};
use crate::retry::{Attempts, Cooldowns};
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
    RecoveryExhausted,
    RangeRecoveryExhausted(Box<TransferError>),
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
        )
    }

    pub(crate) fn retry_hint(&self) -> Option<Duration> {
        match self {
            Self::Open(FetchError::HttpRetryable { retry_after, .. }) => *retry_after,
            _ => None,
        }
    }

    pub(crate) fn origin_throttle(&self) -> bool {
        self.throttle_status().is_some()
    }

    pub(crate) fn throttle_status(&self) -> Option<u16> {
        match self {
            Self::CoolingDown(status) => Some(*status),
            Self::Open(FetchError::HttpRetryable {
                status: status @ 429,
                ..
            })
            | Self::Open(FetchError::HttpRetryable {
                status: status @ 503,
                retry_after: Some(_),
            }) => Some(*status),
            _ => None,
        }
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
            Self::RecoveryExhausted => formatter.write_str("retry budget exhausted"),
            Self::RangeRecoveryExhausted(error) => {
                write!(formatter, "retry budget exhausted: {error}")
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
        let activity = SegmentActivity::new(0, true);
        self.copy(&mut connection.response.body, stage, 0, None, &activity)
            .await
            .result
    }

    pub(crate) async fn open_whole(
        &self,
        stage: &Mutex<&mut AtomicFile>,
        recovery: bool,
    ) -> Result<(), TransferError> {
        if let Some(share) = self.share {
            share.set_remaining(None);
            share.set_width(1);
        }
        let connection = self.open(None, recovery, None).await?;
        self.whole(connection, stage).await
    }

    async fn open(
        &self,
        range: Option<RangeRequest<'_>>,
        recovery: bool,
        activity: Option<&SegmentActivity>,
    ) -> Result<Connection<'_, impl DownloadBody>, TransferError> {
        self.check_cooldown()?;
        let share = match self.share {
            Some(share) => Some(share.acquire().await),
            None => None,
        };
        let host = self.host.acquire().await;
        let global = self.resources.connections.acquire().await;
        self.check_cooldown()?;
        if recovery && !self.attempts.dispatch_recovery() {
            return Err(TransferError::RecoveryExhausted);
        }
        self.attempts.record_request();
        if let Some(activity) = activity {
            activity.mark_dispatched();
        }
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
        recovery: bool,
        activity: Option<&SegmentActivity>,
    ) -> Result<Connection<'_, impl DownloadBody>, TransferError> {
        self.open(
            Some(RangeRequest {
                interval: range,
                if_range: tag,
            }),
            recovery,
            activity,
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
        recovery: bool,
        activity: &SegmentActivity,
        initial: Option<Connection<'_, Body>>,
    ) -> CopyResult {
        if let Some(connection) = initial {
            return self
                .copy_segment(stage, expected, validator, activity, connection)
                .await;
        }
        let tag = validator.map(RangeValidator::tag);
        let connection = match self
            .open_for_range(expected.interval, tag, recovery, Some(activity))
            .await
        {
            Ok(connection) => connection,
            Err(error) => {
                return CopyResult {
                    end: expected.interval.start,
                    dispatched: activity.dispatched(),
                    result: Err(error),
                };
            }
        };
        self.copy_segment(stage, expected, validator, activity, connection)
            .await
    }

    async fn copy_segment(
        &self,
        stage: &Mutex<&mut AtomicFile>,
        expected: ResponseRange,
        validator: Option<&RangeValidator>,
        activity: &SegmentActivity,
        mut connection: Connection<'_, impl DownloadBody>,
    ) -> CopyResult {
        let response = &mut connection.response;
        if response.range != Some(expected)
            || !validator
                .is_none_or(|validator| validator.matches(response.representation.as_ref()))
        {
            return CopyResult {
                end: expected.interval.start,
                dispatched: true,
                result: Err(TransferError::RangeUnavailable),
            };
        }
        self.copy(
            &mut response.body,
            stage,
            expected.interval.start,
            Some(expected.interval.end),
            activity,
        )
        .await
    }

    async fn copy(
        &self,
        body: &mut impl DownloadBody,
        stage: &Mutex<&mut AtomicFile>,
        start: u64,
        end: Option<u64>,
        activity: &SegmentActivity,
    ) -> CopyResult {
        let mut position = start;
        let result = self
            .copy_body(body, stage, &mut position, end, activity)
            .await;
        CopyResult {
            end: position,
            dispatched: true,
            result,
        }
    }

    async fn copy_body(
        &self,
        body: &mut impl DownloadBody,
        stage: &Mutex<&mut AtomicFile>,
        position: &mut u64,
        end: Option<u64>,
        activity: &SegmentActivity,
    ) -> Result<(), TransferError> {
        while let Some((chunk, _buffer)) = self.read(body).await.map_err(TransferError::Body)? {
            self.write_chunk(stage, position, end, &chunk, activity)
                .await?;
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
        activity: &SegmentActivity,
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
            activity.record_written(*position);
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
    pub(crate) dispatched: bool,
    pub(crate) result: Result<(), TransferError>,
}

pub(crate) struct SegmentActivity {
    written: AtomicU64,
    dispatched: AtomicBool,
}

impl SegmentActivity {
    pub(crate) fn new(start: u64, dispatched: bool) -> Self {
        Self {
            written: AtomicU64::new(start),
            dispatched: AtomicBool::new(dispatched),
        }
    }

    fn mark_dispatched(&self) {
        self.dispatched.store(true, Ordering::Relaxed);
    }

    fn record_written(&self, end: u64) {
        self.written.store(end, Ordering::Relaxed);
    }

    fn dispatched(&self) -> bool {
        self.dispatched.load(Ordering::Relaxed)
    }

    pub(crate) fn paused(&self, status: u16) -> CopyResult {
        CopyResult {
            end: self.written.load(Ordering::Relaxed),
            dispatched: self.dispatched(),
            result: Err(TransferError::CoolingDown(status)),
        }
    }
}
