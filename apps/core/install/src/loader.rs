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
/// The document is written first: a caller that stops here still has a version
/// the launcher can name, and the next run re-places the rest.
pub fn place(game_root: &Path, profile: &LoaderProfile) -> Result<Placed, PlaceError> {
    check_id(&profile.version_id)?;

    let document = document_path(game_root, &profile.version_id);
    write(&document, &profile.document)?;

    for file in &profile.files {
        let relative = safe_relative(&file.path).ok_or_else(|| PlaceError::InvalidPath {
            path: file.path.clone(),
        })?;
        write(&game_root.join(relative), &file.bytes)?;
    }

    Ok(Placed {
        version_id: profile.version_id.clone(),
        document,
        files: profile.files.len(),
    })
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), PlaceError> {
    write_atomic(path, bytes).map_err(|source| PlaceError::Write {
        path: path.to_path_buf(),
        source,
    })
}

/// A version id is a directory name: no separators, no traversal, not empty.
fn check_id(id: &str) -> Result<(), PlaceError> {
    let usable = !id.is_empty()
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
    if path.is_empty() || path.starts_with('/') || path.contains('\\') {
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

#[cfg(test)]
#[path = "../../../../tests/apps/core/install/src/loader.rs"]
mod tests;
