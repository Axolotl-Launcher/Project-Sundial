// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! Instance manifests: one directory per instance, each with `instance.json`.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use superlogical_storage::write_atomic;

use crate::document::{self, Existing};

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

/// Longest instance id accepted; also keeps the directory name short on every
/// filesystem this project targets.
const MAX_ID_LEN: usize = 64;

/// Longest display name accepted.
const MAX_NAME_LEN: usize = 120;

/// Why a manifest was not written.
#[derive(Debug)]
pub enum InstanceError {
    InvalidId { id: String },
    InvalidName { name: String },
    AlreadyExists { path: PathBuf },
    NotFound { path: PathBuf },
    Read { path: PathBuf, source: io::Error },
    Write { path: PathBuf, source: io::Error },
    RefusedSchema { path: PathBuf, found: u32 },
    RefusedUnreadable { path: PathBuf },
}

impl fmt::Display for InstanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId { id } => write!(
                formatter,
                "\"{id}\" is not a valid instance id: use 1-{MAX_ID_LEN} characters from a-z, 0-9, '-' and '_'"
            ),
            Self::InvalidName { name } => write!(
                formatter,
                "\"{name}\" is not a valid name: use 1-{MAX_NAME_LEN} visible characters"
            ),
            Self::AlreadyExists { path } => {
                write!(formatter, "{} already exists", path.display())
            }
            Self::NotFound { path } => write!(formatter, "{} does not exist", path.display()),
            Self::Read { path, source } => {
                write!(formatter, "cannot read {}: {source}", path.display())
            }
            Self::Write { path, source } => {
                write!(formatter, "cannot write {}: {source}", path.display())
            }
            Self::RefusedSchema { path, found } => write!(
                formatter,
                "{} uses schema {found}; this build writes schema {INSTANCE_SCHEMA_VERSION}",
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

impl std::error::Error for InstanceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } | Self::Write { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Writes a new instance directory and its manifest.
///
/// The directory must not exist: writing into a folder that already holds
/// something would mix the launcher's files with a user's.
pub fn create(root: &Path, instance: &Instance) -> Result<(), InstanceError> {
    let instance = normalise(instance)?;
    let directory = root.join(&instance.id);
    let manifest = directory.join(MANIFEST);

    fs::create_dir_all(root).map_err(|source| InstanceError::Write {
        path: root.to_path_buf(),
        source,
    })?;

    if let Err(source) = fs::create_dir(&directory) {
        return Err(if source.kind() == io::ErrorKind::AlreadyExists {
            InstanceError::AlreadyExists { path: directory }
        } else {
            InstanceError::Write {
                path: directory,
                source,
            }
        });
    }

    if let Err(error) = write_manifest(&manifest, &instance) {
        let _ = fs::remove_dir_all(&directory);
        return Err(error);
    }
    Ok(())
}

/// Rewrites an instance's manifest.
///
/// The instance must already exist, and its manifest must be one this build
/// owns: replacing another schema's document would destroy that build's data.
pub fn save(root: &Path, instance: &Instance) -> Result<(), InstanceError> {
    let instance = normalise(instance)?;
    let manifest = root.join(&instance.id).join(MANIFEST);

    let existing = document::inspect(&manifest).map_err(|source| InstanceError::Read {
        path: manifest.clone(),
        source,
    })?;

    match existing {
        Existing::Missing => Err(InstanceError::NotFound { path: manifest }),
        Existing::Unreadable => Err(InstanceError::RefusedUnreadable { path: manifest }),
        Existing::Schema(found) if found != INSTANCE_SCHEMA_VERSION => {
            Err(InstanceError::RefusedSchema {
                path: manifest,
                found,
            })
        }
        Existing::Schema(_) => write_manifest(&manifest, &instance),
    }
}

fn write_manifest(path: &Path, instance: &Instance) -> Result<(), InstanceError> {
    let manifest = InstanceManifest {
        schema_version: INSTANCE_SCHEMA_VERSION,
        name: instance.name.clone(),
        game_version: instance.game_version.clone(),
        loader: instance.loader.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&manifest).expect("instance manifest serializes");
    write_atomic(path, &bytes).map_err(|source| InstanceError::Write {
        path: path.to_path_buf(),
        source,
    })
}

/// Trims input and rejects anything that cannot be a directory name.
///
/// The id becomes a directory on disk, so it is limited to characters that
/// cannot leave the root and cannot collide on a case-insensitive filesystem.
fn normalise(instance: &Instance) -> Result<Instance, InstanceError> {
    let id = instance.id.trim();
    let allowed = id.chars().all(|character| {
        character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || matches!(character, '-' | '_')
    });
    if id.is_empty() || id.len() > MAX_ID_LEN || !allowed {
        return Err(InstanceError::InvalidId {
            id: instance.id.clone(),
        });
    }

    let name = instance.name.trim();
    if name.is_empty() || name.len() > MAX_NAME_LEN {
        return Err(InstanceError::InvalidName {
            name: instance.name.clone(),
        });
    }

    Ok(Instance {
        id: id.to_string(),
        name: name.to_string(),
        game_version: trimmed(instance.game_version.as_deref()),
        loader: trimmed(instance.loader.as_deref()),
    })
}

fn trimmed(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
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

    fn sample(id: &str, name: &str) -> Instance {
        Instance {
            id: id.into(),
            name: name.into(),
            game_version: Some("1.21".into()),
            loader: Some("fabric".into()),
        }
    }

    #[test]
    fn create_writes_a_manifest_that_discovery_finds() {
        let temp = TempRoot::new();

        create(temp.path(), &sample("survival", "Survival")).expect("create");

        assert_eq!(
            discover(temp.path()).expect("discovery"),
            vec![sample("survival", "Survival")]
        );
    }

    #[test]
    fn create_trims_the_name_and_blank_optional_fields() {
        let temp = TempRoot::new();
        let mut instance = sample("padded", "  Padded  ");
        instance.loader = Some("   ".into());

        create(temp.path(), &instance).expect("create");

        let found = discover(temp.path()).expect("discovery");
        assert_eq!(found[0].name, "Padded");
        assert_eq!(found[0].loader, None);
    }

    #[test]
    fn create_refuses_an_existing_directory() {
        let temp = TempRoot::new();
        fs::create_dir(temp.path().join("taken")).expect("directory");

        let error = create(temp.path(), &sample("taken", "Taken")).expect_err("must refuse");

        assert!(matches!(error, InstanceError::AlreadyExists { .. }));
    }

    #[test]
    fn ids_that_could_leave_the_root_are_rejected() {
        let temp = TempRoot::new();

        for id in ["", "..", "a/b", "a\\b", "UPPER", "with space"] {
            let error = create(temp.path(), &sample(id, "Bad")).expect_err(id);
            assert!(matches!(error, InstanceError::InvalidId { .. }), "{id}");
        }
    }

    #[test]
    fn an_empty_name_is_rejected() {
        let temp = TempRoot::new();

        let error = create(temp.path(), &sample("ok", "   ")).expect_err("must refuse");

        assert!(matches!(error, InstanceError::InvalidName { .. }));
    }

    #[test]
    fn save_rewrites_the_manifest() {
        let temp = TempRoot::new();
        create(temp.path(), &sample("survival", "Survival")).expect("create");
        let mut renamed = sample("survival", "Renamed");
        renamed.loader = None;

        save(temp.path(), &renamed).expect("save");

        assert_eq!(discover(temp.path()).expect("discovery"), vec![renamed]);
    }

    #[test]
    fn save_refuses_a_missing_instance() {
        let temp = TempRoot::new();

        let error = save(temp.path(), &sample("absent", "Absent")).expect_err("must fail");

        assert!(matches!(error, InstanceError::NotFound { .. }));
    }

    #[test]
    fn save_refuses_another_schema_and_leaves_it_alone() {
        let temp = TempRoot::new();
        let directory = temp.path().join("future");
        fs::create_dir_all(&directory).expect("directory");
        let manifest = directory.join(MANIFEST);
        fs::write(&manifest, r#"{"schema_version":2,"name":"Future"}"#).expect("seed");
        let before = fs::read(&manifest).expect("before");

        let error = save(temp.path(), &sample("future", "Renamed")).expect_err("must refuse");

        assert!(matches!(
            error,
            InstanceError::RefusedSchema { found: 2, .. }
        ));
        assert_eq!(fs::read(&manifest).expect("after"), before);
    }

    #[test]
    fn save_refuses_an_unreadable_manifest_and_leaves_it_alone() {
        let temp = TempRoot::new();
        let directory = temp.path().join("broken");
        fs::create_dir_all(&directory).expect("directory");
        let manifest = directory.join(MANIFEST);
        fs::write(&manifest, "{ broken").expect("seed");
        let before = fs::read(&manifest).expect("before");

        let error = save(temp.path(), &sample("broken", "Broken")).expect_err("must refuse");

        assert!(matches!(error, InstanceError::RefusedUnreadable { .. }));
        assert_eq!(fs::read(&manifest).expect("after"), before);
    }
}
