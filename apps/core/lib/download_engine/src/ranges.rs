// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axlauncher_spi::{ByteRange, DownloadBody, Network, ResponseRange};
use axlauncher_storage::AtomicFile;
use futures::{StreamExt, future::Either, stream::FuturesUnordered};
use tokio::sync::watch;

use crate::retry::WaitBudget;
use crate::transfer::{
    Connection, CopyResult, RangeValidator, SegmentActivity, Transfer, TransferError,
};
use crate::weighted::{MIN_RANGE_BYTES, range_width};

mod schedule;
use schedule::{Schedule, Work};

const MIN_SEGMENT: u64 = 256 * 1024;
const INITIAL_SEGMENT: u64 = 8 * 1024 * 1024;

const RANGE_THRESHOLD: u64 = 4 * 1024 * 1024;
const PROBE: ByteRange = ByteRange { start: 0, end: 1 };

#[derive(Default)]
pub(crate) enum TransferMode {
    #[default]
    Discover,
    Ranged(u64),
    Paused(Box<RangeProgress>),
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
            return self.transfer.open_whole(&stage, self.is_recovery()).await;
        }
        let attempt = self.run_ranges(size, &stage, mode, wait).await;
        if !matches!(attempt, Err(TransferError::RangeUnavailable)) {
            return attempt;
        }
        *mode = TransferMode::Whole;
        **stage.lock().expect("staged download lock") = AtomicFile::new(destination)?;
        self.transfer.open_whole(&stage, false).await
    }

    async fn run_ranges(
        &self,
        size: Option<u64>,
        stage: &Mutex<&mut AtomicFile>,
        mode: &mut TransferMode,
        wait: &mut WaitBudget,
    ) -> Result<(), TransferError> {
        if let TransferMode::Paused(progress) = mode {
            let total = progress.source.total;
            let result = self.continue_ranges(stage, progress, wait).await;
            return restore_mode(mode, total, result);
        }
        if let TransferMode::Ranged(total) = *mode {
            return self
                .ranged(stage, total, mode, wait, self.is_recovery())
                .await;
        }
        if let Some(total) = size.filter(|&total| total >= self.range_threshold()) {
            *mode = TransferMode::Ranged(total);
            return self
                .ranged(stage, total, mode, wait, self.is_recovery())
                .await;
        }
        if size.is_none() && self.probe_unknown && self.parallelism > 1 {
            return self.probe(stage, mode, wait).await;
        }
        *mode = TransferMode::Whole;
        self.transfer.open_whole(stage, self.is_recovery()).await
    }

    async fn probe(
        &self,
        stage: &Mutex<&mut AtomicFile>,
        mode: &mut TransferMode,
        wait: &mut WaitBudget,
    ) -> Result<(), TransferError> {
        let connection = self
            .transfer
            .open_for_range(PROBE, None, self.is_recovery(), None)
            .await?;
        let Some(range) = connection.response.range else {
            *mode = TransferMode::Whole;
            return self.transfer.whole(connection, stage).await;
        };
        if range.interval != PROBE
            || range.total < self.range_threshold()
            || (!self.has_digest
                && RangeValidator::from_response(&connection.response, false).is_none())
        {
            return Err(TransferError::RangeUnavailable);
        }
        drop(connection);
        *mode = TransferMode::Ranged(range.total);
        self.ranged(stage, range.total, mode, wait, false).await
    }

    async fn ranged(
        &self,
        stage: &Mutex<&mut AtomicFile>,
        total: u64,
        mode: &mut TransferMode,
        wait: &mut WaitBudget,
        recovery: bool,
    ) -> Result<(), TransferError> {
        let (width, chunk, minimum) = self.range_plan(total);
        if let Some(share) = self.transfer.share {
            share.set_remaining(Some(total));
        }
        let end = total.min(chunk);
        let interval = ByteRange { start: 0, end };
        let connection = self
            .transfer
            .open_for_range(interval, None, recovery, None)
            .await?;
        let Some(range) = connection.response.range else {
            *mode = TransferMode::Whole;
            return self.transfer.whole(connection, stage).await;
        };
        let validator = RangeValidator::from_response(&connection.response, self.has_digest);
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
        let mut progress = RangeProgress::new(source, interval.end);
        let result = download(
            self.transfer,
            stage,
            &mut progress,
            Some((interval, connection)),
            wait,
        )
        .await;
        if result.as_ref().is_err_and(TransferError::origin_throttle) {
            *mode = TransferMode::Paused(Box::new(progress));
        }
        result
    }

    async fn continue_ranges(
        &self,
        stage: &Mutex<&mut AtomicFile>,
        progress: &mut RangeProgress,
        wait: &mut WaitBudget,
    ) -> Result<(), TransferError> {
        if let Some(share) = self.transfer.share {
            share.set_remaining(Some(progress.work.unwritten()));
        }
        download::<Net, Option<bytes::Bytes>>(self.transfer, stage, progress, None, wait).await
    }

    fn is_recovery(&self) -> bool {
        self.transfer.attempts.request_count() != 0
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

pub(crate) struct RangeProgress {
    source: RangeSource,
    work: Schedule,
}

impl RangeProgress {
    fn new(source: RangeSource, first_end: u64) -> Self {
        let work = Schedule::new(first_end, source.total, source.chunk, source.minimum);
        Self { source, work }
    }
}

async fn download<Net: Network, Body: DownloadBody>(
    transfer: &Transfer<'_, Net>,
    stage: &Mutex<&mut AtomicFile>,
    progress: &mut RangeProgress,
    initial: Option<(ByteRange, Connection<'_, Body>)>,
    wait: &mut WaitBudget,
) -> Result<(), TransferError> {
    let RangeProgress { source, work } = progress;
    let mut active = FuturesUnordered::new();
    let (pause, paused) = watch::channel(None);
    if let Some((interval, connection)) = initial {
        active.push(run(
            transfer,
            stage,
            Work::fresh(interval),
            source,
            Some(connection),
            paused.clone(),
        ));
    }
    let mut demand = source.width;
    let mut cooldown = None;
    loop {
        if cooldown.is_none() {
            active.extend(
                (active.len()..source.width)
                    .map_while(|_| work.next())
                    .map(|next| run(transfer, stage, next, source, None, paused.clone())),
            );
        }
        update_demand(transfer, &mut demand, active.len());
        if active.is_empty()
            && let Some(error) = cooldown.take()
        {
            return Err(error);
        }
        if active.is_empty() && !work.wait_next(wait).await? {
            return Ok(());
        }
        if active.is_empty() {
            continue;
        }
        let delay = (cooldown.is_none() && active.len() < source.width)
            .then(|| work.next_delay())
            .flatten();
        let Some((item, result, elapsed)) = complete_or_wake(&mut active, delay, wait).await else {
            continue;
        };
        let written = work.completed(transfer.attempts, item, result, elapsed, &mut cooldown)?;
        if let Some(status) = cooldown.as_ref().and_then(TransferError::throttle_status) {
            pause.send_if_modified(|current| current.replace(status).is_none());
        }
        if let Some(share) = transfer.share {
            share.record_written(written);
        }
    }
}

fn update_demand(transfer: &Transfer<'_, impl Network>, demand: &mut usize, active: usize) {
    if let Some(share) = transfer.share
        && *demand != active
    {
        *demand = active;
        share.set_width(active);
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
    work: Work,
    source: &RangeSource,
    initial: Option<Connection<'_, impl DownloadBody>>,
    mut pause: watch::Receiver<Option<u16>>,
) -> (Work, CopyResult, Duration) {
    let start = Instant::now();
    let interval = work.interval();
    let activity = SegmentActivity::new(interval.start, initial.is_some());
    let copying = transfer.segment(
        stage,
        ResponseRange {
            interval,
            total: source.total,
        },
        source.validator.as_ref(),
        work.is_recovery(),
        &activity,
        initial,
    );
    let result = tokio::select! {
        biased;
        status = pause.wait_for(Option::is_some) => activity.paused(status.expect("active range schedule").expect("cooldown status")),
        result = copying => result,
    };
    (work, result, start.elapsed())
}

fn restore_mode(
    mode: &mut TransferMode,
    total: u64,
    result: Result<(), TransferError>,
) -> Result<(), TransferError> {
    if !result.as_ref().is_err_and(TransferError::origin_throttle) {
        *mode = TransferMode::Ranged(total);
    }
    result
}
