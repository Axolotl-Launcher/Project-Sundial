// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::super::RuntimeFile;
use super::{Record, from_manifest, marker_path, matches, read};
use crate::test_tempfile as tempfile;

const JAVA: &[u8] = b"java binary";
const PLAIN: &[u8] = b"plain file";

fn digest(bytes: &[u8]) -> String {
    let mut hash = crate::test_sha1::Sha1::new();
    hash.update(bytes);
    hash.digest().to_string()
}

fn runtime_file(json: &str) -> RuntimeFile {
    serde_json::from_str(json).expect("runtime file")
}

fn raw_file(sha1: &str) -> String {
    format!(
        r#"{{"type":"file","downloads":{{"raw":{{"url":"https://example.test/file","sha1":"{sha1}"}}}}}}"#
    )
}

fn put(root: &Path, name: &str, bytes: &[u8]) {
    let path = root.join(name);
    fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    fs::write(path, bytes).expect("fixture");
}

fn record_of(entries: &[(&str, &[u8])]) -> Record {
    entries
        .iter()
        .map(|(name, bytes)| ((*name).to_owned(), digest(bytes)))
        .collect()
}

#[test]
fn a_written_record_reads_back_unchanged() {
    let runtime = tempfile::tempdir().expect("tempdir");
    let recorded = record_of(&[("bin/java", JAVA), ("lib/plain.txt", PLAIN)]);
    let bytes = serde_json::to_vec(&recorded).expect("encode");
    fs::write(marker_path(runtime.path()), bytes).expect("marker");

    assert_eq!(read(runtime.path()), Some(recorded));
}

#[test]
fn the_marker_sits_at_the_completion_name_inside_the_runtime() {
    let directory = Path::new("runtimes").join("java-runtime-gamma");

    assert_eq!(
        marker_path(&directory),
        directory.join(".axlauncher-complete")
    );
}

#[test]
fn a_runtime_without_a_marker_has_no_record() {
    let runtime = tempfile::tempdir().expect("tempdir");

    assert_eq!(read(runtime.path()), None);
}

#[test]
fn a_marker_that_cannot_be_read_is_no_record() {
    let runtime = tempfile::tempdir().expect("tempdir");
    fs::create_dir(marker_path(runtime.path())).expect("directory at the marker path");

    assert_eq!(read(runtime.path()), None);
}

#[test]
fn a_marker_that_is_not_a_record_is_no_record() {
    let runtime = tempfile::tempdir().expect("tempdir");
    fs::write(marker_path(runtime.path()), b"not a record").expect("marker");

    assert_eq!(read(runtime.path()), None);
}

#[test]
fn a_marker_that_is_json_but_not_a_map_is_no_record() {
    let runtime = tempfile::tempdir().expect("tempdir");
    fs::write(marker_path(runtime.path()), b"[1, 2, 3]").expect("marker");

    assert_eq!(read(runtime.path()), None);
}

#[test]
fn only_files_with_a_raw_digest_are_recorded() {
    let java = digest(JAVA);
    let plain = digest(PLAIN);
    let packed = digest(b"packed object");
    let files = BTreeMap::from([
        ("bin/java".to_owned(), runtime_file(&raw_file(&java))),
        ("lib/plain.txt".to_owned(), runtime_file(&raw_file(&plain))),
        (
            "lib/packed.txt".to_owned(),
            runtime_file(&format!(
                r#"{{"type":"file","downloads":{{"lzma":{{"url":"https://example.test/packed","sha1":"{packed}"}},"raw":{{"url":"https://example.test/plain","sha1":"{plain}"}}}}}}"#
            )),
        ),
        ("lib".to_owned(), runtime_file(r#"{"type":"directory"}"#)),
        (
            "bin/current".to_owned(),
            runtime_file(r#"{"type":"link","target":"bin/java"}"#),
        ),
        (
            "lib/lzma-only.txt".to_owned(),
            runtime_file(&format!(
                r#"{{"type":"file","downloads":{{"lzma":{{"url":"https://example.test/packed","sha1":"{packed}"}}}}}}"#
            )),
        ),
        (
            "lib/no-downloads.txt".to_owned(),
            runtime_file(r#"{"type":"file"}"#),
        ),
    ]);

    let recorded = from_manifest(&files);
    let expected = BTreeMap::from([
        ("bin/java".to_owned(), java),
        ("lib/packed.txt".to_owned(), plain),
        ("lib/plain.txt".to_owned(), digest(PLAIN)),
    ]);

    assert_eq!(recorded, expected);
    assert!(!recorded.contains_key("lib"));
    assert!(!recorded.contains_key("bin/current"));
    assert!(!recorded.contains_key("lib/lzma-only.txt"));
    assert!(!recorded.contains_key("lib/no-downloads.txt"));
}

#[test]
fn an_intact_tree_matches_its_record() {
    let runtime = tempfile::tempdir().expect("tempdir");
    put(runtime.path(), "bin/java", JAVA);
    put(runtime.path(), "lib/plain.txt", PLAIN);
    let recorded = record_of(&[("bin/java", JAVA), ("lib/plain.txt", PLAIN)]);

    assert!(matches(runtime.path(), &recorded));
}

#[test]
fn a_recorded_file_that_is_missing_does_not_match() {
    let runtime = tempfile::tempdir().expect("tempdir");
    put(runtime.path(), "bin/java", JAVA);
    let recorded = record_of(&[("bin/java", JAVA), ("lib/plain.txt", PLAIN)]);

    assert!(!matches(runtime.path(), &recorded));
}

#[test]
fn an_altered_file_does_not_match() {
    let runtime = tempfile::tempdir().expect("tempdir");
    put(runtime.path(), "bin/java", b"stale runtime");
    let recorded = record_of(&[("bin/java", JAVA)]);

    assert!(!matches(runtime.path(), &recorded));
}

#[test]
fn a_name_that_leaves_the_directory_never_matches() {
    let root = tempfile::tempdir().expect("tempdir");
    let runtime = root.path().join("runtime");
    fs::create_dir(&runtime).expect("runtime");
    // The traversal target exists, so only refusing the name can keep this false.
    fs::write(root.path().join("escape"), JAVA).expect("outside file");
    let recorded = record_of(&[("../escape", JAVA)]);

    assert!(!matches(&runtime, &recorded));
}
