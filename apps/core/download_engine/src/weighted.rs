// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::{BTreeMap, BinaryHeap};
use std::sync::Mutex;

use tokio::sync::watch;

pub(crate) const MIN_RANGE_BYTES: u64 = 16 * 1024;

pub(crate) struct WeightedConnections {
    state: Mutex<State>,
}

struct State {
    limit: usize,
    next_id: u64,
    files: BTreeMap<u64, FileDemand>,
}

struct FileDemand {
    remaining: Option<u64>,
    width: usize,
    active: usize,
    target: usize,
    changed: watch::Sender<usize>,
}

impl WeightedConnections {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            state: Mutex::new(State {
                limit,
                next_id: 0,
                files: BTreeMap::new(),
            }),
        }
    }

    pub(crate) fn set_limit(&self, limit: usize) {
        self.rebalance(|state| state.limit = limit);
    }

    pub(crate) fn register(&self, remaining: Option<u64>, width: usize) -> FileShare<'_> {
        let changed = watch::Sender::new(0);
        let id = self.rebalance(|state| {
            let id = state.next_id;
            state.next_id += 1;
            state.files.insert(
                id,
                FileDemand {
                    remaining,
                    width,
                    active: 0,
                    target: 0,
                    changed: changed.clone(),
                },
            );
            id
        });
        FileShare {
            pool: self,
            id,
            changed,
        }
    }

    fn rebalance<T>(&self, update: impl FnOnce(&mut State) -> T) -> T {
        let mut state = self.state.lock().expect("HTTP/1.1 connection shares");
        let result = update(&mut state);
        state.allocate();
        state.files.values().for_each(FileDemand::publish_target);
        result
    }
}

impl State {
    fn allocate(&mut self) {
        let total_demand = self
            .files
            .values()
            .fold(0usize, |sum, file| sum.saturating_add(file.cap()));
        if total_demand <= self.limit {
            self.files
                .values_mut()
                .for_each(|file| file.target = file.cap());
            return;
        }

        let mut candidates = BinaryHeap::new();
        let mut admitted = 0;
        for (&id, file) in &mut self.files {
            file.target = usize::from(file.cap() > 0 && admitted < self.limit);
            admitted += file.target;
            candidates.extend(file.priority(id));
        }
        // Highest averages with one slot reserved for each admitted file.
        let mut slots = admitted..self.limit;
        while let Some((_, std::cmp::Reverse(id))) = slots.next().and_then(|_| candidates.pop()) {
            let file = self.files.get_mut(&id).expect("allocated file");
            file.target += 1;
            candidates.extend(file.priority(id));
        }
    }
}

impl FileDemand {
    fn publish_target(&self) {
        self.changed.send_if_modified(|published| {
            let increased = self.target > *published;
            *published = self.target;
            increased
        });
    }

    fn cap(&self) -> usize {
        if self.width == 0 {
            return 0;
        }
        self.remaining
            .map_or(1, |remaining| range_width(remaining, self.width))
    }

    fn priority(&self, id: u64) -> Option<(u128, std::cmp::Reverse<u64>)> {
        (self.target > 0 && self.target < self.cap()).then(|| {
            let weight = u128::from(self.remaining.unwrap_or(1));
            // A fixed-point quotient keeps ties deterministic for small files.
            let priority = (weight << 32) / (self.target as u128 + 1);
            (priority, std::cmp::Reverse(id))
        })
    }
}

pub(crate) fn range_width(total: u64, limit: usize) -> usize {
    usize::try_from(total / MIN_RANGE_BYTES)
        .unwrap_or(usize::MAX)
        .clamp(1, limit)
}

pub(crate) struct FileShare<'a> {
    pool: &'a WeightedConnections,
    id: u64,
    changed: watch::Sender<usize>,
}

impl FileShare<'_> {
    pub(crate) fn set_remaining(&self, remaining: Option<u64>) {
        self.pool.rebalance(|state| {
            state
                .files
                .get_mut(&self.id)
                .expect("registered file")
                .remaining = remaining
        });
    }

    pub(crate) fn set_width(&self, width: usize) {
        self.pool.rebalance(|state| {
            state
                .files
                .get_mut(&self.id)
                .expect("registered file")
                .width = width
        });
    }

    pub(crate) fn record_written(&self, bytes: u64) {
        if bytes == 0 {
            return;
        }
        self.pool.rebalance(|state| {
            let file = state.files.get_mut(&self.id).expect("registered file");
            file.remaining = file
                .remaining
                .map(|remaining| remaining.saturating_sub(bytes));
        });
    }

    pub(crate) async fn acquire(&self) -> ShareLease<'_> {
        let mut changed = self.changed.subscribe();
        while !self.try_acquire() {
            let _ = changed.changed().await;
        }
        ShareLease(self)
    }

    fn try_acquire(&self) -> bool {
        let mut state = self.pool.state.lock().expect("HTTP/1.1 connection shares");
        let file = state.files.get_mut(&self.id).expect("registered file");
        if file.active >= file.target {
            return false;
        }
        file.active += 1;
        true
    }
}

impl Drop for FileShare<'_> {
    fn drop(&mut self) {
        self.pool.rebalance(|state| state.files.remove(&self.id));
    }
}

pub(crate) struct ShareLease<'a>(&'a FileShare<'a>);

impl Drop for ShareLease<'_> {
    fn drop(&mut self) {
        let mut state = self
            .0
            .pool
            .state
            .lock()
            .expect("HTTP/1.1 connection shares");
        let file = state.files.get_mut(&self.0.id).expect("registered file");
        file.active -= 1;
        if file.active < file.target {
            file.changed.send_replace(file.target);
        }
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/download_engine/src/weighted.rs"]
mod tests;
