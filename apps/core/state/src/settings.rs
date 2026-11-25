// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The settings document.
//!
//! The file is an envelope: `{"schema_version": 1, "data": {...}}`. A version
//! this build does not write is read as defaults and left alone, never
//! converted.
//!
//! Raising the schema: a change that only adds an optional field with a
//! `serde(default)` does not need a bump, because an older build ignores the
//! unknown field and a newer build fills the default. Renaming, removing or
//! changing the meaning of a field does, because the two builds would disagree
//! about what the file means.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use axlauncher_storage::write_atomic;
use serde::{Deserialize, Serialize};

use crate::document::{self, Existing};

/// The only settings schema this build writes.
pub const SETTINGS_SCHEMA_VERSION: u32 = 1;

pub const SETTINGS_FILE: &str = "settings.json";

/// Which palette the user prefers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeChoice {
    #[default]
    Dark,
    Light,
}

/// Where a download source prefers to fetch from.
///
/// `Auto` uses official sources when reachable, `OfficialOnly` disables
/// mirrors, and `MirrorPreferred` tries BMCLAPI first where it is available.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourcePreference {
    #[default]
    Auto,
    OfficialOnly,
    MirrorPreferred,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SettingsData {
    #[serde(default)]
    pub theme: ThemeChoice,
    /// The initial Minecraft language, with an empty value following the launcher.
    #[serde(default)]
    pub default_game_language: String,
    /// How many files may be requested at once.
    #[serde(default = "default_download_concurrency")]
    pub max_concurrent_downloads: u32,
    /// How many files may be written at once.
    #[serde(default = "default_concurrency")]
    pub max_concurrent_writes: u32,
    /// The page the launcher opens on.
    #[serde(default)]
    pub default_page: String,
    /// Whether the launcher may send usage data.
    #[serde(default)]
    pub telemetry: bool,
    /// Where the game's metadata comes from.
    #[serde(default)]
    pub minecraft_metadata_source: SourcePreference,
    /// Where the game's files come from.
    #[serde(default)]
    pub minecraft_file_source: SourcePreference,
    /// Fields a newer build wrote; saving keeps them for that build.
    #[serde(flatten, default)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

const fn default_download_concurrency() -> u32 {
    256
}

/// The file-writing width a fresh install starts with.
fn default_concurrency() -> u32 {
    10
}

impl Default for SettingsData {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::default(),
            default_game_language: String::new(),
            max_concurrent_downloads: default_download_concurrency(),
            max_concurrent_writes: default_concurrency(),
            default_page: "home".into(),
            telemetry: false,
            minecraft_metadata_source: SourcePreference::default(),
            minecraft_file_source: SourcePreference::default(),
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct SettingsDocument {
    schema_version: u32,
    data: SettingsData,
}

/// Where the settings in use came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsOrigin {
    /// There was no file; defaults were written so the next run has one.
    Created,
    /// The file was readable and current.
    Stored,
    /// The file could not be parsed; it was moved aside and defaults applied.
    Quarantined { path: PathBuf },
    /// The file belongs to another schema and was left untouched.
    ReadOnlyOtherSchema { found: u32 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoadedSettings {
    pub data: SettingsData,
    pub origin: SettingsOrigin,
}

#[derive(Debug)]
pub enum SettingsError {
    Read { path: PathBuf, source: io::Error },
    Write { path: PathBuf, source: io::Error },
    Quarantine { path: PathBuf, source: io::Error },
    RefusedSchema { path: PathBuf, found: u32 },
    RefusedUnreadable { path: PathBuf },
}

impl fmt::Display for SettingsError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(formatter, "cannot read {}: {source}", path.display())
            }
            Self::Write { path, source } => {
                write!(formatter, "cannot write {}: {source}", path.display())
            }
            Self::Quarantine { path, source } => {
                write!(formatter, "cannot move {} aside: {source}", path.display())
            }
            Self::RefusedSchema { path, found } => write!(
                formatter,
                "{} uses schema {found}; this build writes schema {SETTINGS_SCHEMA_VERSION}",
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

impl std::error::Error for SettingsError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. }
            | Self::Write { source, .. }
            | Self::Quarantine { source, .. } => Some(source),
            Self::RefusedSchema { .. } | Self::RefusedUnreadable { .. } => None,
        }
    }
}

/// Reads the settings under `root`, creating the file with defaults when none
/// exists.
///
/// A document that does not parse is moved to `settings.json.invalid`, so the
/// damaged bytes stay inspectable, and defaults are written in its place. A
/// document from another schema is read as defaults and left as it is: it
/// belongs to another build, and rewriting it would destroy that build's data.
pub fn load_settings(root: &Path) -> Result<LoadedSettings, SettingsError> {
    let _guard = lock_settings(root)?;
    let path = root.join(SETTINGS_FILE);

    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let data = SettingsData::default();
            write_settings(&path, &data)?;
            return Ok(LoadedSettings {
                data,
                origin: SettingsOrigin::Created,
            });
        }
        Err(source) => return Err(SettingsError::Read { path, source }),
    };

    let document = match serde_json::from_slice::<SettingsDocument>(&bytes) {
        Ok(document) => document,
        Err(_) => return quarantine(path),
    };

    if document.schema_version != SETTINGS_SCHEMA_VERSION {
        return Ok(LoadedSettings {
            data: SettingsData::default(),
            origin: SettingsOrigin::ReadOnlyOtherSchema {
                found: document.schema_version,
            },
        });
    }

    Ok(LoadedSettings {
        data: document.data,
        origin: SettingsOrigin::Stored,
    })
}

fn quarantine(path: PathBuf) -> Result<LoadedSettings, SettingsError> {
    let invalid = path.with_file_name(format!("{SETTINGS_FILE}.invalid"));
    fs::rename(&path, &invalid).map_err(|source| SettingsError::Quarantine {
        path: path.clone(),
        source,
    })?;

    let data = SettingsData::default();
    write_settings(&path, &data)?;
    Ok(LoadedSettings {
        data,
        origin: SettingsOrigin::Quarantined { path: invalid },
    })
}

/// Writes `data`, refusing to replace a document this build does not own.
pub fn save_settings(root: &Path, data: &SettingsData) -> Result<(), SettingsError> {
    update_settings(root, |current| current.clone_from(data)).map(|_| ())
}

/// A settings update sees the latest document while holding the writer lock.
pub fn update_settings(
    root: &Path,
    update: impl FnOnce(&mut SettingsData),
) -> Result<SettingsData, SettingsError> {
    let _guard = lock_settings(root)?;
    let path = root.join(SETTINGS_FILE);
    let existing = document::inspect(&path).map_err(|source| SettingsError::Read {
        path: path.clone(),
        source,
    })?;
    let mut data = match existing {
        Existing::Missing => SettingsData::default(),
        Existing::Schema(SETTINGS_SCHEMA_VERSION) => {
            let bytes = fs::read(&path).map_err(|source| SettingsError::Read {
                path: path.clone(),
                source,
            })?;
            serde_json::from_slice::<SettingsDocument>(&bytes)
                .map_err(|_| SettingsError::RefusedUnreadable { path: path.clone() })?
                .data
        }
        Existing::Schema(found) => {
            return Err(SettingsError::RefusedSchema { path, found });
        }
        Existing::Unreadable => return Err(SettingsError::RefusedUnreadable { path }),
    };
    update(&mut data);
    write_settings(&path, &data)?;
    Ok(data)
}

fn write_settings(path: &Path, data: &SettingsData) -> Result<(), SettingsError> {
    let document = SettingsDocument {
        schema_version: SETTINGS_SCHEMA_VERSION,
        data: data.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&document).expect("settings document serializes");
    write_atomic(path, &bytes).map_err(|source| SettingsError::Write {
        path: path.to_path_buf(),
        source,
    })
}

fn lock_settings(root: &Path) -> Result<fs::File, SettingsError> {
    let path = root.join("settings.lock");
    let failure = |source| SettingsError::Write {
        path: path.clone(),
        source,
    };
    fs::create_dir_all(root).map_err(failure)?;
    let lock = fs::File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(failure)?;
    lock.lock().map_err(failure)?;
    Ok(lock)
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/state/src/settings.rs"]
mod tests;
