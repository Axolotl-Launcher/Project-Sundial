// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! Instance manifests: one directory per instance, each with `instance.json`.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The only manifest version this build reads.
pub const INSTANCE_SCHEMA_VERSION: u32 = 1;

const MANIFEST: &str = "instance.json";

/// The on-disk manifest, one per instance directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceManifest {
    pub schema_version: u32,
    pub name: String,
    #[serde(default)]
    pub game_version: Option<String>,
    #[serde(default)]
    pub loader: Option<String>,
}

/// One instance as the launcher shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    /// The directory name, which is the instance's stable identity on disk.
    pub id: String,
    pub name: String,
    pub game_version: Option<String>,
    pub loader: Option<String>,
}

#[derive(Debug)]
pub enum DiscoveryError {
    Read {
        path: PathBuf,
        source: io::Error,
    },
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    UnsupportedSchema {
        path: PathBuf,
        found: u32,
    },
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(formatter, "cannot read {}: {source}", path.display())
            }
            Self::Parse { path, source } => {
                write!(formatter, "cannot parse {}: {source}", path.display())
            }
            Self::UnsupportedSchema { path, found } => write!(
                formatter,
                "{} uses schema {found}; this build reads schema {INSTANCE_SCHEMA_VERSION}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for DiscoveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::UnsupportedSchema { .. } => None,
        }
    }
}

/// Lists the instances under `root`, sorted by display name.
///
/// A missing root is an empty list: a first run has nothing to show, which is
/// not an error. Anything else that is wrong — an unreadable directory, a
/// malformed manifest, an unknown schema — is returned, never skipped, so a
/// broken instance is visible instead of silently absent.
pub fn discover(root: &Path) -> Result<Vec<Instance>, DiscoveryError> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(source) => {
            return Err(DiscoveryError::Read {
                path: root.to_path_buf(),
                source,
            });
        }
    };

    let mut instances = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| DiscoveryError::Read {
            path: root.to_path_buf(),
            source,
        })?;

        let directory = entry.path();
        if !directory.is_dir() {
            continue;
        }

        let manifest = directory.join(MANIFEST);
        if !manifest.is_file() {
            continue;
        }

        let id = entry.file_name().to_string_lossy().into_owned();
        instances.push(read_manifest(&manifest, id)?);
    }

    instances.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(instances)
}

fn read_manifest(path: &Path, id: String) -> Result<Instance, DiscoveryError> {
    let bytes = fs::read(path).map_err(|source| DiscoveryError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let manifest: InstanceManifest =
        serde_json::from_slice(&bytes).map_err(|source| DiscoveryError::Parse {
            path: path.to_path_buf(),
            source,
        })?;

    if manifest.schema_version != INSTANCE_SCHEMA_VERSION {
        return Err(DiscoveryError::UnsupportedSchema {
            path: path.to_path_buf(),
            found: manifest.schema_version,
        });
    }

    Ok(Instance {
        id,
        name: manifest.name,
        game_version: manifest.game_version,
        loader: manifest.loader,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::TempRoot;

    fn write_manifest(root: &Path, id: &str, manifest: &str) {
        let directory = root.join(id);
        fs::create_dir_all(&directory).expect("instance directory");
        fs::write(directory.join(MANIFEST), manifest).expect("manifest");
    }

    #[test]
    fn a_missing_root_is_an_empty_list() {
        let temp = TempRoot::new();
        let missing = temp.path().join("does-not-exist");
        assert_eq!(discover(&missing).expect("discovery"), Vec::new());
    }

    #[test]
    fn reads_a_manifest_and_uses_the_directory_as_id() {
        let temp = TempRoot::new();
        write_manifest(
            temp.path(),
            "survival",
            r#"{"schema_version":1,"name":"Survival","game_version":"1.21","loader":"fabric"}"#,
        );

        assert_eq!(
            discover(temp.path()).expect("discovery"),
            vec![Instance {
                id: "survival".into(),
                name: "Survival".into(),
                game_version: Some("1.21".into()),
                loader: Some("fabric".into()),
            }]
        );
    }

    #[test]
    fn a_directory_without_a_manifest_is_not_an_instance() {
        let temp = TempRoot::new();
        fs::create_dir_all(temp.path().join("scratch")).expect("directory");
        assert_eq!(discover(temp.path()).expect("discovery"), Vec::new());
    }

    #[test]
    fn sorts_by_name_then_id() {
        let temp = TempRoot::new();
        write_manifest(temp.path(), "b", r#"{"schema_version":1,"name":"Alpha"}"#);
        write_manifest(temp.path(), "c", r#"{"schema_version":1,"name":"Alpha"}"#);
        write_manifest(temp.path(), "a", r#"{"schema_version":1,"name":"Zulu"}"#);

        let ids: Vec<String> = discover(temp.path())
            .expect("discovery")
            .into_iter()
            .map(|instance| instance.id)
            .collect();
        assert_eq!(ids, ["b", "c", "a"]);
    }

    #[test]
    fn a_malformed_manifest_is_reported_with_its_path() {
        let temp = TempRoot::new();
        write_manifest(temp.path(), "broken", "{ not json }");

        let error = discover(temp.path()).expect_err("must fail");
        assert!(matches!(error, DiscoveryError::Parse { .. }));
        assert!(error.to_string().contains("broken"));
    }

    #[test]
    fn a_newer_schema_is_rejected_rather_than_misread() {
        let temp = TempRoot::new();
        write_manifest(
            temp.path(),
            "future",
            r#"{"schema_version":2,"name":"Future"}"#,
        );

        let error = discover(temp.path()).expect_err("must fail");
        assert!(matches!(
            error,
            DiscoveryError::UnsupportedSchema { found: 2, .. }
        ));
    }

    #[test]
    fn a_manifest_without_a_name_is_rejected() {
        let temp = TempRoot::new();
        write_manifest(temp.path(), "nameless", r#"{"schema_version":1}"#);
        assert!(matches!(
            discover(temp.path()).expect_err("must fail"),
            DiscoveryError::Parse { .. }
        ));
    }
}
