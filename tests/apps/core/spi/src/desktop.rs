// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashSet;

use super::operations;

#[test]
fn every_desktop_operation_has_a_distinct_key() {
    let listed: Vec<_> = operations::all().collect();
    let distinct: HashSet<_> = listed.iter().collect();

    assert!(listed.contains(&operations::instance::LIST));
    assert!(listed.contains(&operations::install::CREATE_INSTANCE));
    assert_eq!(distinct.len(), listed.len());
}
