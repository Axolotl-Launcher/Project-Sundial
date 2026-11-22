// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Writing a resolved loader profile into the game root.
//!
//! A [`ModLoader`] provider answers with a version document and, for a loader
//! that needs them, the files its installer would place. Neither touches the
//! filesystem: which directory a version document lives in, and what a safe
//! relative path is, belong to the layout this system owns.
//!
//! The id and the paths come from a network answer, so both are checked here
//! rather than trusted: a version id becomes a directory name and a file path
//! is joined under the game root.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use axlauncher_launch::{ChainError, Version, resolve};
use axlauncher_spi::LoaderProfile;
use axlauncher_storage::write_atomic;

/// Longest version id accepted; long enough for any loader's own scheme.
const MAX_ID_LEN: usize = 120;

/// What a placed profile left on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placed {
    /// The id the profile was written under.
    pub version_id: String,
    /// The version document's path.
    pub document: PathBuf,
    /// How many extra files the loader brought with it.
    pub files: usize,
}

/// Why a profile was not placed.
#[derive(Debug)]
pub enum PlaceError {
    /// The profile's version id cannot be a directory name.
    InvalidId { id: String },
    /// One of the profile's file paths could leave the game root.
    InvalidPath { path: String },
    /// The provider's version document could not be decoded or encoded.
    Document {
        id: String,
        source: serde_json::Error,
    },
    /// The profile's inherited game version could not be resolved.
    Inheritance { source: ChainError },
    /// A write failed.
    Write { path: PathBuf, source: io::Error },
}

impl fmt::Display for PlaceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidId { id } => write!(
                formatter,
                "\"{id}\" is not a usable version id: use 1-{MAX_ID_LEN} characters from a-z, A-Z, 0-9, '-', '_', '.' and '+'"
            ),
            Self::InvalidPath { path } => {
                write!(formatter, "\"{path}\" is not a path inside the game root")
            }
            Self::Document { id, source } => {
                write!(formatter, "cannot process profile {id}: {source}")
            }
            Self::Inheritance { source } => {
                write!(formatter, "cannot resolve the loader profile: {source}")
            }
            Self::Write { path, source } => {
                write!(formatter, "cannot write {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for PlaceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Write { source, .. } => Some(source),
            Self::Document { source, .. } => Some(source),
            Self::Inheritance { source } => Some(source),
            Self::InvalidId { .. } | Self::InvalidPath { .. } => None,
        }
    }
}

/// Where a version document lives under `game_root`.
pub fn document_path(game_root: &Path, version_id: &str) -> PathBuf {
    game_root
        .join("versions")
        .join(version_id)
        .join(format!("{version_id}.json"))
}

/// Writes `profile` into `game_root` and returns what it left there.
///
/// The parent version document must already exist. The returned id names the
/// resolved profile and the client JAR that installation will prepare for it.
pub fn place(game_root: &Path, profile: &LoaderProfile) -> Result<Placed, PlaceError> {
    check_id(&profile.version_id)?;
    let document = document_path(game_root, &profile.version_id);
    let files = profile
        .files
        .iter()
        .map(|file| {
            safe_relative(&file.path)
                .map(|path| (path, &file.bytes))
                .ok_or_else(|| PlaceError::InvalidPath {
                    path: file.path.clone(),
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let bytes = resolve_document(game_root, profile)?;
    for (relative, bytes) in files {
        write(&game_root.join(relative), bytes)?;
    }
    write(&document, &bytes)?;

    Ok(Placed {
        version_id: profile.version_id.clone(),
        document,
        files: profile.files.len(),
    })
}

fn resolve_document(game_root: &Path, profile: &LoaderProfile) -> Result<Vec<u8>, PlaceError> {
    let document_error = |source| PlaceError::Document {
        id: profile.version_id.clone(),
        source,
    };
    let mut version: Version = serde_json::from_slice(&profile.document).map_err(document_error)?;
    if version.id == profile.version_id && version.inherits_from.is_none() {
        // A profile that is already self-contained still needs a jar id: with
        // none it would run from a jar named after itself, and a document that
        // never declares one has no parent to borrow it from.
        if version.jar.is_some() {
            return Ok(profile.document.clone());
        }
        version.jar = Some(version.id.clone());
        return serde_json::to_vec(&version).map_err(document_error);
    }
    version.id.clone_from(&profile.version_id);
    // The resolution below already keeps the profile's jar when it names one
    // and the parent's otherwise, defaulting to the profile's own id last.
    // Overwriting `jar` here would discard the parent's, which is the jar a
    // Fabric or Quilt profile must run.
    let resolved = resolve(&game_root.join("versions"), version)
        .map_err(|source| PlaceError::Inheritance { source })?;
    serde_json::to_vec(&resolved).map_err(document_error)
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), PlaceError> {
    write_atomic(path, bytes).map_err(|source| PlaceError::Write {
        path: path.to_path_buf(),
        source,
    })
}

/// A version id is a directory name: no separators, no traversal, not empty.
fn check_id(id: &str) -> Result<(), PlaceError> {
    let usable = !matches!(id, "" | "." | "..")
        && id.len() <= MAX_ID_LEN
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '+'));
    if usable {
        Ok(())
    } else {
        Err(PlaceError::InvalidId { id: id.to_owned() })
    }
}

/// A loader file's path, checked to stay under the game root.
///
/// The format writes these with `/`, so a backslash is a path from another
/// system rather than a separator to translate, and `..` is refused instead of
/// resolved: a network answer has no business naming anything above the root.
fn safe_relative(path: &str) -> Option<PathBuf> {
    if path.is_empty() || path.starts_with('/') || path.contains(['\\', ':']) {
        return None;
    }
    let mut relative = PathBuf::new();
    for segment in path.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return None;
        }
        relative.push(segment);
    }
    Some(relative)
}

/// A loader that is installed and can be launched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoaderInstalled {
    /// The version id the launch asks for; the document on disk is under it.
    pub version_id: String,
    pub prepared: crate::prepare::Prepared,
}

/// Why a loader install stopped.
#[derive(Debug)]
pub enum LoaderInstallError {
    /// The vanilla version underneath the loader could not be acquired.
    Vanilla(String),
    /// The loader's own metadata could not be resolved.
    Provider(axlauncher_spi::LoaderError),
    /// The resolved profile could not be written.
    Place(PlaceError),
    /// The profile on disk could not be read back.
    Document {
        id: String,
        source: axlauncher_launch::version::VersionError,
    },
    /// The files the profile names could not be prepared.
    Prepare(crate::prepare::PrepareError),
}

impl fmt::Display for LoaderInstallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Vanilla(message) => formatter.write_str(message),
            Self::Provider(error) => write!(formatter, "cannot resolve the loader: {error}"),
            Self::Place(error) => write!(formatter, "cannot write the loader profile: {error}"),
            Self::Document { id, source } => {
                write!(formatter, "cannot read loader profile {id}: {source}")
            }
            Self::Prepare(error) => write!(formatter, "cannot prepare the loader's files: {error}"),
        }
    }
}

impl std::error::Error for LoaderInstallError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Provider(source) => Some(source),
            Self::Place(source) => Some(source),
            Self::Document { source, .. } => Some(source),
            Self::Prepare(source) => Some(source),
            Self::Vanilla(_) => None,
        }
    }
}

/// A loader install is one download pass: the vanilla version underneath is
/// acquired first, the profile is placed as a self-contained document (which
/// resolves what it inherits and needs the parent on disk), and every file the
/// resolved profile names is prepared. Preparing the resolved profile is what
/// keeps the parent's client jar and libraries in the same pass instead of a
/// second one.
///
/// `loader` is supplied by the caller rather than chosen here: this system may
/// not depend on a provider, and which provider serves a kind is a wiring
/// decision.
pub async fn install(
    network: &impl axlauncher_spi::Network,
    loader: &impl axlauncher_spi::ModLoader,
    game_root: &Path,
    request: axlauncher_spi::LoaderRequest,
    target: &axlauncher_launch::Target,
    concurrency: usize,
    mut report: impl FnMut(crate::prepare::Progress),
) -> Result<LoaderInstalled, LoaderInstallError> {
    let _lock =
        crate::vanilla::installation_lock(game_root).map_err(LoaderInstallError::Vanilla)?;

    report(crate::prepare::Progress::Version);
    crate::vanilla::ensure_version(network, game_root, &request.game_version)
        .await
        .map_err(LoaderInstallError::Vanilla)?;

    report(crate::prepare::Progress::Loader);
    let profile = loader
        .profile(request)
        .await
        .map_err(LoaderInstallError::Provider)?;
    let placed = place(game_root, &profile).map_err(LoaderInstallError::Place)?;

    let version = axlauncher_launch::version::load(&game_root.join("versions"), &placed.version_id)
        .map_err(|source| LoaderInstallError::Document {
            id: placed.version_id.clone(),
            source,
        })?;

    let prepared = crate::prepare::prepare_reporting(
        network,
        game_root,
        &version,
        target,
        concurrency,
        report,
    )
    .await
    .map_err(LoaderInstallError::Prepare)?;

    Ok(LoaderInstalled {
        version_id: placed.version_id,
        prepared,
    })
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/install/src/loader.rs"]
mod tests;
