// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

#[test]
fn a_reply_from_the_newest_request_is_applied_and_an_older_one_is_dropped() {
    let generation = AtomicU64::new(4);

    assert!(!is_stale(&generation, 4));
    assert!(is_stale(&generation, 3));
    assert!(
        is_stale(&generation, 5),
        "a reply numbered ahead of the counter is not current either"
    );
}
