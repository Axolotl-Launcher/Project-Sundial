// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use crate::test_support::TempRoot;

fn entry(path: &str, missing: bool) -> FileEntry {
    FileEntry {
        relative_path: path.into(),
        enabled: true,
        sha1: "0123456789abcdef0123456789abcdef01234567".into(),
        size: 10,
        missing,
        added_at: 1_700_000_000_000,
        modified_at: 1_700_000_000_000,
    }
}

#[test]
fn an_index_round_trips() {
    let root = TempRoot::new();
    let mut files = Files::new();
    files.insert(entry("mods/example.jar", false));

    save(root.path(), &files).expect("save");
    let loaded = load(root.path()).expect("load");

    assert_eq!(loaded.origin, FilesOrigin::Stored);
    assert_eq!(loaded.files, files);
}

#[test]
fn a_second_entry_for_the_same_path_replaces_the_first() {
    let mut files = Files::new();
    files.insert(entry("mods/example.jar", false));
    files.insert(entry("mods/example.jar", true));

    assert_eq!(files.len(), 1);
    assert!(files.get("mods/example.jar").expect("entry").missing);
}

#[test]
fn a_missing_entry_stays_in_the_index_and_is_reported() {
    let root = TempRoot::new();
    let mut files = Files::new();
    files.insert(entry("mods/present.jar", false));
    files.insert(entry("mods/removed.jar", true));
    save(root.path(), &files).expect("save");

    let loaded = load(root.path()).expect("load");
    let missing: Vec<&str> = loaded
        .files
        .missing()
        .map(|e| e.relative_path.as_str())
        .collect();

    assert_eq!(missing, ["mods/removed.jar"]);
    assert_eq!(loaded.files.len(), 2);
}

#[test]
fn an_absent_document_is_an_empty_index() {
    let root = TempRoot::new();
    let loaded = load(root.path()).expect("load");

    assert_eq!(loaded.origin, FilesOrigin::Absent);
    assert!(loaded.files.is_empty());
}

#[test]
fn a_corrupt_document_is_reported_rather_than_read_as_empty() {
    let root = TempRoot::new();
    fs::write(path(root.path()), b"{not json").expect("seed");

    let error = load(root.path()).expect_err("corrupt document");
    assert!(matches!(error, FilesError::Parse { .. }));
}

#[test]
fn another_schema_is_read_as_empty_and_never_replaced() {
    let root = TempRoot::new();
    let document = br#"{"schema_version": 99, "files": {}}"#;
    fs::write(path(root.path()), document).expect("seed");

    let loaded = load(root.path()).expect("load");
    assert_eq!(
        loaded.origin,
        FilesOrigin::ReadOnlyOtherSchema { found: 99 }
    );

    let error = save(root.path(), &Files::new()).expect_err("refused");
    assert!(matches!(error, FilesError::RefusedSchema { found: 99, .. }));
    assert_eq!(fs::read(path(root.path())).expect("untouched"), document);
}
