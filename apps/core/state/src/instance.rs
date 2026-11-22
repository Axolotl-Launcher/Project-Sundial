// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Instance manifests: one directory per instance, each with `instance.json`.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use axlauncher_storage::write_atomic;
use serde::{Deserialize, Serialize};

use crate::document::{self, Existing};

mod profiles;
pub use profiles::{create_numbered, discover_profiles};

/// The only manifest version this build reads.
pub const INSTANCE_SCHEMA_VERSION: u32 = 1;

const MANIFEST: &str = "instance.json";

/// How far an instance's install has got.
///
/// A half-installed instance must be distinguishable from a ready one, or the
/// launcher offers to start a game whose files are still arriving.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallStage {
    /// Never installed.
    #[default]
    NotInstalled,
    /// The game's own files are arriving.
    MinecraftInstalling,
    /// The game is ready and no content pack has been applied.
    Installed,
    /// A content pack is being applied on top of a ready game.
    PackInstalling,
    /// The game and its content pack are ready.
    PackInstalled,
}

impl InstallStage {
    /// Whether the instance's files are complete enough to launch.
    pub fn is_ready(self) -> bool {
        matches!(self, Self::Installed | Self::PackInstalled)
    }
}

/// Where an instance came from, when it was not created here.
///
/// Only the identity of the external project is kept: the server, hosting and
/// shared-instance columns the reference also has belong to features this
/// project does not have, and a field nothing reads is a field that rots.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceLink {
    /// The kind of project, in the reference's vocabulary (`modrinth_project`).
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_id: Option<String>,
}

/// The on-disk manifest, one per instance directory.
///
/// Every field the launcher can grow without is optional with a default, so a
/// document written by an earlier build still parses. The field names follow
/// the reference's columns, so an external converter is a direct map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstanceManifest {
    pub schema_version: u32,
    pub name: String,
    #[serde(default)]
    pub game_version: Option<String>,
    #[serde(default)]
    pub loader: Option<String>,
    /// An icon's path, relative to the instance directory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon_path: Option<String>,
    /// Unix milliseconds.
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub modified: i64,
    /// Unix milliseconds; absent until the instance is first played.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_played: Option<i64>,
    /// Total milliseconds played, as the reference counts it.
    #[serde(default)]
    pub submitted_time_played: i64,
    /// Milliseconds played since the last submission to the total.
    #[serde(default)]
    pub recent_time_played: i64,
    #[serde(default)]
    pub install_stage: InstallStage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<InstanceLink>,
}

/// One instance as the launcher shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    /// The directory name, which is the instance's stable identity on disk.
    pub id: String,
    pub name: String,
    pub game_version: Option<String>,
    pub loader: Option<String>,
    pub icon_path: Option<String>,
    pub created: i64,
    pub modified: i64,
    pub last_played: Option<i64>,
    pub submitted_time_played: i64,
    pub recent_time_played: i64,
    pub install_stage: InstallStage,
    pub link: Option<InstanceLink>,
}

impl Instance {
    /// An instance with nothing but the fields `create` needs filled in.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            game_version: None,
            loader: None,
            icon_path: None,
            created: 0,
            modified: 0,
            last_played: None,
            submitted_time_played: 0,
            recent_time_played: 0,
            install_stage: InstallStage::default(),
            link: None,
        }
    }
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
    discover_in(root, |_| true)
}

fn discover_in(
    root: &Path,
    include: impl Fn(&str) -> bool,
) -> Result<Vec<Instance>, DiscoveryError> {
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

        let id = entry.file_name().to_string_lossy().into_owned();
        let manifest = directory.join(MANIFEST);
        if !include(&id) || !manifest.is_file() {
            continue;
        }

        instances.push(read_manifest(&manifest, id)?);
    }

    sort_instances(&mut instances);
    Ok(instances)
}

fn sort_instances(instances: &mut [Instance]) {
    instances.sort_by(|left, right| {
        left.name
            .cmp(&right.name)
            .then_with(|| left.id.cmp(&right.id))
    });
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
        icon_path: manifest.icon_path,
        created: manifest.created,
        modified: manifest.modified,
        last_played: manifest.last_played,
        submitted_time_played: manifest.submitted_time_played,
        recent_time_played: manifest.recent_time_played,
        install_stage: manifest.install_stage,
        link: manifest.link,
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
    NoAvailableNumber,
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
            Self::NoAvailableNumber => write!(formatter, "Cannot find a free instance name."),
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
        icon_path: instance.icon_path.clone(),
        created: instance.created,
        modified: instance.modified,
        last_played: instance.last_played,
        submitted_time_played: instance.submitted_time_played,
        recent_time_played: instance.recent_time_played,
        install_stage: instance.install_stage,
        link: instance.link.clone(),
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
        icon_path: instance.icon_path.clone(),
        created: instance.created,
        modified: instance.modified,
        last_played: instance.last_played,
        submitted_time_played: instance.submitted_time_played,
        recent_time_played: instance.recent_time_played,
        install_stage: instance.install_stage,
        link: instance.link.clone(),
    })
}

fn trimmed(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/state/src/instance/tests.rs"]
mod tests;
