// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::fs;
use std::io::{Read, Write};

use super::*;

fn tempdir() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

/// The names beside `destination`, excluding the destination itself.
fn neighbours(destination: &Path) -> Vec<String> {
    let parent = destination.parent().expect("parent");
    let destination_name = destination
        .file_name()
        .expect("file name")
        .to_string_lossy();
    fs::read_dir(parent)
        .expect("read parent")
        .map(|entry| {
            entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .filter(|name| name.as_str() != destination_name)
        .collect()
}

#[test]
fn a_committed_file_replaces_the_existing_destination() {
    let directory = tempdir();
    let destination = directory.path().join("settings.json");
    fs::write(&destination, b"old").expect("seed");

    let mut staging = AtomicFile::new(&destination).expect("staging");
    staging.file().write_all(b"new").expect("write");
    staging.commit(&destination).expect("commit");

    assert_eq!(fs::read(&destination).expect("read"), b"new");
}

#[test]
fn a_commit_leaves_the_destination_and_nothing_else() {
    let directory = tempdir();
    let destination = directory.path().join("settings.json");

    let mut staging = AtomicFile::new(&destination).expect("staging");
    staging.file().write_all(b"new").expect("write");
    staging.commit(&destination).expect("commit");

    assert!(neighbours(&destination).is_empty());
}

#[test]
fn the_staging_file_lives_beside_the_destination() {
    let directory = tempdir();
    let destination = directory.path().join("settings.json");

    let staging = AtomicFile::new(&destination).expect("staging");

    // Same directory means the same filesystem, so the final rename is atomic.
    assert_eq!(
        staging.0.path().parent().expect("staging parent"),
        directory.path()
    );
    assert!(!destination.exists());
}

#[test]
fn dropping_without_a_commit_leaves_the_destination_and_no_stray_file() {
    let directory = tempdir();
    let destination = directory.path().join("settings.json");
    fs::write(&destination, b"old").expect("seed");

    {
        let mut staging = AtomicFile::new(&destination).expect("staging");
        staging.file().write_all(b"unfinished").expect("write");
    }

    assert_eq!(fs::read(&destination).expect("read"), b"old");
    assert!(neighbours(&destination).is_empty());
}

#[test]
fn staging_creates_a_missing_parent_directory() {
    let directory = tempdir();
    let destination = directory
        .path()
        .join("nested")
        .join("deeper")
        .join("file.json");

    let mut staging = AtomicFile::new(&destination).expect("staging");
    staging.file().write_all(b"data").expect("write");
    staging.commit(&destination).expect("commit");

    assert_eq!(fs::read(&destination).expect("read"), b"data");
}

#[test]
fn a_rewound_staging_file_reads_from_its_first_byte() {
    let directory = tempdir();
    let destination = directory.path().join("file.bin");

    let mut staging = AtomicFile::new(&destination).expect("staging");
    staging.file().write_all(b"payload").expect("write");

    let mut read_back = Vec::new();
    staging
        .rewind()
        .expect("rewind")
        .read_to_end(&mut read_back)
        .expect("read");

    assert_eq!(read_back, b"payload");
    staging.commit(&destination).expect("commit");
    assert_eq!(fs::read(&destination).expect("read"), b"payload");
}
