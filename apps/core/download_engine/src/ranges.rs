// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axlauncher_spi::{ByteRange, DownloadBody, Network};
use axlauncher_storage::AtomicFile;
use futures::{StreamExt, stream::FuturesUnordered};

use crate::transfer::{Connection, CopyResult, Transfer, TransferError};
use crate::weighted::MIN_RANGE_BYTES;

pub(crate) const MIN_SEGMENT: u64 = 256 * 1024;
pub(crate) const INITIAL_SEGMENT: u64 = 8 * 1024 * 1024;
const MAX_SEGMENT: u64 = 32 * 1024 * 1024;
const SEGMENT_TIME: Duration = Duration::from_secs(2);
const SEGMENT_RETRIES: usize = 3;

#[derive(Clone, Copy)]
pub(crate) struct RangeSource {
    pub(crate) total: u64,
    pub(crate) width: usize,
    pub(crate) chunk: u64,
    pub(crate) minimum: u64,
}

#[derive(Clone, Copy)]
struct Work {
    interval: ByteRange,
    retries: usize,
}

pub(crate) async fn download<Net: Network>(
    transfer: &Transfer<'_, Net>,
    stage: &Mutex<&mut AtomicFile>,
    source: RangeSource,
    first: ByteRange,
    connection: Connection<'_, impl DownloadBody>,
    final_round: bool,
) -> Result<(), TransferError> {
    let mut work = Schedule {
        remaining: ByteRange {
            start: first.end,
            end: source.total,
        },
        retries: VecDeque::new(),
        chunk: source.chunk,
        minimum: source.minimum,
    };
    let mut active = FuturesUnordered::new();
    active.push(run(
        transfer,
        stage,
        Work {
            interval: first,
            retries: 0,
        },
        source,
        final_round,
        Some(connection),
    ));
    let mut demand = source.width;
    loop {
        let available = active.len()..source.width;
        active.extend(
            available
                .map_while(|_| work.next())
                .map(|next| run(transfer, stage, next, source, final_round, None)),
        );
        if let Some(share) = transfer.share
            && demand != active.len().max(1)
        {
            demand = active.len().max(1);
            share.set_width(demand);
        }
        let Some((item, result, elapsed)) = active.next().await else {
            return Ok(());
        };
        let written = work.completed(item, result, elapsed)?;
        if let Some(share) = transfer.share {
            share.record_written(written);
        }
    }
}

async fn run<Net: Network>(
    transfer: &Transfer<'_, Net>,
    stage: &Mutex<&mut AtomicFile>,
    work: Work,
    source: RangeSource,
    final_round: bool,
    initial: Option<Connection<'_, impl DownloadBody>>,
) -> (Work, CopyResult, Duration) {
    let start = Instant::now();
    let split_slow = !final_round && work.retries < SEGMENT_RETRIES;
    let result = transfer
        .segment(stage, work.interval, source.total, split_slow, initial)
        .await;
    (work, result, start.elapsed())
}

struct Schedule {
    remaining: ByteRange,
    retries: VecDeque<Work>,
    chunk: u64,
    minimum: u64,
}

impl Schedule {
    fn next(&mut self) -> Option<Work> {
        if let Some(work) = self.retries.pop_front() {
            return Some(work);
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
            retries: 0,
        })
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
        work: Work,
        result: CopyResult,
        elapsed: Duration,
    ) -> Result<u64, TransferError> {
        let Err(error) = result.result else {
            let transferred = work.interval.end - work.interval.start;
            let suggested =
                (transferred as f64 * SEGMENT_TIME.as_secs_f64() / elapsed.as_secs_f64()) as u64;
            self.chunk = suggested.clamp(self.minimum, MAX_SEGMENT);
            return Ok(transferred);
        };
        if !error.retryable() || work.retries >= SEGMENT_RETRIES {
            return Err(error);
        }
        let start = if result.end < work.interval.end {
            result.end
        } else {
            work.interval.start
        };
        self.requeue(
            ByteRange {
                start,
                end: work.interval.end,
            },
            work.retries + 1,
        );
        Ok(start - work.interval.start)
    }

    fn requeue(&mut self, interval: ByteRange, retries: usize) {
        let remaining = interval.end - interval.start;
        if remaining >= 2 * self.minimum {
            let middle = interval.start + remaining / 2;
            self.retries.push_back(Work {
                interval: ByteRange {
                    start: interval.start,
                    end: middle,
                },
                retries,
            });
            self.retries.push_back(Work {
                interval: ByteRange {
                    start: middle,
                    end: interval.end,
                },
                retries,
            });
        } else {
            self.retries.push_back(Work { interval, retries });
        }
    }
}
