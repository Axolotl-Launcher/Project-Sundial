// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Folder operations for an existing managed profile.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use axlauncher_storage::{self as storage, DataDir, DataPath, Document, FolderRemoval};

use super::{is_link, read};

/// Opens the directory of the profile the caller saw in discovery.
pub fn open_folder(paths: &DataDir, directory: &Path, id: &str) -> io::Result<()> {
    let directory = current_directory(paths, directory, id)?;
    storage::open_folder(&directory)
}

/// Removes the directory only if it still belongs to the confirmed profile.
pub fn delete_folder(paths: &DataDir, directory: &Path, id: &str) -> io::Result<FolderRemoval> {
    let directory = current_directory(paths, directory, id)?;
    let _lock = Document::Profile
        .lock(&directory)
        .map_err(io::Error::from)?;
    check_identity(&directory, id)?;
    storage::remove_folder(&directory)
}

fn current_directory(paths: &DataDir, directory: &Path, id: &str) -> io::Result<PathBuf> {
    let profiles = paths.path(DataPath::Profiles);
    let Some(name) = directory
        .file_name()
        .filter(|_| directory.parent() == Some(profiles))
    else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "The folder is outside the instance directory.",
        ));
    };
    let directory = dunce::canonicalize(profiles)?.join(name);
    check_identity(&directory, id)?;
    Ok(directory)
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/state/src/profile/folders.rs"]
mod tests;

fn check_identity(directory: &Path, id: &str) -> io::Result<()> {
    let metadata = fs::symlink_metadata(directory)?;
    if metadata.is_dir()
        && !is_link(&metadata)
        && read(directory)
            .map_err(io::Error::other)?
            .is_some_and(|profile| profile.id() == id)
    {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "The folder no longer holds the selected instance.",
    ))
}
