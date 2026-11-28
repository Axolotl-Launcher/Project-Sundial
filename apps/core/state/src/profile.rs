// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Portable profile documents and their local resources.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use axlauncher_domain::{is_visible_instance_name, memory::MaximumHeap};
use axlauncher_storage::{Document, write_atomic};

use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::document::Schema;

mod create;
mod discovery;
mod folders;
mod game;
mod installation;
mod resources;
mod staged;
mod update;
pub use create::create;
pub use discovery::{DiscoveredProfile, DiscoveryError, ProfileDiscovery, discover};
pub use folders::{ProfileFolderName, delete_folder, open_folder};
pub use game::{Game, Loader};
pub use installation::{mark_failed, mark_installed};
pub use resources::{Resource, ResourceKind, count_enabled_mods, scan_resources};
pub use staged::StagedProfile;
pub use update::{
    ProfileUpdateError, rename_if_current, rename_with_folder_if_current, set_memory_if_current,
};

const SCHEMA_VERSION: u32 = 1;

/// A portable snapshot of the instance and its recorded content.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct Profile {
    pub schema_version: u32,
    id: String,
    pub instance: ProfileInfo,
    game: Option<Game>,
    #[serde(
        default,
        deserialize_with = "crate::document::read_preference",
        skip_serializing_if = "Option::is_none"
    )]
    memory: Option<MaximumHeap>,
    #[serde(default)]
    pub install: Install,
    #[serde(default)]
    pub playtime: Playtime,
    pub source: Option<Source>,
    #[serde(default)]
    pub content: Content,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct ProfileInfo {
    pub name: String,
    pub icon: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct Install {
    #[serde(default)]
    pub status: InstallStatus,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallStatus {
    #[default]
    NotInstalled,
    Installing,
    Ready,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
#[non_exhaustive]
pub struct Playtime {
    pub total_seconds: u64,
    pub session_count: u64,
    pub last_played_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct Source {
    pub provider: String,
    pub project_id: String,
    pub version_id: String,
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct Content {
    #[serde(default, deserialize_with = "read_content_files")]
    pub files: BTreeMap<String, ContentFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct ContentFile {
    pub size_bytes: u64,
    pub hashes: BTreeMap<String, String>,
    pub source: Option<Source>,
    pub download_cache: Option<DownloadCache>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct DownloadCache {
    pub url: String,
    pub resolved_at: Option<String>,
    pub expires_at: Option<String>,
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
    let path = Document::Profile.path(directory);
    read_marker(&path).map(|marker| marker.map(|(_, profile)| profile))
}

fn read_marker(path: &Path) -> Result<Option<(Vec<u8>, Profile)>, DiscoveryError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(DiscoveryError::Read {
                path: path.to_owned(),
                source,
            });
        }
    };
    if !metadata.is_file() || is_link(&metadata) {
        return Err(invalid(path, "the profile marker must be a regular file"));
    }
    let bytes = fs::read(path).map_err(|source| DiscoveryError::Read {
        path: path.to_owned(),
        source,
    })?;
    let profile = parse_marker(path, &bytes)?;
    Ok(Some((bytes, profile)))
}

fn parse_marker(path: &Path, bytes: &[u8]) -> Result<Profile, DiscoveryError> {
    let parse_error = |source| DiscoveryError::Parse {
        path: path.to_owned(),
        source,
    };
    let schema: Schema = serde_json::from_slice(bytes).map_err(parse_error)?;
    if schema.schema_version != SCHEMA_VERSION {
        return Err(DiscoveryError::UnsupportedSchema {
            path: path.to_owned(),
            found: schema.schema_version,
        });
    }
    let mut profile: Profile = serde_json::from_slice(bytes).map_err(parse_error)?;
    validate(&profile).map_err(|reason| invalid(path, reason))?;
    profile.id.make_ascii_lowercase();
    Ok(profile)
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
    if !is_visible_instance_name(&profile.instance.name) {
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
        .find(|id| !axlauncher_domain::is_version_id(id));
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

impl Profile {
    /// A fresh profile: a new UUID, current schema, nothing installed yet.
    ///
    /// The id is generated rather than derived from the directory, so moving a
    /// profile keeps its identity and copying it produces a duplicate the
    /// discovery reports instead of a silent second instance.
    pub fn new(name: impl Into<String>, game: Option<Game>) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            id: uuid::Uuid::new_v4().to_string(),
            instance: ProfileInfo {
                name: name.into(),
                icon: None,
                created_at: None,
                updated_at: None,
            },
            game,
            memory: None,
            install: Install::default(),
            playtime: Playtime::default(),
            source: None,
            content: Content::default(),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn game(&self) -> Option<&Game> {
        self.game.as_ref()
    }

    pub fn memory(&self) -> Option<MaximumHeap> {
        self.memory
    }

    /// A view may apply a committed heap without replacing a newer profile snapshot.
    pub fn apply_heap_snapshot(&mut self, memory: Option<MaximumHeap>) {
        self.memory = memory;
    }
}

/// Why a profile was not written.
#[derive(Debug)]
pub enum ProfileWriteError {
    /// The profile does not satisfy the document's own rules.
    Invalid { reason: String },
    /// The marker belongs to another schema and is left untouched.
    RefusedSchema { path: PathBuf, found: u32 },
    /// The marker is not a document this build can safely replace.
    RefusedMarker { path: PathBuf, reason: String },
    /// The write failed.
    Write { path: PathBuf, source: io::Error },
}

impl fmt::Display for ProfileWriteError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid { reason } => write!(formatter, "invalid profile: {reason}"),
            Self::RefusedSchema { path, found } => write!(
                formatter,
                "{} uses schema {found}; this build writes schema {SCHEMA_VERSION}",
                path.display()
            ),
            Self::RefusedMarker { path, reason } => {
                write!(formatter, "{} cannot be replaced: {reason}", path.display())
            }
            Self::Write { path, source } => {
                write!(formatter, "cannot write {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for ProfileWriteError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Write { source, .. } => Some(source),
            Self::Invalid { .. } | Self::RefusedSchema { .. } | Self::RefusedMarker { .. } => None,
        }
    }
}

/// Writes `profile` as the marker of `directory`.
///
/// A marker this build does not own is refused rather than replaced: another
/// schema's document belongs to that build, and an unreadable file may be the
/// only copy of something a user hand-edited. Keys this build does not know
/// survive a rewrite, because a field added by a newer build within the same
/// schema would otherwise be destroyed by saving over it.
pub fn save(directory: &Path, profile: &Profile) -> Result<(), ProfileWriteError> {
    validate(profile).map_err(|reason| ProfileWriteError::Invalid { reason })?;

    let path = Document::Profile.path(directory);
    let existing = match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if !metadata.is_file() || is_link(&metadata) {
                return Err(ProfileWriteError::RefusedMarker {
                    path,
                    reason: "the profile marker must be a regular file".into(),
                });
            }
            let bytes = fs::read(&path).map_err(|source| ProfileWriteError::Write {
                path: path.clone(),
                source,
            })?;
            let schema: Schema = serde_json::from_slice(&bytes).map_err(|error| {
                ProfileWriteError::RefusedMarker {
                    path: path.clone(),
                    reason: format!("it does not parse: {error}"),
                }
            })?;
            if schema.schema_version != SCHEMA_VERSION {
                return Err(ProfileWriteError::RefusedSchema {
                    path,
                    found: schema.schema_version,
                });
            }
            serde_json::from_slice::<serde_json::Value>(&bytes).ok()
        }
        Err(source) if source.kind() == io::ErrorKind::NotFound => None,
        Err(source) => {
            return Err(ProfileWriteError::Write { path, source });
        }
    };

    let mut document = serde_json::to_value(profile).map_err(|error| ProfileWriteError::Write {
        path: path.clone(),
        source: io::Error::other(error),
    })?;
    if let Some(previous) = existing {
        merge_profile_fields(previous, &mut document);
    }
    // The merge can add entries that this build's rules reject, and a marker
    // that reads back as invalid would make the instance vanish from discovery.
    // The merged document is therefore held to the same rules as the caller's.
    // It is validated through the typed profile; the bytes written are the
    // merged document, so keys this build does not model are not dropped.
    let merged: Profile =
        serde_json::from_value(document.clone()).map_err(|error| ProfileWriteError::Invalid {
            reason: error.to_string(),
        })?;
    validate(&merged).map_err(|reason| ProfileWriteError::Invalid { reason })?;
    write_marker(path, &document)
}

fn write_marker(path: PathBuf, document: &Value) -> Result<(), ProfileWriteError> {
    let bytes = serde_json::to_vec_pretty(document).map_err(|error| ProfileWriteError::Write {
        path: path.clone(),
        source: io::Error::other(error),
    })?;
    write_atomic(&path, &bytes).map_err(|source| ProfileWriteError::Write { path, source })
}

fn merge_profile_fields(mut previous: serde_json::Value, document: &mut serde_json::Value) {
    // The override is one value. Removing it must restore inheritance.
    if let Some(fields) = previous.as_object_mut() {
        fields.remove("memory");
    }
    merge_unknown(previous, document);
}

/// Fields whose members are data entries rather than document fields.
///
/// An entry's key is a content path or a hash algorithm, so an entry the caller
/// removed must stay removed. Merging them would resurrect content that no
/// longer exists and make the document disagree with the disk.
const MAP_FIELDS: [&str; 2] = ["files", "hashes"];

/// Carries keys from `previous` that `written` does not state.
///
/// Recursion stops at arrays and at the map fields above: a list this build
/// wrote replaces the old one whole, because an element-wise merge would have
/// to know what makes two elements the same, and a removed map entry must not
/// come back.
fn merge_unknown(previous: serde_json::Value, written: &mut serde_json::Value) {
    let (serde_json::Value::Object(previous), serde_json::Value::Object(written)) =
        (previous, written)
    else {
        return;
    };
    for (key, previous_value) in previous {
        if MAP_FIELDS.contains(&key.as_str()) {
            continue;
        }
        if let Some(written_value) = written.get_mut(&key) {
            merge_unknown(previous_value, written_value);
            continue;
        }
        written.insert(key, previous_value);
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/state/src/profile.rs"]
mod tests;
