// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashMap;
use std::hash::Hash;
use std::num::{NonZeroU64, NonZeroUsize};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use tokio::sync::{Mutex as AsyncMutex, Notify, watch};
use tokio::time::Instant;

pub(crate) const BUFFER_BYTES: usize = 64 * 1024;

#[derive(Clone)]
pub(crate) struct Quota(Arc<QuotaState>);

struct QuotaState {
    usage: Mutex<Usage>,
    queue: AsyncMutex<()>,
    changed: Notify,
}

struct Usage {
    limit: usize,
    active: usize,
}

impl Quota {
    pub(crate) fn new(limit: NonZeroUsize) -> Self {
        Self(Arc::new(QuotaState {
            usage: Mutex::new(Usage {
                limit: limit.get(),
                active: 0,
            }),
            queue: AsyncMutex::new(()),
            changed: Notify::new(),
        }))
    }

    pub(crate) fn set_limit(&self, limit: NonZeroUsize) {
        self.0.usage.lock().expect("connection quota lock").limit = limit.get();
        self.0.changed.notify_one();
    }

    pub(crate) fn active(&self) -> usize {
        self.0.usage.lock().expect("connection quota lock").active
    }

    pub(crate) async fn acquire(&self) -> Lease {
        // Tokio's FIFO mutex keeps a single waiter at the head of each quota.
        let _position = self.0.queue.lock().await;
        while !self.try_acquire() {
            self.0.changed.notified().await;
        }
        Lease(self.clone())
    }

    fn try_acquire(&self) -> bool {
        let mut usage = self.0.usage.lock().expect("connection quota lock");
        if usage.active >= usage.limit {
            return false;
        }
        usage.active += 1;
        true
    }
}

pub(crate) struct Lease(Quota);

impl Drop for Lease {
    fn drop(&mut self) {
        self.0.0.usage.lock().expect("connection quota lock").active -= 1;
        self.0.0.changed.notify_one();
    }
}

#[derive(Default)]
pub(crate) struct Quotas<Key>(Mutex<HashMap<Key, Weak<QuotaState>>>);

impl<Key: Eq + Hash> Quotas<Key> {
    pub(crate) fn for_key(&self, key: Key, limit: NonZeroUsize) -> Quota {
        let mut quotas = self.0.lock().expect("download quota lock");
        if let Some(quota) = quotas.get(&key).and_then(Weak::upgrade) {
            return Quota(quota);
        }
        quotas.retain(|_, quota| quota.strong_count() != 0);
        let quota = Quota::new(limit);
        quotas.insert(key, Arc::downgrade(&quota.0));
        quota
    }
}

pub(crate) struct Bandwidth {
    changed: watch::Sender<()>,
    bucket: Mutex<Bucket>,
    queue: AsyncMutex<()>,
}

struct Bucket {
    rate: Option<NonZeroU64>,
    tokens: f64,
    updated: Instant,
}

impl Bandwidth {
    pub(crate) fn new(rate: Option<NonZeroU64>) -> Self {
        Self {
            changed: watch::Sender::new(()),
            bucket: Mutex::new(Bucket {
                rate,
                tokens: 0.0,
                updated: Instant::now(),
            }),
            queue: AsyncMutex::new(()),
        }
    }

    pub(crate) fn set_limit(&self, rate: Option<NonZeroU64>) {
        let mut bucket = self.bucket.lock().expect("bandwidth lock");
        if bucket.rate != rate {
            bucket.refill();
            bucket.rate = rate;
            bucket.updated = Instant::now();
            self.changed.send_replace(());
        }
    }

    pub(crate) async fn acquire(&self, count: usize) {
        let _position = self.queue.lock().await;
        let mut changed = self.changed.subscribe();
        while let Some(delay) = self.delay(count) {
            let timer = std::pin::pin!(tokio::time::sleep(delay));
            let update = std::pin::pin!(changed.changed());
            let _ = futures::future::select(timer, update).await;
        }
    }

    fn delay(&self, count: usize) -> Option<Duration> {
        self.bucket.lock().expect("bandwidth lock").charge(count)
    }

    pub(crate) fn is_limited(&self) -> bool {
        self.bucket.lock().expect("bandwidth lock").rate.is_some()
    }
}

impl Bucket {
    fn refill(&mut self) {
        let now = Instant::now();
        if let Some(rate) = self.rate {
            let rate = rate.get() as f64;
            self.tokens = (self.tokens + (now - self.updated).as_secs_f64() * rate)
                .min(rate.max(BUFFER_BYTES as f64));
        }
        self.updated = now;
    }

    fn charge(&mut self, count: usize) -> Option<Duration> {
        let rate = self.rate?.get() as f64;
        self.refill();
        if self.tokens >= count as f64 {
            self.tokens -= count as f64;
            return None;
        }
        Some(Duration::from_secs_f64((count as f64 - self.tokens) / rate))
    }
}
