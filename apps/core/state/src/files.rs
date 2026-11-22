// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! The per-instance file index.
//!
//! One document per instance, recording what content the instance holds: the
//! relative path, the hash and size it had, whether the user disabled it, and
//! whether it is currently missing from disk.
//!
//! The collection is a map keyed by relative path, not an array. The reference
//! enforces the same key with a uniqueness constraint; a map makes the
//! constraint structural, so a duplicate cannot be written at all.
//!
//! `missing` is the field a launch reads: an entry marked missing is one the
//! launcher knows about but the disk does not have, which is the difference
//! between "the user never installed this" and "the file was removed".

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use axlauncher_storage::write_atomic;
use serde::{Deserialize, Serialize};

use crate::document::{self, Existing};

/// The only file-index schema this build reads.
pub const FILES_SCHEMA_VERSION: u32 = 1;

/// The document's name inside an instance directory.
pub const FILES_FILE: &str = "files.json";

/// One file the instance holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// The path relative to the instance directory, with `/` separators.
    pub relative_path: String,
    /// Whether the game loads the file. A disabled mod stays on disk.
    pub enabled: bool,
    /// The SHA-1 of the contents when it was recorded.
    pub sha1: String,
    pub size: u64,
    /// Whether the file was absent the last time the index was checked.
    #[serde(default)]
    pub missing: bool,
    /// Unix milliseconds, as the reference stores them.
    pub added_at: i64,
    pub modified_at: i64,
}

/// The instance's file index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Files {
    entries: BTreeMap<String, FileEntry>,
}

impl Files {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a file, replacing whatever was recorded at its path before.
    pub fn insert(&mut self, entry: FileEntry) {
        self.entries.insert(entry.relative_path.clone(), entry);
    }

    pub fn get(&self, relative_path: &str) -> Option<&FileEntry> {
        self.entries.get(relative_path)
    }

    pub fn remove(&mut self, relative_path: &str) -> Option<FileEntry> {
        self.entries.remove(relative_path)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &FileEntry)> {
        self.entries
            .iter()
            .map(|(path, entry)| (path.as_str(), entry))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The entries whose file is known to be absent.
    pub fn missing(&self) -> impl Iterator<Item = &FileEntry> {
        self.entries.values().filter(|entry| entry.missing)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FilesDocument {
    schema_version: u32,
    #[serde(default)]
    files: BTreeMap<String, FileEntry>,
}

/// What a load found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilesOrigin {
    /// The document was present and current.
    Stored,
    /// There was no document; an empty index is returned.
    Absent,
    /// The document names another schema and was left untouched.
    ReadOnlyOtherSchema { found: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedFiles {
    pub files: Files,
    pub origin: FilesOrigin,
}

#[derive(Debug)]
pub enum FilesError {
    Read {
        path: PathBuf,
        source: io::Error,
    },
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    Write {
        path: PathBuf,
        source: io::Error,
    },
    RefusedSchema {
        path: PathBuf,
        found: u32,
    },
    RefusedUnreadable {
        path: PathBuf,
    },
}

impl fmt::Display for FilesError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(formatter, "cannot read {}: {source}", path.display())
            }
            Self::Parse { path, source } => {
                write!(formatter, "cannot parse {}: {source}", path.display())
            }
            Self::Write { path, source } => {
                write!(formatter, "cannot write {}: {source}", path.display())
            }
            Self::RefusedSchema { path, found } => write!(
                formatter,
                "{} uses schema {found}; this build reads schema {FILES_SCHEMA_VERSION}",
                path.display()
            ),
            Self::RefusedUnreadable { path } => {
                write!(
                    formatter,
                    "{} is unreadable; refusing to replace it",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for FilesError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } | Self::Write { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::RefusedSchema { .. } | Self::RefusedUnreadable { .. } => None,
        }
    }
}

/// The document's path inside `instance_directory`.
pub fn path(instance_directory: &Path) -> PathBuf {
    instance_directory.join(FILES_FILE)
}

/// Reads an instance's file index.
///
/// An absent document is an empty index, not an error: an instance with no
/// content recorded is the state every instance starts in. A document from
/// another schema is returned as empty and marked read-only, so a save never
/// overwrites it.
pub fn load(instance_directory: &Path) -> Result<LoadedFiles, FilesError> {
    let path = path(instance_directory);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(LoadedFiles {
                files: Files::new(),
                origin: FilesOrigin::Absent,
            });
        }
        Err(source) => return Err(FilesError::Read { path, source }),
    };

    let document: FilesDocument =
        serde_json::from_slice(&bytes).map_err(|source| FilesError::Parse {
            path: path.clone(),
            source,
        })?;

    if document.schema_version != FILES_SCHEMA_VERSION {
        return Ok(LoadedFiles {
            files: Files::new(),
            origin: FilesOrigin::ReadOnlyOtherSchema {
                found: document.schema_version,
            },
        });
    }

    Ok(LoadedFiles {
        files: Files {
            entries: document.files,
        },
        origin: FilesOrigin::Stored,
    })
}

/// Writes an instance's file index.
///
/// The write refuses to replace a document this build does not own, for the
/// same reason the other documents do: the newer build's data must survive.
pub fn save(instance_directory: &Path, files: &Files) -> Result<(), FilesError> {
    let path = path(instance_directory);

    match document::inspect(&path).map_err(|source| FilesError::Read {
        path: path.clone(),
        source,
    })? {
        Existing::Unreadable => {
            return Err(FilesError::RefusedUnreadable { path });
        }
        Existing::Schema(found) if found != FILES_SCHEMA_VERSION => {
            return Err(FilesError::RefusedSchema { path, found });
        }
        Existing::Missing | Existing::Schema(_) => {}
    }

    let document = FilesDocument {
        schema_version: FILES_SCHEMA_VERSION,
        files: files.entries.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&document).map_err(|source| FilesError::Write {
        path: path.clone(),
        source: io::Error::other(source),
    })?;
    write_atomic(&path, &bytes).map_err(|source| FilesError::Write { path, source })
}

#[cfg(test)]
mod tests {
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
}
