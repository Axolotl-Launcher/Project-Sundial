// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::fmt;
use std::io::{self, Seek, SeekFrom, Write};
use std::sync::Mutex;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use axlauncher_spi::{
    ByteRange, DownloadBody, DownloadResponse, FetchError, Network, ResponseRange,
};
use axlauncher_storage::AtomicFile;
use bytes::Bytes;

use crate::engine::{DownloadEngine, DownloadJob};
use crate::ranges::{self, RangeSource};
use crate::resources::{BUFFER_BYTES, Lease, Quota};
use crate::weighted::{FileShare, MIN_RANGE_BYTES, ShareLease, range_width};

const RANGE_THRESHOLD: u64 = 4 * 1024 * 1024;
const PROBE: ByteRange = ByteRange { start: 0, end: 1 };
const SLOW_INTERVAL: Duration = Duration::from_secs(5);
const SLOW_BYTES_PER_SECOND: u64 = 40 * 1024;

pub(crate) struct Transfer<'a, Net> {
    pub(crate) engine: &'a DownloadEngine,
    pub(crate) network: &'a Net,
    pub(crate) source: &'a str,
    pub(crate) host: &'a Quota,
    pub(crate) share: Option<&'a FileShare<'a>>,
}

#[derive(Debug)]
pub(crate) enum TransferError {
    Network(FetchError),
    Disk(io::Error),
    Invalid(String),
    RangeUnavailable,
}

impl TransferError {
    pub(crate) fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Network(
                FetchError::Retryable(_)
                    | FetchError::Rejected {
                        status: 408 | 429,
                        ..
                    }
            ) | Self::Invalid(_)
        )
    }
}

impl From<FetchError> for TransferError {
    fn from(error: FetchError) -> Self {
        Self::Network(error)
    }
}

impl fmt::Display for TransferError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(FetchError::Retryable(reason) | FetchError::Fatal(reason))
            | Self::Invalid(reason) => formatter.write_str(reason),
            Self::Network(error) => error.fmt(formatter),
            Self::Disk(error) => write!(formatter, "cannot write download: {error}"),
            Self::RangeUnavailable => formatter.write_str("source did not honor the byte range"),
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
    pub(crate) async fn run(
        &self,
        job: DownloadJob<'_>,
        staging: &mut AtomicFile,
        final_round: bool,
    ) -> Result<(), TransferError> {
        let stage = Mutex::new(staging);
        let attempt = match job.size {
            Some(total) if total >= self.range_threshold() => {
                self.ranged(&stage, total, final_round).await
            }
            None if !job.batch && self.engine.policy.connections_per_file.get() > 1 => {
                self.probe(&stage, final_round).await
            }
            _ => Err(TransferError::RangeUnavailable),
        };
        match attempt {
            Ok(()) => return Ok(()),
            Err(TransferError::RangeUnavailable) => stage
                .lock()
                .expect("staged download lock")
                .file()
                .set_len(0)?,
            Err(error) => return Err(error),
        }
        if let Some(share) = self.share {
            share.set_remaining(None);
        }
        let connection = self.open(None).await?;
        self.whole(connection, &stage).await
    }

    async fn probe(
        &self,
        stage: &Mutex<&mut AtomicFile>,
        final_round: bool,
    ) -> Result<(), TransferError> {
        let connection = self.open_for_range(PROBE).await?;
        let Some(range) = connection.response.range else {
            return self.whole(connection, stage).await;
        };
        if range.interval != PROBE || range.total < self.range_threshold() {
            return Err(TransferError::RangeUnavailable);
        }
        drop(connection);
        self.ranged(stage, range.total, final_round).await
    }

    async fn ranged(
        &self,
        stage: &Mutex<&mut AtomicFile>,
        total: u64,
        final_round: bool,
    ) -> Result<(), TransferError> {
        let (width, chunk, minimum) = self.range_plan(total);
        if let Some(share) = self.share {
            share.set_remaining(Some(total));
        }
        let end = total.min(chunk);
        let interval = ByteRange { start: 0, end };
        let connection = self.open_for_range(interval).await?;
        let Some(range) = connection.response.range else {
            return self.whole(connection, stage).await;
        };
        if range.interval != interval || range.total != total {
            return Err(TransferError::RangeUnavailable);
        }
        let source = RangeSource {
            total,
            width,
            chunk,
            minimum,
        };
        ranges::download(self, stage, source, interval, connection, final_round).await
    }

    fn range_threshold(&self) -> u64 {
        if self.share.is_some() {
            2 * MIN_RANGE_BYTES
        } else {
            RANGE_THRESHOLD
        }
    }

    fn range_plan(&self, total: u64) -> (usize, u64, u64) {
        let limit = self.engine.policy.connections_per_file.get();
        let http1 = self.share.is_some();
        let width = if http1 {
            range_width(total, limit)
        } else {
            limit
        };
        let chunk = match (width, http1) {
            (1, _) => total,
            (_, true) => total.div_ceil(width as u64).min(ranges::INITIAL_SEGMENT),
            (_, false) => ranges::INITIAL_SEGMENT,
        };
        let minimum = if http1 {
            MIN_RANGE_BYTES
        } else {
            ranges::MIN_SEGMENT
        };
        (width, chunk, minimum)
    }

    async fn whole(
        &self,
        mut connection: Connection<'_, impl DownloadBody>,
        stage: &Mutex<&mut AtomicFile>,
    ) -> Result<(), TransferError> {
        if connection.response.range.is_some() {
            return Err(TransferError::RangeUnavailable);
        }
        if let Some(share) = self.share {
            share.set_remaining(None);
        }
        self.copy(&mut connection.response.body, stage, 0, None, false)
            .await
            .result
    }

    async fn open(
        &self,
        range: Option<ByteRange>,
    ) -> Result<Connection<'_, impl DownloadBody>, TransferError> {
        let share = match self.share {
            Some(share) => Some(share.acquire().await),
            None => None,
        };
        let host = self.host.acquire().await;
        let global = self.engine.connections.acquire().await;
        let response = self
            .network
            .open_download(self.source, self.engine.policy.protocol, range)
            .await?;
        Ok(Connection {
            response,
            _host: host,
            _global: global,
            _share: share,
        })
    }

    async fn open_for_range(
        &self,
        range: ByteRange,
    ) -> Result<Connection<'_, impl DownloadBody>, TransferError> {
        self.open(Some(range)).await.map_err(|error| match error {
            TransferError::Network(FetchError::Rejected {
                status: 206 | 416, ..
            }) => TransferError::RangeUnavailable,
            other => other,
        })
    }

    pub(crate) async fn segment<Body: DownloadBody>(
        &self,
        stage: &Mutex<&mut AtomicFile>,
        range: ByteRange,
        total: u64,
        split_slow: bool,
        initial: Option<Connection<'_, Body>>,
    ) -> CopyResult {
        if let Some(connection) = initial {
            return self
                .copy_segment(stage, range, total, split_slow, connection)
                .await;
        }
        let connection = match self.open_for_range(range).await {
            Ok(connection) => connection,
            Err(error) => {
                return CopyResult {
                    end: range.start,
                    result: Err(error),
                };
            }
        };
        self.copy_segment(stage, range, total, split_slow, connection)
            .await
    }

    async fn copy_segment(
        &self,
        stage: &Mutex<&mut AtomicFile>,
        range: ByteRange,
        total: u64,
        split_slow: bool,
        mut connection: Connection<'_, impl DownloadBody>,
    ) -> CopyResult {
        let expected = ResponseRange {
            interval: range,
            total,
        };
        let response = &mut connection.response;
        if response.range != Some(expected) {
            return CopyResult {
                end: range.start,
                result: Err(TransferError::RangeUnavailable),
            };
        }
        self.copy(
            &mut response.body,
            stage,
            range.start,
            Some(range.end),
            split_slow,
        )
        .await
    }

    async fn copy(
        &self,
        body: &mut impl DownloadBody,
        stage: &Mutex<&mut AtomicFile>,
        start: u64,
        end: Option<u64>,
        restart_slow: bool,
    ) -> CopyResult {
        let mut position = start;
        let result = self
            .copy_body(body, stage, &mut position, end, restart_slow)
            .await;
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
        restart_slow: bool,
    ) -> Result<(), TransferError> {
        let mut speed = ReadSpeed::default();
        while let Some((chunk, _buffer, elapsed)) = self.read(body).await? {
            self.write_chunk(stage, position, end, &chunk).await?;
            let slow = speed.observe(elapsed, chunk.len() as u64);
            self.check_speed(
                restart_slow && slow,
                end.map_or(u64::MAX, |end| end - *position),
            )?;
        }
        if end.is_some_and(|expected| expected != *position) {
            return Err(
                FetchError::Retryable("download ended before the declared length".into()).into(),
            );
        }
        Ok(())
    }

    async fn read(
        &self,
        body: &mut impl DownloadBody,
    ) -> Result<Option<(Bytes, Lease, Duration)>, FetchError> {
        let buffer = self.engine.buffers.acquire().await;
        let started = Instant::now();
        let chunk = body.next_chunk().await?;
        Ok(chunk.map(|chunk| (chunk, buffer, started.elapsed())))
    }

    async fn write_chunk(
        &self,
        stage: &Mutex<&mut AtomicFile>,
        position: &mut u64,
        end: Option<u64>,
        chunk: &[u8],
    ) -> Result<(), TransferError> {
        let length = chunk.len() as u64;
        self.engine
            .bytes_received
            .fetch_add(length, Ordering::Relaxed);
        if end.is_some_and(|expected| expected.saturating_sub(*position) < length) {
            return Err(TransferError::Invalid(
                "download returned more bytes than declared".into(),
            ));
        }
        for bytes in chunk.chunks(BUFFER_BYTES) {
            self.engine.bandwidth.acquire(bytes.len()).await;
            write(stage, *position, bytes)?;
            *position += bytes.len() as u64;
        }
        Ok(())
    }

    fn check_speed(&self, restart: bool, remainder: u64) -> Result<(), TransferError> {
        if restart && remainder > BUFFER_BYTES as u64 && !self.engine.bandwidth.is_limited() {
            return Err(FetchError::Retryable("download source is too slow".into()).into());
        }
        Ok(())
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

#[derive(Default)]
struct ReadSpeed {
    elapsed: Duration,
    bytes: u64,
}

impl ReadSpeed {
    fn observe(&mut self, elapsed: Duration, bytes: u64) -> bool {
        self.elapsed += elapsed;
        self.bytes = self.bytes.saturating_add(bytes);
        if self.elapsed < SLOW_INTERVAL {
            return false;
        }
        let slow = (self.bytes as f64) < SLOW_BYTES_PER_SECOND as f64 * self.elapsed.as_secs_f64();
        *self = Self::default();
        slow
    }
}
