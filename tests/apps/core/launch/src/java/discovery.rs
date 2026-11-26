// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

#[test]
fn launches_share_recent_search_paths_and_refresh_replaces_them() {
    let mut search = None;
    let old = search_paths(&mut search, false, SEARCH_MAX_AGE, || {
        vec![PathBuf::from("old")]
    });
    let cached = search_paths(&mut search, false, SEARCH_MAX_AGE, Vec::new);
    assert!(Arc::ptr_eq(&old, &cached));

    let refreshed = search_paths(&mut search, true, SEARCH_MAX_AGE, || {
        vec![PathBuf::from("new")]
    });
    assert_eq!(&*refreshed, [PathBuf::from("new")]);

    let expired = search_paths(&mut search, false, Duration::ZERO, Vec::new);
    assert!(expired.is_empty());
}
