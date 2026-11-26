// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axlauncher_spi::FetchError;
use tokio::time::Instant;
use url::Origin;

const RECOVERIES: usize = 3;
const FIRST_BACKOFF_MS: u64 = 500;
const COOLDOWN_JITTER_MS: u64 = 100;
const WAIT_LIMIT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy)]
pub(crate) struct Delay {
    started: Instant,
    duration: Duration,
}

impl Delay {
    pub(crate) fn new(duration: Duration) -> Self {
        Self {
            started: Instant::now(),
            duration,
        }
    }

    pub(crate) fn remaining(self) -> Duration {
        self.duration.saturating_sub(self.started.elapsed())
    }

    fn remaining_with(self, grace: Duration) -> Duration {
        self.duration
            .saturating_add(grace)
            .saturating_sub(self.started.elapsed())
    }
}

#[derive(Default)]
pub(crate) struct Cooldowns(Mutex<HashMap<Origin, (u16, Delay)>>);

impl Cooldowns {
    pub(crate) fn remaining(&self, origin: &Origin, grace: Duration) -> Option<(u16, Duration)> {
        let mut cooldowns = self.0.lock().expect("download cooldown lock");
        let &(status, delay) = cooldowns.get(origin)?;
        let remaining = delay.remaining_with(grace);
        if delay
            .remaining_with(Duration::from_millis(COOLDOWN_JITTER_MS))
            .is_zero()
        {
            cooldowns.remove(origin);
        }
        (!remaining.is_zero()).then_some((status, remaining))
    }

    pub(crate) fn observe(&self, origin: &Origin, error: &FetchError, attempts: &Attempts) {
        let (status, duration) = match error {
            FetchError::HttpRetryable {
                status: 429,
                retry_after,
            } => (
                429,
                retry_after.unwrap_or_else(|| attempts.backoff(None).duration),
            ),
            FetchError::HttpRetryable {
                status: 503,
                retry_after: Some(delay),
            } => (503, *delay),
            _ => return,
        };
        let mut cooldowns = self.0.lock().expect("download cooldown lock");
        cooldowns.retain(|_, (_, delay)| {
            !delay
                .remaining_with(Duration::from_millis(COOLDOWN_JITTER_MS))
                .is_zero()
        });
        let limit = cooldowns
            .entry(origin.clone())
            .or_insert_with(|| (status, Delay::new(duration)));
        if limit.1.remaining() <= duration {
            *limit = (status, Delay::new(duration));
        }
    }
}

pub(crate) fn cooldown_jitter() -> Duration {
    Duration::from_millis(fastrand::u64(1..=COOLDOWN_JITTER_MS))
}

#[derive(Default)]
pub(crate) struct Attempts {
    recoveries: AtomicUsize,
    requests: AtomicUsize,
}

impl Attempts {
    pub(crate) fn reserve(&self) -> Option<Recovery<'_>> {
        let index = self
            .recoveries
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                (used < RECOVERIES).then_some(used + 1)
            })
            .ok()?;
        Some(Recovery {
            attempts: self,
            index,
            dispatched: false,
        })
    }

    pub(crate) fn can_recover(&self) -> bool {
        self.recoveries.load(Ordering::Relaxed) < RECOVERIES
    }

    pub(crate) fn backoff(&self, hint: Option<Duration>) -> Delay {
        Delay::new(backoff(self.recoveries.load(Ordering::Relaxed), hint))
    }

    pub(crate) fn record_request(&self) {
        self.requests.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn request_count(&self) -> usize {
        self.requests.load(Ordering::Relaxed)
    }
}

pub(crate) struct Recovery<'a> {
    attempts: &'a Attempts,
    index: usize,
    dispatched: bool,
}

impl Recovery<'_> {
    pub(crate) fn delay(&self, hint: Option<Duration>) -> Delay {
        Delay::new(backoff(self.index, hint))
    }

    pub(crate) fn dispatch(mut self) {
        self.dispatched = true;
    }
}

impl Drop for Recovery<'_> {
    fn drop(&mut self) {
        if !self.dispatched {
            self.attempts.recoveries.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

fn backoff(index: usize, hint: Option<Duration>) -> Duration {
    let window = FIRST_BACKOFF_MS << index.min(RECOVERIES - 1);
    let local = Duration::from_millis(fastrand::u64(1..=window));
    hint.map_or(local, |delay| {
        local.max(delay.saturating_add(cooldown_jitter()))
    })
}

pub(crate) struct WaitBudget(Duration);

impl Default for WaitBudget {
    fn default() -> Self {
        Self(WAIT_LIMIT)
    }
}

impl WaitBudget {
    pub(crate) fn allows(&self, delay: Duration) -> bool {
        delay <= self.0
    }

    pub(crate) async fn wait(&mut self, delay: Duration) {
        let started = Instant::now();
        tokio::time::sleep(delay).await;
        self.charge(started);
    }

    pub(crate) fn charge(&mut self, started: Instant) {
        self.0 = self.0.saturating_sub(started.elapsed());
    }
}
