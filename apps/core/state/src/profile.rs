// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The read side of the portable profile document and its local resources.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer};

mod discovery;
mod resources;
pub use discovery::{DiscoveredProfile, DiscoveryError, ProfileDiscovery, discover};
pub use resources::{Resource, ResourceKind, scan_resources};

pub const FILE_NAME: &str = ".axlprofile";
const SCHEMA_VERSION: u32 = 1;

/// A portable snapshot of the instance and its recorded content.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub struct Profile {
    pub schema_version: u32,
    pub id: String,
    pub instance: ProfileInfo,
    pub game: Option<Game>,
    #[serde(default)]
    pub install: Install,
    #[serde(default)]
    pub playtime: Playtime,
    pub source: Option<Source>,
    #[serde(default)]
    pub content: Content,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub struct ProfileInfo {
    pub name: String,
    pub icon: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub struct Game {
    pub minecraft: String,
    pub loader: Option<Loader>,
    pub version_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub struct Loader {
    pub kind: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub struct Install {
    #[serde(default)]
    pub status: InstallStatus,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallStatus {
    #[default]
    NotInstalled,
    Installing,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
#[non_exhaustive]
pub struct Playtime {
    pub total_seconds: u64,
    pub session_count: u64,
    pub last_played_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub struct Source {
    pub provider: String,
    pub project_id: String,
    pub version_id: String,
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub struct Content {
    #[serde(default, deserialize_with = "read_content_files")]
    pub files: BTreeMap<String, ContentFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub struct ContentFile {
    pub size_bytes: u64,
    pub hashes: BTreeMap<String, String>,
    pub source: Option<Source>,
    pub download_cache: Option<DownloadCache>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[non_exhaustive]
pub struct DownloadCache {
    pub url: String,
    pub resolved_at: Option<String>,
    pub expires_at: Option<String>,
}

#[derive(Deserialize)]
struct Schema {
    schema_version: u32,
}

fn read_content_files<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, ContentFile>, D::Error> {
    deserializer.deserialize_map(ContentFiles)
}

struct ContentFiles;

impl<'de> Visitor<'de> for ContentFiles {
    type Value = BTreeMap<String, ContentFile>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a map with unique content paths")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
        let mut files = BTreeMap::new();
        let mut count = 0;
        while let Some((path, file)) = map.next_entry::<String, ContentFile>()? {
            files.insert(path, file);
            count += 1;
        }
        if count != files.len() {
            return Err(de::Error::custom("duplicate content path"));
        }
        Ok(files)
    }
}

/// A missing marker means the directory is not an `.axlprofile` instance.
pub fn read(directory: &Path) -> Result<Option<Profile>, DiscoveryError> {
    let path = directory.join(FILE_NAME);
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(DiscoveryError::Read { path, source }),
    };
    if !metadata.is_file() || is_link(&metadata) {
        return Err(invalid(&path, "the profile marker must be a regular file"));
    }
    let bytes = fs::read(&path).map_err(|source| DiscoveryError::Read {
        path: path.clone(),
        source,
    })?;
    let parse_error = |source| DiscoveryError::Parse {
        path: path.clone(),
        source,
    };
    let schema: Schema = serde_json::from_slice(&bytes).map_err(parse_error)?;
    if schema.schema_version != SCHEMA_VERSION {
        return Err(DiscoveryError::UnsupportedSchema {
            path,
            found: schema.schema_version,
        });
    }
    let mut profile: Profile = serde_json::from_slice(&bytes).map_err(parse_error)?;
    validate(&profile).map_err(|reason| invalid(&path, reason))?;
    profile.id.make_ascii_lowercase();
    Ok(Some(profile))
}

fn validate(profile: &Profile) -> Result<(), String> {
    let parts = profile.id.split('-');
    if !parts.clone().map(str::len).eq([8, 4, 4, 4, 12])
        || !parts
            .flat_map(str::bytes)
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err("id must be a UUID".into());
    }
    if profile.instance.name.trim().is_empty()
        || profile.instance.name.chars().any(char::is_control)
    {
        return Err("instance name must be visible text".into());
    }
    if profile
        .instance
        .icon
        .as_deref()
        .is_some_and(|path| !relative_path(path))
    {
        return Err("icon path must be relative to the instance".into());
    }
    if let Some(game) = &profile.game {
        validate_game(game)?;
    }
    validate_files(&profile.content.files)
}

fn validate_game(game: &Game) -> Result<(), String> {
    let invalid = std::iter::once(game.minecraft.as_str())
        .chain(game.version_id.as_deref())
        .find(|id| !relative_path(id) || id.contains('/'));
    invalid.map_or(Ok(()), |id| Err(format!("invalid game version id {id:?}")))
}

fn validate_files(files: &BTreeMap<String, ContentFile>) -> Result<(), String> {
    let mut paths = std::collections::HashSet::new();
    for (path, file) in files {
        if !relative_path(path)
            || path.ends_with(".disabled")
            || !paths.insert(path.to_ascii_lowercase())
        {
            return Err(format!("invalid or conflicting content path {path:?}"));
        }
        validate_hashes(path, &file.hashes)?;
    }
    Ok(())
}

fn validate_hashes(path: &str, hashes: &BTreeMap<String, String>) -> Result<(), String> {
    for (algorithm, digits) in [("sha1", 40), ("sha256", 64), ("sha512", 128)] {
        if hashes.get(algorithm).is_some_and(|hash| {
            hash.len() != digits || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
        }) {
            return Err(format!("invalid {algorithm} for {path:?}"));
        }
    }
    Ok(())
}

fn relative_path(path: &str) -> bool {
    !path.contains(['\\', ':'])
        && !path.chars().any(char::is_control)
        && path
            .split('/')
            .all(|part| !matches!(part, "" | "." | "..") && !part.ends_with(['.', ' ']))
}

fn invalid(path: &Path, reason: impl Into<String>) -> DiscoveryError {
    DiscoveryError::InvalidManifest {
        path: path.to_path_buf(),
        reason: reason.into(),
    }
}

pub(crate) fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/state/src/profile.rs"]
mod tests;
