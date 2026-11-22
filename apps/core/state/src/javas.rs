// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! The Java runtimes the user chose and the ones found on this machine.
//!
//! Two kinds of answer live together because they answer one question — which
//! executable runs a game that asks for major version N — but they are not the
//! same kind of fact:
//!
//! - `chosen` is **user intent**: a path the user picked for a major version.
//!   It wins over everything else and is never discarded automatically.
//! - `discovered` is a **cache**: every runtime a scan found, keyed by its
//!   path, with the file's size and modification time so a later run can tell
//!   whether the cached details still describe the file on disk.
//!
//! Keeping the two apart means a rescan can rebuild `discovered` freely without
//! touching what the user chose.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use axlauncher_storage::write_atomic;
use serde::{Deserialize, Serialize};

use crate::document::{self, Existing};

/// The only schema this build reads.
pub const JAVAS_SCHEMA_VERSION: u32 = 1;

/// The document's name under the data root.
pub const JAVAS_FILE: &str = "javas.json";

/// What a scan learned about one runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredJava {
    pub major_version: u32,
    pub full_version: String,
    pub architecture: String,
    /// The file's size when the details were recorded, in bytes.
    pub file_size: u64,
    /// The file's modification time when the details were recorded, in Unix
    /// milliseconds.
    pub file_mtime_ms: i64,
}

impl DiscoveredJava {
    /// Whether this record still describes the file at `path`.
    ///
    /// A runtime replaced in place keeps its path but changes size or time, and
    /// the recorded version would then be a lie about a different binary.
    pub fn matches_file(&self, path: &Path) -> bool {
        let Ok(metadata) = fs::metadata(path) else {
            return false;
        };
        if metadata.len() != self.file_size {
            return false;
        }
        modification_ms(&metadata).is_some_and(|ms| ms == self.file_mtime_ms)
    }
}

/// A file's modification time in Unix milliseconds.
pub fn modification_ms(metadata: &fs::Metadata) -> Option<i64> {
    let modified = metadata.modified().ok()?;
    let since_epoch = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(since_epoch.as_millis()).ok()
}

/// The user's chosen runtime for a major version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChosenJava {
    pub major_version: u32,
    pub full_version: String,
    pub architecture: String,
    pub path: String,
}

/// Both halves of the document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Javas {
    chosen: Vec<ChosenJava>,
    discovered: BTreeMap<String, DiscoveredJava>,
}

impl Javas {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the user's choice for a major version, replacing any previous
    /// one for that version.
    pub fn choose(&mut self, chosen: ChosenJava) {
        self.chosen
            .retain(|existing| existing.major_version != chosen.major_version);
        self.chosen.push(chosen);
        self.chosen.sort_by_key(|entry| entry.major_version);
    }

    /// The path the user chose for `major`, if any.
    pub fn chosen_for(&self, major: u32) -> Option<&ChosenJava> {
        self.chosen
            .iter()
            .find(|entry| entry.major_version == major)
    }

    pub fn chosen(&self) -> &[ChosenJava] {
        &self.chosen
    }

    /// Records what a scan found about the runtime at `path`.
    pub fn discover(&mut self, path: &str, java: DiscoveredJava) {
        self.discovered.insert(path.to_owned(), java);
    }

    pub fn discovered(&self) -> &BTreeMap<String, DiscoveredJava> {
        &self.discovered
    }

    /// The runtimes for `major` whose cached details still match the file.
    ///
    /// A record whose file changed or vanished is skipped, not deleted: a
    /// temporarily unavailable path must not erase what a scan learned.
    pub fn usable(&self, major: u32) -> impl Iterator<Item = (&str, &DiscoveredJava)> {
        self.discovered
            .iter()
            .filter(move |(_, java)| java.major_version == major)
            .filter(|(path, java)| java.matches_file(Path::new(path)))
            .map(|(path, java)| (path.as_str(), java))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JavasDocument {
    schema_version: u32,
    #[serde(default)]
    chosen: Vec<ChosenJava>,
    #[serde(default)]
    discovered: BTreeMap<String, DiscoveredJava>,
}

#[derive(Debug)]
pub enum JavasError {
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

impl fmt::Display for JavasError {
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
                "{} uses schema {found}; this build reads schema {JAVAS_SCHEMA_VERSION}",
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

impl std::error::Error for JavasError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } | Self::Write { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::RefusedSchema { .. } | Self::RefusedUnreadable { .. } => None,
        }
    }
}

/// What a load found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JavasOrigin {
    Stored,
    Absent,
    ReadOnlyOtherSchema { found: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedJavas {
    pub javas: Javas,
    pub origin: JavasOrigin,
}

/// Reads the document under `root`, treating an absent file as no choices and
/// no cache.
pub fn load(root: &Path) -> Result<LoadedJavas, JavasError> {
    let path = root.join(JAVAS_FILE);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(LoadedJavas {
                javas: Javas::new(),
                origin: JavasOrigin::Absent,
            });
        }
        Err(source) => return Err(JavasError::Read { path, source }),
    };

    let document: JavasDocument =
        serde_json::from_slice(&bytes).map_err(|source| JavasError::Parse {
            path: path.clone(),
            source,
        })?;

    if document.schema_version != JAVAS_SCHEMA_VERSION {
        return Ok(LoadedJavas {
            javas: Javas::new(),
            origin: JavasOrigin::ReadOnlyOtherSchema {
                found: document.schema_version,
            },
        });
    }

    Ok(LoadedJavas {
        javas: Javas {
            chosen: document.chosen,
            discovered: document.discovered,
        },
        origin: JavasOrigin::Stored,
    })
}

/// Writes the document under `root`, refusing to replace one this build does
/// not own.
pub fn save(root: &Path, javas: &Javas) -> Result<(), JavasError> {
    let path = root.join(JAVAS_FILE);

    match document::inspect(&path).map_err(|source| JavasError::Read {
        path: path.clone(),
        source,
    })? {
        Existing::Unreadable => return Err(JavasError::RefusedUnreadable { path }),
        Existing::Schema(found) if found != JAVAS_SCHEMA_VERSION => {
            return Err(JavasError::RefusedSchema { path, found });
        }
        Existing::Missing | Existing::Schema(_) => {}
    }

    let document = JavasDocument {
        schema_version: JAVAS_SCHEMA_VERSION,
        chosen: javas.chosen.clone(),
        discovered: javas.discovered.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&document).map_err(|source| JavasError::Write {
        path: path.clone(),
        source: io::Error::other(source),
    })?;
    write_atomic(&path, &bytes).map_err(|source| JavasError::Write { path, source })
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/state/src/javas.rs"]
mod tests;
