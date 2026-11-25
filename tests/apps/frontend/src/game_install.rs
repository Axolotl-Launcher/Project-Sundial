// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

/// Writes one version document under a fresh game root and reads it back the
/// same way an install does.
fn installed_document(json: &str) -> (tempfile::TempDir, Version) {
    let root = tempfile::tempdir().unwrap();
    let versions = root.path().join("versions");
    let folder = versions.join("1.21");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("1.21.json"), json).unwrap();
    let document = version::load(&versions, "1.21").unwrap();
    (root, document)
}

#[test]
fn a_document_without_java_version_fetches_nothing() {
    let (_root, document) = installed_document(r#"{"id":"1.21"}"#);
    let asked = std::cell::Cell::new(false);

    let required = missing_java(&document, |_| {
        asked.set(true);
        false
    });

    assert_eq!(required, None);
    assert!(!asked.get(), "a version stating no major must not ask");
}

#[test]
fn a_major_with_a_runtime_fetches_nothing() {
    let (_root, document) =
        installed_document(r#"{"id":"1.21","javaVersion":{"majorVersion":21}}"#);
    let asked = std::cell::Cell::new(None);

    let required = missing_java(&document, |major| {
        asked.set(Some(major));
        true
    });

    assert_eq!(required, None);
    assert_eq!(asked.get(), Some(21));
}

#[test]
fn a_major_without_a_runtime_asks_for_that_major() {
    let (_root, document) =
        installed_document(r#"{"id":"1.21","javaVersion":{"majorVersion":21}}"#);

    assert_eq!(missing_java(&document, |_| false), Some(21));
}
