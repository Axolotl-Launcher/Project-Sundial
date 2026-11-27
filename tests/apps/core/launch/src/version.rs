// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

#[test]
fn load_rejects_a_traversal_before_touching_the_filesystem() {
    let error = load(&GameDir::standalone("game"), "../escape").expect_err("must reject");

    assert!(matches!(error, VersionError::InvalidId { .. }));
}
