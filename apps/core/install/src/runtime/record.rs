// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! What a finished runtime install leaves behind, and how reuse checks it.
//!
//! Runtime files land through a concurrent stream, so a JDK's own `release`
//! file can be written while later entries are still in flight. A kill at that
//! moment would leave a tree that looks installed and is not. The record here
//! is written only after the last file, and it names every file with the digest
//! it must have, so a reuse can tell a finished tree from a partial or damaged
//! one — without a network round trip, and without trusting a single marker
//! file's mere existence.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use super::{RuntimeFile, safe_join};

/// The file a finished install writes, and the only proof a runtime is complete.
const MARKER: &str = ".axlauncher-complete";

/// What a finished install recorded: every file it wrote and that file's digest.
pub(super) type Record = BTreeMap<String, String>;

/// The record a finished install left, when there is one.
///
/// An unreadable or unparsable marker is no record at all: treating it as one
/// would mean trusting a file whose contents could not be checked.
pub(super) fn read(directory: &Path) -> Option<Record> {
    let bytes = fs::read(directory.join(MARKER)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// The record a completed download leaves, taken from what the manifest stated.
///
/// Only files are recorded: a directory is implied by the file beneath it, and
/// links are skipped while installing, so neither can be verified later.
pub(super) fn from_manifest(files: &BTreeMap<String, RuntimeFile>) -> Record {
    files
        .iter()
        .filter(|(_, file)| file.kind == "file")
        .filter_map(|(name, file)| Some((name.clone(), file.digest()?.to_string())))
        .collect()
}

/// Whether the tree still holds every file the record names, unchanged.
///
/// A runtime with a missing or altered file is not the one that was installed,
/// so it is rebuilt rather than trusted.
pub(super) fn matches(directory: &Path, recorded: &Record) -> bool {
    recorded.iter().all(|(name, digest)| {
        let Some(target) = safe_join(directory, name) else {
            return false;
        };
        // Hashing reads the whole tree, and a runtime's largest object is tens
        // of megabytes, so the bytes are streamed rather than held.
        fs::File::open(&target)
            .ok()
            .and_then(|source| axlauncher_network::verify_reader(source, digest).ok())
            .is_some_and(|matches| matches)
    })
}

/// The path the marker occupies inside a runtime's directory.
pub(super) fn marker_path(directory: &Path) -> std::path::PathBuf {
    directory.join(MARKER)
}
