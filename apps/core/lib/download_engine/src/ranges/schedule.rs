// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::time::Duration;

use axlauncher_network_spi::ByteRange;

use crate::retry::{Attempts, Delay, WaitBudget};
use crate::transfer::{CopyResult, TransferError};
use crate::weighted::MIN_RANGE_BYTES;

const MAX_SEGMENT: u64 = 32 * 1024 * 1024;
const SEGMENT_TIME: Duration = Duration::from_secs(2);

pub(super) struct Work {
    interval: ByteRange,
    failure: Option<TransferError>,
}

impl Work {
    pub(super) fn fresh(interval: ByteRange) -> Self {
        Self {
            interval,
            failure: None,
        }
    }

    pub(super) fn interval(&self) -> ByteRange {
        self.interval
    }

    pub(super) fn is_recovery(&self) -> bool {
        self.failure.is_some()
    }

    fn wait_exceeded(self) -> TransferError {
        match self.failure {
            Some(error) => TransferError::RangeWaitExceeded(Box::new(error)),
            None => TransferError::RetryWaitExceeded,
        }
    }

    fn attempts_exceeded(self) -> TransferError {
        match self.failure {
            Some(error) => TransferError::RangeRecoveryExhausted(Box::new(error)),
            None => TransferError::RecoveryExhausted,
        }
    }
}

struct Pending {
    work: Work,
    delay: Delay,
}

pub(super) struct Schedule {
    remaining: ByteRange,
    pending: Vec<Pending>,
    chunk: u64,
    minimum: u64,
}

impl Schedule {
    pub(super) fn new(start: u64, end: u64, chunk: u64, minimum: u64) -> Self {
        Self {
            remaining: ByteRange { start, end },
            pending: Vec::new(),
            chunk,
            minimum,
        }
    }

    pub(super) fn next(&mut self) -> Option<Work> {
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
            failure: None,
        })
    }

    fn earliest(&self) -> Option<usize> {
        self.pending
            .iter()
            .enumerate()
            .min_by_key(|(_, pending)| pending.delay.remaining())
            .map(|(index, _)| index)
    }

    pub(super) fn next_delay(&self) -> Option<Duration> {
        self.earliest()
            .map(|index| self.pending[index].delay.remaining())
    }

    pub(super) async fn wait_next(&mut self, wait: &mut WaitBudget) -> Result<bool, TransferError> {
        let Some(index) = self.earliest() else {
            return Ok(false);
        };
        let delay = self.pending[index].delay.remaining();
        if !wait.allows(delay) {
            let failed = self.pending.swap_remove(index);
            return Err(failed.work.wait_exceeded());
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

    pub(super) fn unwritten(&self) -> u64 {
        self.remaining.end - self.remaining.start
            + self
                .pending
                .iter()
                .map(|pending| pending.work.interval.end - pending.work.interval.start)
                .sum::<u64>()
    }

    pub(super) fn completed(
        &mut self,
        attempts: &Attempts,
        work: Work,
        result: CopyResult,
        elapsed: Duration,
        cooldown: &mut Option<TransferError>,
    ) -> Result<u64, TransferError> {
        let interval = work.interval;
        let end = result.end;
        let Err(error) = result.result else {
            let transferred = interval.end - interval.start;
            let suggested =
                (transferred as f64 * SEGMENT_TIME.as_secs_f64() / elapsed.as_secs_f64()) as u64;
            self.chunk = suggested.clamp(self.minimum, MAX_SEGMENT);
            return Ok(transferred);
        };
        if matches!(error, TransferError::RecoveryExhausted) {
            return Err(work.attempts_exceeded());
        }
        if error.origin_throttle() {
            let start = unwritten_start(interval, end);
            self.pause(work, error, start, result.dispatched, cooldown);
            return Ok(start - interval.start);
        }
        if !error.retryable() {
            return Err(error);
        }
        let start = unwritten_start(interval, end);
        let delay = attempts.backoff(error.retry_hint());
        self.requeue(Pending {
            work: Work {
                interval: ByteRange {
                    start,
                    end: interval.end,
                },
                failure: Some(error),
            },
            delay,
        });
        Ok(start - interval.start)
    }

    fn pause(
        &mut self,
        mut work: Work,
        error: TransferError,
        start: u64,
        dispatched: bool,
        cooldown: &mut Option<TransferError>,
    ) {
        work.interval.start = start;
        if dispatched {
            work.failure = error.throttle_status().map(TransferError::CoolingDown);
        }
        cooldown.get_or_insert(error);
        self.requeue(Pending {
            work,
            delay: Delay::new(Duration::ZERO),
        });
    }

    fn requeue(&mut self, mut pending: Pending) {
        while let Some(index) = self.pending.iter().position(|other| pending.merge(other)) {
            self.pending.swap_remove(index);
        }
        self.pending.push(pending);
    }
}

impl Pending {
    fn merge(&mut self, other: &Self) -> bool {
        let left = self.work.interval;
        let right = other.work.interval;
        if !self.work.is_recovery()
            || !other.work.is_recovery()
            || (left.end != right.start && right.end != left.start)
        {
            return false;
        }
        self.work.interval = ByteRange {
            start: left.start.min(right.start),
            end: left.end.max(right.end),
        };
        if self.delay.remaining() < other.delay.remaining() {
            self.delay = other.delay;
        }
        true
    }
}

fn unwritten_start(interval: ByteRange, end: u64) -> u64 {
    if end < interval.end {
        end
    } else {
        interval.start
    }
}
