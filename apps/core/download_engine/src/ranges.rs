// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axlauncher_spi::{ByteRange, DownloadBody, Network};
use axlauncher_storage::AtomicFile;
use futures::{StreamExt, stream::FuturesUnordered};

use crate::transfer::{Connection, CopyResult, Transfer, TransferError};

const MIN_SEGMENT: u64 = 256 * 1024;
pub(crate) const INITIAL_SEGMENT: u64 = 8 * 1024 * 1024;
const MAX_SEGMENT: u64 = 32 * 1024 * 1024;
const SEGMENT_TIME: Duration = Duration::from_secs(2);
const SEGMENT_RETRIES: usize = 3;

#[derive(Clone, Copy)]
pub(crate) struct RangeSource<'a> {
    pub(crate) total: u64,
    pub(crate) validator: Option<&'a str>,
}

#[derive(Clone, Copy)]
struct Work {
    interval: ByteRange,
    retries: usize,
}

pub(crate) async fn download<Net: Network>(
    transfer: &Transfer<'_, Net>,
    stage: &Mutex<&mut AtomicFile>,
    source: RangeSource<'_>,
    first: ByteRange,
    connection: Connection<impl DownloadBody>,
    final_round: bool,
) -> Result<(), TransferError> {
    let width = transfer.engine.policy.connections_per_file.get();
    let mut work = Schedule {
        remaining: ByteRange {
            start: first.end,
            end: source.total,
        },
        retries: VecDeque::new(),
        chunk: INITIAL_SEGMENT,
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
    loop {
        let available = active.len()..width;
        active.extend(
            available
                .map_while(|_| work.next())
                .map(|next| run(transfer, stage, next, source, final_round, None)),
        );
        let Some((item, result, elapsed)) = active.next().await else {
            return Ok(());
        };
        work.completed(item, result, elapsed)?;
    }
}

async fn run<Net: Network>(
    transfer: &Transfer<'_, Net>,
    stage: &Mutex<&mut AtomicFile>,
    work: Work,
    source: RangeSource<'_>,
    final_round: bool,
    initial: Option<Connection<impl DownloadBody>>,
) -> (Work, CopyResult, Duration) {
    let start = Instant::now();
    let split_slow = !final_round && work.retries < SEGMENT_RETRIES;
    let result = transfer
        .segment(stage, work.interval, source, split_slow, initial)
        .await;
    (work, result, start.elapsed())
}

struct Schedule {
    remaining: ByteRange,
    retries: VecDeque<Work>,
    chunk: u64,
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
        let end = self.remaining.start + available.min(self.chunk);
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

    fn completed(
        &mut self,
        work: Work,
        result: CopyResult,
        elapsed: Duration,
    ) -> Result<(), TransferError> {
        let Err(error) = result.result else {
            let transferred = work.interval.end - work.interval.start;
            let suggested =
                (transferred as f64 * SEGMENT_TIME.as_secs_f64() / elapsed.as_secs_f64()) as u64;
            self.chunk = suggested.clamp(MIN_SEGMENT, MAX_SEGMENT);
            return Ok(());
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
        Ok(())
    }

    fn requeue(&mut self, interval: ByteRange, retries: usize) {
        let remaining = interval.end - interval.start;
        if remaining >= 2 * MIN_SEGMENT {
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
