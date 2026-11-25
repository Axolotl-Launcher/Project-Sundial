// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::is_version_id;

#[test]
fn versions_can_use_internal_spaces() {
    for id in [
        "1.21",
        "fabric-loader-0.15.11-1.21",
        "1.14 Pre-Release 1",
        "3D Shareware v1.34",
    ] {
        assert!(is_version_id(id), "{id:?} must be accepted");
    }
}

#[test]
fn versions_must_be_portable_directory_names() {
    for id in [
        "",
        ".",
        "..",
        "../outside",
        "nested/one",
        "nested\\one",
        "/absolute",
        "C:\\absolute",
        "1.14.",
        "1.14 ",
        "1.14?",
        "1.14\0",
        "NUL.json",
        "com1",
        "LPT².jar",
    ] {
        assert!(!is_version_id(id), "{id:?} must be rejected");
    }
}
