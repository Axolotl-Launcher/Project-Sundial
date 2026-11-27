// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::JavaRelease;

#[test]
fn release_order_compares_updates_stages_and_builds_numerically() {
    let releases = [
        "25-ea+99",
        "25",
        "25.0.9+17-LTS",
        "25.0.10+2-LTS",
        "25.0.10+10-LTS",
    ];
    for pair in releases.windows(2) {
        assert!(JavaRelease::parse(pair[0]).unwrap() < JavaRelease::parse(pair[1]).unwrap());
    }
    assert_eq!(JavaRelease::parse("25"), JavaRelease::parse("25.0.0"));
}

#[test]
fn legacy_updates_and_builds_are_comparable_to_modern_runtime_metadata() {
    let releases = ["1.8.0_99-b12", "1.8.0_112-b9", "1.8.0_112-b10", "8.0.113"];
    for pair in releases.windows(2) {
        assert!(JavaRelease::parse(pair[0]).unwrap() < JavaRelease::parse(pair[1]).unwrap());
    }
}
