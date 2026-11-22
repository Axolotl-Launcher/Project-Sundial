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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsData {
    #[serde(default)]
    pub theme: ThemeChoice,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
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
    let path = root.join(SETTINGS_FILE);

    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let data = SettingsData::default();
            save_settings(root, &data)?;
            return Ok(LoadedSettings {
                data,
                origin: SettingsOrigin::Created,
            });
        }
        Err(source) => return Err(SettingsError::Read { path, source }),
    };

    let document = match serde_json::from_slice::<SettingsDocument>(&bytes) {
        Ok(document) => document,
        Err(_) => return quarantine(root, path),
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

fn quarantine(root: &Path, path: PathBuf) -> Result<LoadedSettings, SettingsError> {
    let invalid = path.with_file_name(format!("{SETTINGS_FILE}.invalid"));
    fs::rename(&path, &invalid).map_err(|source| SettingsError::Quarantine {
        path: path.clone(),
        source,
    })?;

    let data = SettingsData::default();
    save_settings(root, &data)?;
    Ok(LoadedSettings {
        data,
        origin: SettingsOrigin::Quarantined { path: invalid },
    })
}

/// Writes `data`, refusing to replace a document this build does not own.
pub fn save_settings(root: &Path, data: &SettingsData) -> Result<(), SettingsError> {
    let path = root.join(SETTINGS_FILE);

    let existing = document::inspect(&path).map_err(|source| SettingsError::Read {
        path: path.clone(),
        source,
    })?;
    match existing {
        Existing::Missing | Existing::Schema(SETTINGS_SCHEMA_VERSION) => {}
        Existing::Schema(found) => {
            return Err(SettingsError::RefusedSchema { path, found });
        }
        Existing::Unreadable => return Err(SettingsError::RefusedUnreadable { path }),
    }

    let document = SettingsDocument {
        schema_version: SETTINGS_SCHEMA_VERSION,
        data: data.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&document).expect("settings document serializes");
    write_atomic(&path, &bytes).map_err(|source| SettingsError::Write { path, source })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempRoot;

    fn seed(root: &Path, contents: &str) -> PathBuf {
        let path = root.join(SETTINGS_FILE);
        fs::write(&path, contents).expect("seed");
        path
    }

    #[test]
    fn a_missing_file_is_created_with_defaults() {
        let temp = TempRoot::new();

        let loaded = load_settings(temp.path()).expect("load");

        assert_eq!(loaded.origin, SettingsOrigin::Created);
        assert_eq!(loaded.data, SettingsData::default());

        let written = fs::read_to_string(temp.path().join(SETTINGS_FILE)).expect("read back");
        assert!(written.contains(r#""schema_version": 1"#));
        assert!(written.contains(r#""theme": "dark""#));
    }

    #[test]
    fn a_saved_theme_roundtrips() {
        let temp = TempRoot::new();
        let data = SettingsData {
            theme: ThemeChoice::Light,
        };

        save_settings(temp.path(), &data).expect("save");
        let loaded = load_settings(temp.path()).expect("load");

        assert_eq!(loaded.data, data);
        assert_eq!(loaded.origin, SettingsOrigin::Stored);
    }

    #[test]
    fn an_unreadable_document_is_quarantined_and_defaults_apply() {
        let temp = TempRoot::new();
        seed(temp.path(), "{ not a document }");

        let loaded = load_settings(temp.path()).expect("load");

        assert_eq!(loaded.data, SettingsData::default());
        let SettingsOrigin::Quarantined { path } = &loaded.origin else {
            panic!("expected quarantine, got {:?}", loaded.origin);
        };
        assert_eq!(
            fs::read_to_string(path).expect("kept bytes"),
            "{ not a document }"
        );
        assert!(temp.path().join(SETTINGS_FILE).is_file());
    }

    #[test]
    fn a_document_from_another_schema_is_read_only() {
        let temp = TempRoot::new();
        let contents = r#"{"schema_version":2,"data":{"theme":"light"}}"#;
        let path = seed(temp.path(), contents);
        let before = fs::read(&path).expect("before");

        let loaded = load_settings(temp.path()).expect("load");

        assert_eq!(
            loaded.origin,
            SettingsOrigin::ReadOnlyOtherSchema { found: 2 }
        );
        assert_eq!(loaded.data, SettingsData::default());
        assert_eq!(fs::read(&path).expect("after"), before);
    }

    #[test]
    fn saving_over_another_schema_is_refused() {
        let temp = TempRoot::new();
        let contents = r#"{"schema_version":2,"data":{"theme":"light"}}"#;
        let path = seed(temp.path(), contents);
        let before = fs::read(&path).expect("before");

        let error = save_settings(temp.path(), &SettingsData::default()).expect_err("must refuse");

        assert!(matches!(
            error,
            SettingsError::RefusedSchema { found: 2, .. }
        ));
        assert_eq!(fs::read(&path).expect("after"), before);
    }

    #[test]
    fn saving_over_an_unreadable_document_is_refused() {
        let temp = TempRoot::new();
        let path = seed(temp.path(), "{ broken");
        let before = fs::read(&path).expect("before");

        let error = save_settings(temp.path(), &SettingsData::default()).expect_err("must refuse");

        assert!(matches!(error, SettingsError::RefusedUnreadable { .. }));
        assert_eq!(fs::read(&path).expect("after"), before);
    }
}
