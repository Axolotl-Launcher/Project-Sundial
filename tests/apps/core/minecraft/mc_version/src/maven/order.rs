// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

#[test]
fn dotted_numbers_compare_numerically() {
    assert_eq!(compare("47.4.28", "47.4.29"), Ordering::Less);
    assert_eq!(compare("47.10.0", "47.9.0"), Ordering::Greater);
    assert_eq!(compare("1.0", "1.0.0"), Ordering::Less);
}

#[test]
fn a_pre_release_is_older_than_its_release() {
    assert_eq!(compare("20.2.3-beta", "20.2.3"), Ordering::Less);
    assert_eq!(compare("20.2.3", "20.2.3-beta"), Ordering::Greater);
    assert_eq!(compare("20.2.3-beta.2", "20.2.3-beta.10"), Ordering::Less);
}

#[test]
fn equal_versions_are_equal() {
    assert_eq!(compare("20.2.93", "20.2.93"), Ordering::Equal);
}

#[test]
fn mixed_components_sort_transitively() {
    assert_eq!(compare("2", "10"), Ordering::Less);
    assert_eq!(compare("10", "1a"), Ordering::Less);
    assert_eq!(compare("2", "1a"), Ordering::Less);
    assert_eq!(
        compare("1.999999999999999999999", "1.1000000000000000000000"),
        Ordering::Less
    );
}
