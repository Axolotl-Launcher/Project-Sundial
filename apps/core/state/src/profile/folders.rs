// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Folder operations for an existing managed profile.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use axlauncher_domain::{is_portable_directory_name, is_visible_instance_name};
use axlauncher_storage::{
    self as storage, DataDir, DataPath, Document, FolderRemoval, is_reserved_instance_directory,
};

use super::{is_link, read};

/// A user-selected name for a single directory in the profile root.
#[derive(Debug)]
pub struct ProfileFolderName(String);

impl ProfileFolderName {
    pub fn new(name: &str) -> io::Result<Self> {
        const MAX_NAME_BYTES: usize = 255;
        let error = |message| io::Error::new(io::ErrorKind::InvalidInput, message);
        if !is_visible_instance_name(name) || !is_portable_directory_name(name) {
            return Err(error(
                "Enter a folder name that is valid on Windows, macOS, and Linux.",
            ));
        }
        if name.len() > MAX_NAME_BYTES {
            return Err(error("The folder name is too long."));
        }
        if is_reserved_instance_directory(name) {
            return Err(error("This folder name is reserved by the launcher."));
        }
        Ok(Self(name.to_owned()))
    }

    /// A dangling symlink still reserves the chosen folder name.
    pub fn check_available(&self, instances: &Path) -> io::Result<()> {
        let destination = self.path_in(instances);
        match fs::symlink_metadata(&destination) {
            Ok(_) => Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!(
                    "{} already exists. Choose another folder name.",
                    destination.display()
                ),
            )),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error),
        }
    }

    pub(super) fn path_in(&self, instances: &Path) -> PathBuf {
        instances.join(&self.0)
    }
}

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
    let holding = storage::instance_move_path(&directory)?;
    storage::move_instance_folder(&directory, &holding)?;
    remove_held(&directory, &holding, id)
}

fn remove_held(original: &Path, holding: &Path, id: &str) -> io::Result<FolderRemoval> {
    // The no-replace move captures one directory entry. Validate that entry
    // before passing its private name to the path-based system trash API.
    let removal = check_identity(holding, id).and_then(|()| storage::remove_folder(holding));
    removal.or_else(|error| restore_after_move_failure(original, holding, error))
}

pub(super) fn restore_after_move_failure<T>(
    original: &Path,
    holding: &Path,
    error: io::Error,
) -> io::Result<T> {
    match storage::move_instance_folder(holding, original) {
        Ok(()) => Err(error),
        Err(restore) => Err(io::Error::other(format!(
            "{error}. The folder could not be moved back from {} ({restore}).",
            holding.display(),
        ))),
    }
}

pub(super) fn current_directory(
    paths: &DataDir,
    directory: &Path,
    id: &str,
) -> io::Result<PathBuf> {
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

pub(super) fn check_identity(directory: &Path, id: &str) -> io::Result<()> {
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
