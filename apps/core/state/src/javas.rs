// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Machine-local Java registrations and preferences.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use axlauncher_storage::{Document, write_atomic};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const SCHEMA_VERSION: u32 = 1;

/// The user's preferred installation for a Java major.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PreferredJava {
    major_version: u32,
    path: PathBuf,
}

/// The local catalog entries and selections that persist across scans.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Javas {
    // Older catalogs used `chosen` for entries recorded automatically at install time.
    preferred: Vec<PreferredJava>,
    #[serde(alias = "manual", deserialize_with = "read_installations")]
    installations: BTreeMap<PathBuf, Option<u32>>,
    profiles: BTreeMap<String, PathBuf>,
}

impl Javas {
    pub fn preferred(&self) -> impl Iterator<Item = (u32, &Path)> {
        self.preferred
            .iter()
            .map(|entry| (entry.major_version, entry.path.as_path()))
    }

    /// A major has at most one preferred installation.
    pub fn set_preferred(&mut self, major: u32, path: Option<PathBuf>) {
        self.preferred.retain(|entry| entry.major_version != major);
        if let Some(path) = path {
            self.preferred.push(PreferredJava {
                major_version: major,
                path,
            });
            self.preferred.sort_by_key(|entry| entry.major_version);
        }
    }

    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        self.installations.keys().map(PathBuf::as_path)
    }

    pub fn installations(&self) -> impl Iterator<Item = (&Path, Option<u32>)> {
        self.installations
            .iter()
            .map(|(path, major)| (path.as_path(), *major))
    }

    pub fn contains(&self, path: &Path) -> bool {
        self.installations.contains_key(path)
    }

    pub fn is_empty(&self) -> bool {
        self.installations.is_empty()
    }

    pub fn add(&mut self, path: PathBuf, major: u32) {
        self.installations.insert(path, Some(major));
    }

    /// Removes a registration and clears preferences that referred to it.
    pub fn remove(&mut self, path: &Path) -> bool {
        if self.installations.remove(path).is_none() {
            return false;
        }
        self.preferred.retain(|entry| entry.path != path);
        self.profiles.retain(|_, selected| selected != path);
        true
    }

    pub fn profile_preference(&self, id: &str) -> Option<&Path> {
        self.profiles.get(id).map(PathBuf::as_path)
    }

    /// Saves a machine-local override for a portable profile ID.
    pub fn set_profile_preference(&mut self, id: &str, path: Option<PathBuf>) {
        if let Some(path) = path {
            self.profiles.insert(id.to_owned(), path);
        } else {
            self.profiles.remove(id);
        }
    }
}

fn read_installations<'de, D>(deserializer: D) -> Result<BTreeMap<PathBuf, Option<u32>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Entries {
        Installations(BTreeMap<PathBuf, Option<u32>>),
        Paths(BTreeSet<PathBuf>),
    }

    Ok(match Entries::deserialize(deserializer)? {
        Entries::Installations(installations) => installations,
        Entries::Paths(paths) => paths.into_iter().map(|path| (path, None)).collect(),
    })
}

#[derive(Debug, Serialize, Deserialize)]
struct JavasDocument {
    schema_version: u32,
    #[serde(flatten)]
    javas: Javas,
}

#[derive(Debug, Error)]
pub enum JavasError {
    #[error("cannot read {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("cannot parse {path}: {source}")]
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("cannot write {path}: {source}")]
    Write { path: PathBuf, source: io::Error },
    #[error("{path} uses unsupported Java catalog schema {found}")]
    RefusedSchema { path: PathBuf, found: u32 },
}

/// Reads the machine-local preferences for the current data directory.
pub fn load(root: &Path) -> Result<Javas, JavasError> {
    let path = Document::JavaCatalog.path(root);
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Javas::default()),
        Err(source) => return Err(JavasError::Read { path, source }),
    };
    let document: JavasDocument =
        serde_json::from_slice(&bytes).map_err(|source| JavasError::Parse {
            path: path.clone(),
            source,
        })?;
    if document.schema_version != SCHEMA_VERSION {
        return Err(JavasError::RefusedSchema {
            path,
            found: document.schema_version,
        });
    }
    Ok(document.javas)
}

/// Concurrent writers edit the latest catalog under a lock before atomic publication.
pub fn update<T>(root: &Path, edit: impl FnOnce(&mut Javas) -> T) -> Result<T, JavasError> {
    let path = Document::JavaCatalog.path(root);
    let failure = |source| JavasError::Write {
        path: path.clone(),
        source,
    };
    let _lock = Document::JavaCatalog
        .lock(root)
        .map_err(|error| failure(error.into()))?;
    let mut javas = load(root)?;
    let result = edit(&mut javas);
    let document = JavasDocument {
        schema_version: SCHEMA_VERSION,
        javas,
    };
    let bytes =
        serde_json::to_vec_pretty(&document).map_err(|error| failure(io::Error::other(error)))?;
    write_atomic(&path, &bytes).map_err(failure)?;
    Ok(result)
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/state/src/javas.rs"]
mod tests;
