// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::path::PathBuf;

use super::*;
use crate::test_support::TempRoot;

fn write(root: &Path, name: &str, bytes: &[u8]) -> PathBuf {
    let path = root.join(name);
    fs::write(&path, bytes).expect("fixture");
    path
}

#[test]
fn an_absent_document_is_missing() {
    let temp = TempRoot::new();

    let existing = inspect(&temp.path().join("nothing.json")).expect("inspect");

    assert!(matches!(existing, Existing::Missing));
}

#[test]
fn a_document_with_the_expected_schema_reports_it() {
    let temp = TempRoot::new();
    let path = write(temp.path(), "document.json", br#"{"schema_version":1}"#);

    let existing = inspect(&path).expect("inspect");

    assert!(matches!(existing, Existing::Schema(1)));
}

#[test]
fn a_document_with_another_schema_reports_the_one_it_carries() {
    let temp = TempRoot::new();
    let path = write(temp.path(), "document.json", br#"{"schema_version":7}"#);

    let existing = inspect(&path).expect("inspect");

    assert!(matches!(existing, Existing::Schema(7)));
}

#[test]
fn bytes_that_are_not_json_are_unreadable() {
    let temp = TempRoot::new();
    let path = write(temp.path(), "document.json", b"not a document");

    let existing = inspect(&path).expect("inspect");

    assert!(matches!(existing, Existing::Unreadable));
}

#[test]
fn json_without_a_schema_key_is_unreadable() {
    let temp = TempRoot::new();
    let path = write(temp.path(), "document.json", br#"{"theme":"dark"}"#);

    let existing = inspect(&path).expect("inspect");

    assert!(matches!(existing, Existing::Unreadable));
}

#[test]
fn a_schema_that_is_not_a_non_negative_number_is_unreadable() {
    let temp = TempRoot::new();
    let path = write(temp.path(), "document.json", br#"{"schema_version":"1"}"#);

    let existing = inspect(&path).expect("inspect");

    assert!(matches!(existing, Existing::Unreadable));
}

#[test]
fn a_schema_beyond_the_u32_range_is_never_the_expected_one() {
    let temp = TempRoot::new();
    let path = write(
        temp.path(),
        "document.json",
        br#"{"schema_version":4294967296}"#,
    );

    let existing = inspect(&path).expect("inspect");

    // An out-of-range version is reported at the top of the range, so it can
    // never equal the schema this build writes.
    assert!(matches!(existing, Existing::Schema(u32::MAX)));
}
