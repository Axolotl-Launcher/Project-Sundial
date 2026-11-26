// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use futures::executor::block_on;
use futures::{FutureExt, poll};

use super::*;

#[test]
fn contending_files_receive_proportional_shares_and_unused_slots_are_reclaimed() {
    let pool = WeightedConnections::new(12);
    let large = pool.register(Some(12 * MIN_RANGE_BYTES), 16);
    let small = pool.register(Some(4 * MIN_RANGE_BYTES), 16);
    let target = |share: &FileShare<'_>| pool.state.lock().unwrap().files[&share.id].target;

    assert_eq!((target(&large), target(&small)), (9, 3));
    large.record_written(10 * MIN_RANGE_BYTES);
    assert_eq!((target(&large), target(&small)), (2, 4));
    drop(large);
    pool.set_limit(2);
    assert_eq!(target(&small), 2);
}

#[test]
fn a_waiter_wakes_when_the_admitted_file_finishes() {
    block_on(async {
        let pool = WeightedConnections::new(1);
        let first = pool.register(None, 16);
        let second = pool.register(Some(64 * MIN_RANGE_BYTES), 16);
        let active = first.acquire().await;
        let mut waiting = second.acquire().boxed();
        assert!(poll!(&mut waiting).is_pending());
        drop(active);
        drop(first);
        let _connection = waiting.await;
    });
}

#[test]
fn a_file_can_finish_its_current_requests_after_its_share_shrinks() {
    block_on(async {
        let pool = WeightedConnections::new(3);
        let first = pool.register(Some(16 * MIN_RANGE_BYTES), 16);
        let first_two = (first.acquire().await, first.acquire().await);
        let extra = first.acquire().await;
        let second = pool.register(Some(8 * MIN_RANGE_BYTES), 16);
        let _second_slot = second.acquire().await;
        let mut replacement = first.acquire().boxed();
        assert!(poll!(&mut replacement).is_pending());
        drop(extra);
        assert!(poll!(&mut replacement).is_pending());
        drop(first_two);
        let _replacement = replacement.await;
    });
}
