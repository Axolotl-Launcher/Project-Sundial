// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axlauncher_spi::{ByteRange, DownloadBody, Network, ResponseRange};
use axlauncher_storage::AtomicFile;
use futures::{StreamExt, future::Either, stream::FuturesUnordered};

use crate::retry::{Attempts, Delay, Recovery, WaitBudget};
use crate::transfer::{Connection, CopyResult, RangeValidator, Transfer, TransferError};
use crate::weighted::{MIN_RANGE_BYTES, range_width};

const MIN_SEGMENT: u64 = 256 * 1024;
const INITIAL_SEGMENT: u64 = 8 * 1024 * 1024;
const MAX_SEGMENT: u64 = 32 * 1024 * 1024;
const SEGMENT_TIME: Duration = Duration::from_secs(2);

const RANGE_THRESHOLD: u64 = 4 * 1024 * 1024;
const PROBE: ByteRange = ByteRange { start: 0, end: 1 };

#[derive(Default)]
pub(crate) enum TransferMode {
    #[default]
    Discover,
    Ranged(u64),
    Whole,
}

pub(crate) struct FileTransfer<'a, 'resources, Net> {
    pub(crate) transfer: &'a Transfer<'resources, Net>,
    pub(crate) parallelism: usize,
    pub(crate) has_digest: bool,
    pub(crate) probe_unknown: bool,
}

impl<Net: Network> FileTransfer<'_, '_, Net> {
    pub(crate) async fn run(
        &self,
        destination: &Path,
        size: Option<u64>,
        staging: &mut AtomicFile,
        mode: &mut TransferMode,
        wait: &mut WaitBudget,
    ) -> Result<(), TransferError> {
        let stage = Mutex::new(staging);
        if matches!(mode, TransferMode::Whole) {
            return self.transfer.open_whole(&stage).await;
        }
        let attempt = self.run_ranges(size, &stage, mode, wait).await;
        if !matches!(attempt, Err(TransferError::RangeUnavailable)) {
            return attempt;
        }
        *mode = TransferMode::Whole;
        **stage.lock().expect("staged download lock") = AtomicFile::new(destination)?;
        self.transfer.open_whole(&stage).await
    }

    async fn run_ranges(
        &self,
        size: Option<u64>,
        stage: &Mutex<&mut AtomicFile>,
        mode: &mut TransferMode,
        wait: &mut WaitBudget,
    ) -> Result<(), TransferError> {
        if let TransferMode::Ranged(total) = *mode {
            return self.ranged(stage, total, mode, wait).await;
        }
        if let Some(total) = size.filter(|&total| total >= self.range_threshold()) {
            *mode = TransferMode::Ranged(total);
            return self.ranged(stage, total, mode, wait).await;
        }
        if size.is_none() && self.probe_unknown && self.parallelism > 1 {
            return self.probe(stage, mode, wait).await;
        }
        *mode = TransferMode::Whole;
        self.transfer.open_whole(stage).await
    }

    async fn probe(
        &self,
        stage: &Mutex<&mut AtomicFile>,
        mode: &mut TransferMode,
        wait: &mut WaitBudget,
    ) -> Result<(), TransferError> {
        let connection = self.transfer.open_for_range(PROBE, None, None).await?;
        let Some(range) = connection.response.range else {
            *mode = TransferMode::Whole;
            return self.transfer.whole(connection, stage).await;
        };
        if range.interval != PROBE
            || range.total < self.range_threshold()
            || (!self.has_digest && RangeValidator::from_response(&connection.response).is_none())
        {
            return Err(TransferError::RangeUnavailable);
        }
        drop(connection);
        *mode = TransferMode::Ranged(range.total);
        self.ranged(stage, range.total, mode, wait).await
    }

    async fn ranged(
        &self,
        stage: &Mutex<&mut AtomicFile>,
        total: u64,
        mode: &mut TransferMode,
        wait: &mut WaitBudget,
    ) -> Result<(), TransferError> {
        let (width, chunk, minimum) = self.range_plan(total);
        if let Some(share) = self.transfer.share {
            share.set_remaining(Some(total));
        }
        let end = total.min(chunk);
        let interval = ByteRange { start: 0, end };
        let connection = self.transfer.open_for_range(interval, None, None).await?;
        let Some(range) = connection.response.range else {
            *mode = TransferMode::Whole;
            return self.transfer.whole(connection, stage).await;
        };
        let validator = RangeValidator::from_response(&connection.response);
        if range.interval != interval
            || range.total != total
            || (!self.has_digest && validator.is_none())
        {
            return Err(TransferError::RangeUnavailable);
        }
        let source = RangeSource {
            total,
            width,
            chunk,
            minimum,
            validator,
        };
        let result = download(self.transfer, stage, &source, interval, connection, wait).await;
        if result.is_err() {
            *mode = TransferMode::Whole;
        }
        result
    }

    fn range_threshold(&self) -> u64 {
        if self.transfer.share.is_some() {
            2 * MIN_RANGE_BYTES
        } else {
            RANGE_THRESHOLD
        }
    }

    fn range_plan(&self, total: u64) -> (usize, u64, u64) {
        let limit = self.parallelism;
        let http1 = self.transfer.share.is_some();
        let width = if http1 {
            range_width(total, limit)
        } else {
            limit
        };
        let chunk = match (width, http1) {
            (1, _) => total,
            (_, true) => total.div_ceil(width as u64).min(INITIAL_SEGMENT),
            (_, false) => INITIAL_SEGMENT,
        };
        let minimum = if http1 { MIN_RANGE_BYTES } else { MIN_SEGMENT };
        (width, chunk, minimum)
    }
}

struct RangeSource {
    total: u64,
    width: usize,
    chunk: u64,
    minimum: u64,
    validator: Option<RangeValidator>,
}

struct Work<'a> {
    interval: ByteRange,
    recovery: Option<Recovery<'a>>,
}

struct Pending<'a> {
    work: Work<'a>,
    delay: Delay,
    error: TransferError,
}

async fn download<Net: Network>(
    transfer: &Transfer<'_, Net>,
    stage: &Mutex<&mut AtomicFile>,
    source: &RangeSource,
    first: ByteRange,
    connection: Connection<'_, impl DownloadBody>,
    wait: &mut WaitBudget,
) -> Result<(), TransferError> {
    let mut work = Schedule {
        remaining: ByteRange {
            start: first.end,
            end: source.total,
        },
        pending: Vec::new(),
        chunk: source.chunk,
        minimum: source.minimum,
    };
    let mut active = FuturesUnordered::new();
    active.push(run(
        transfer,
        stage,
        Work {
            interval: first,
            recovery: None,
        },
        source,
        Some(connection),
    ));
    let mut demand = source.width;
    loop {
        active.extend(
            (active.len()..source.width)
                .map_while(|_| work.next())
                .map(|next| run(transfer, stage, next, source, None)),
        );
        if let Some(share) = transfer.share
            && demand != active.len()
        {
            demand = active.len();
            share.set_width(demand);
        }
        if active.is_empty() && !work.wait_next(wait).await? {
            return Ok(());
        }
        if active.is_empty() {
            continue;
        }
        let delay = (active.len() < source.width)
            .then(|| work.next_delay())
            .flatten();
        let Some((interval, result, elapsed)) = complete_or_wake(&mut active, delay, wait).await
        else {
            continue;
        };
        let written = work.completed(transfer.attempts, interval, result, elapsed)?;
        if let Some(share) = transfer.share {
            share.record_written(written);
        }
    }
}

async fn complete_or_wake<T>(
    active: &mut (impl futures::Stream<Item = T> + Unpin),
    delay: Option<Duration>,
    wait: &mut WaitBudget,
) -> Option<T> {
    let Some(delay) = delay.filter(|&delay| wait.allows(delay)) else {
        return active.next().await;
    };
    let started = tokio::time::Instant::now();
    let timer = std::pin::pin!(tokio::time::sleep(delay));
    let completed = match futures::future::select(active.next(), timer).await {
        Either::Left((completed, _)) => completed,
        Either::Right(_) => None,
    };
    wait.charge(started);
    completed
}

async fn run<Net: Network>(
    transfer: &Transfer<'_, Net>,
    stage: &Mutex<&mut AtomicFile>,
    work: Work<'_>,
    source: &RangeSource,
    initial: Option<Connection<'_, impl DownloadBody>>,
) -> (ByteRange, CopyResult, Duration) {
    let start = Instant::now();
    let result = transfer
        .segment(
            stage,
            ResponseRange {
                interval: work.interval,
                total: source.total,
            },
            source.validator.as_ref(),
            work.recovery,
            initial,
        )
        .await;
    (work.interval, result, start.elapsed())
}

struct Schedule<'a> {
    remaining: ByteRange,
    pending: Vec<Pending<'a>>,
    chunk: u64,
    minimum: u64,
}

impl<'a> Schedule<'a> {
    fn next(&mut self) -> Option<Work<'a>> {
        if let Some(index) = self
            .pending
            .iter()
            .position(|pending| pending.delay.remaining().is_zero())
        {
            return Some(self.pending.swap_remove(index).work);
        }
        let available = self.remaining.end - self.remaining.start;
        if available == 0 {
            return None;
        }
        let end = self.remaining.start + self.next_size(available);
        let interval = ByteRange {
            start: self.remaining.start,
            end,
        };
        self.remaining.start = end;
        Some(Work {
            interval,
            recovery: None,
        })
    }

    fn earliest(&self) -> Option<usize> {
        self.pending
            .iter()
            .enumerate()
            .min_by_key(|(_, pending)| pending.delay.remaining())
            .map(|(index, _)| index)
    }

    fn next_delay(&self) -> Option<Duration> {
        self.earliest()
            .map(|index| self.pending[index].delay.remaining())
    }

    async fn wait_next(&mut self, wait: &mut WaitBudget) -> Result<bool, TransferError> {
        let Some(index) = self.earliest() else {
            return Ok(false);
        };
        let delay = self.pending[index].delay.remaining();
        if !wait.allows(delay) {
            let failed = self.pending.swap_remove(index);
            return Err(TransferError::RangeWaitExceeded(Box::new(failed.error)));
        }
        wait.wait(delay).await;
        Ok(true)
    }

    fn next_size(&self, available: u64) -> u64 {
        let bytes = available.min(self.chunk);
        if self.minimum != MIN_RANGE_BYTES || !(1..self.minimum).contains(&(available - bytes)) {
            return bytes;
        }
        if available < 2 * self.minimum {
            available
        } else {
            available - self.minimum
        }
    }

    fn completed(
        &mut self,
        attempts: &'a Attempts,
        interval: ByteRange,
        result: CopyResult,
        elapsed: Duration,
    ) -> Result<u64, TransferError> {
        let Err(error) = result.result else {
            let transferred = interval.end - interval.start;
            let suggested =
                (transferred as f64 * SEGMENT_TIME.as_secs_f64() / elapsed.as_secs_f64()) as u64;
            self.chunk = suggested.clamp(self.minimum, MAX_SEGMENT);
            return Ok(transferred);
        };
        if !error.retryable() || error.origin_throttle() {
            return Err(error);
        }
        let Some(recovery) = attempts.reserve() else {
            let delay = self
                .pending
                .iter()
                .map(|pending| pending.delay.remaining())
                .max();
            return Err(TransferError::RangeRetriesExhausted {
                error: Box::new(error),
                pending_delay: delay,
            });
        };
        let start = if result.end < interval.end {
            result.end
        } else {
            interval.start
        };
        self.pending.push(Pending {
            delay: recovery.delay(error.retry_hint()),
            work: Work {
                interval: ByteRange {
                    start,
                    end: interval.end,
                },
                recovery: Some(recovery),
            },
            error,
        });
        Ok(start - interval.start)
    }
}
