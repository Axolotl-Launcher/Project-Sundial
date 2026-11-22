// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::path::Path;

use super::*;

#[test]
fn unsafe_version_ids_are_rejected() {
    for id in [
        "",
        ".",
        "..",
        "../escape",
        "nested/one",
        "nested\\one",
        "/absolute",
        "C:\\absolute",
    ] {
        assert!(!safe_component(id), "{id:?} must be rejected");
    }

    for id in ["1.21", "fabric-loader-0.15.11-1.21"] {
        assert!(safe_component(id), "{id:?} must be accepted");
    }
}

#[test]
fn load_rejects_a_traversal_before_touching_the_filesystem() {
    let error = load(Path::new("versions"), "../escape").expect_err("must reject");

    assert!(matches!(error, VersionError::InvalidId { .. }));
}
