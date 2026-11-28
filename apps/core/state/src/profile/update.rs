// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use axlauncher_domain::memory::MaximumHeap;
use axlauncher_storage::{
    DataDir, DataPath, Document, instance_move_path, move_instance_folder, write_atomic,
};
use serde_json::Value;

use super::{
    DiscoveryError, Profile, ProfileFolderName, ProfileWriteError,
    folders::{check_identity, current_directory, restore_after_move_failure},
    invalid, is_link, read, read_marker, save, write_marker,
};

#[derive(Debug)]
pub enum ProfileUpdateError {
    Read(DiscoveryError),
    Write(ProfileWriteError),
    MissingProfile(PathBuf),
    MissingGame(PathBuf),
    ChangedProfile(PathBuf),
    ChangedField(PathBuf),
    Cancelled,
}

impl fmt::Display for ProfileUpdateError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => error.fmt(formatter),
            Self::Write(error) => error.fmt(formatter),
            Self::MissingProfile(directory) => {
                write!(
                    formatter,
                    "{} no longer holds a profile document.",
                    directory.display()
                )
            }
            Self::MissingGame(directory) => {
                write!(formatter, "{} no longer names a game.", directory.display())
            }
            Self::ChangedProfile(directory) => {
                write!(
                    formatter,
                    "{} now holds a different profile.",
                    directory.display()
                )
            }
            Self::ChangedField(directory) => {
                write!(
                    formatter,
                    "The instance settings in {} changed. Reopen the editor to continue.",
                    directory.display()
                )
            }
            Self::Cancelled => write!(formatter, "The settings edit was cancelled."),
        }
    }
}

impl std::error::Error for ProfileUpdateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read(error) => Some(error),
            Self::Write(error) => Some(error),
            _ => None,
        }
    }
}

/// A heap edit is conditional on the override the caller originally read.
pub fn set_memory_if_current(
    directory: &Path,
    expected_id: &str,
    expected: Option<MaximumHeap>,
    memory: Option<MaximumHeap>,
    begin_commit: impl FnOnce() -> bool,
) -> Result<Profile, ProfileUpdateError> {
    edit_field(
        directory,
        expected_id,
        begin_commit,
        None,
        |profile, document| {
            if profile.memory != expected {
                return Err(ProfileUpdateError::ChangedField(directory.to_owned()));
            }
            edit_memory(directory, profile, document, memory)
        },
    )
}

/// A display name edit never moves the directory or replaces the profile ID.
pub fn rename_if_current(
    directory: &Path,
    expected_id: &str,
    expected: &str,
    name: &str,
    begin_commit: impl FnOnce() -> bool,
) -> Result<Profile, ProfileUpdateError> {
    rename_profile(directory, expected_id, expected, name, begin_commit, None)
}

/// Publishes the new name with the moved folder. A failed move restores the old marker.
pub fn rename_with_folder_if_current(
    paths: &DataDir,
    directory: &Path,
    expected_id: &str,
    expected: &str,
    name: &str,
    folder: &ProfileFolderName,
    begin_commit: impl FnOnce() -> bool,
) -> Result<PathBuf, ProfileUpdateError> {
    let resolved = current_directory(paths, directory, expected_id)
        .map_err(|source| edit_io(directory, source))?;
    let parent = resolved.parent().expect("checked direct child");
    let destination = folder.path_in(parent);
    let moving = (resolved != destination).then_some(destination.as_path());
    if moving.is_some() && !same_directory(&resolved, &destination) {
        folder
            .check_available(parent)
            .map_err(|source| edit_io(&destination, source))?;
    }
    rename_profile(&resolved, expected_id, expected, name, begin_commit, moving)?;
    Ok(folder.path_in(paths.path(DataPath::Profiles)))
}

fn rename_profile(
    directory: &Path,
    expected_id: &str,
    expected: &str,
    name: &str,
    begin_commit: impl FnOnce() -> bool,
    destination: Option<&Path>,
) -> Result<Profile, ProfileUpdateError> {
    let name = name.trim();
    if !axlauncher_domain::is_visible_instance_name(name) {
        return Err(ProfileUpdateError::Write(ProfileWriteError::Invalid {
            reason: "instance name must be visible text".into(),
        }));
    }
    edit_field(
        directory,
        expected_id,
        begin_commit,
        destination,
        |profile, document| edit_name(directory, profile, document, expected, name),
    )
}

fn edit_name(
    directory: &Path,
    profile: &mut Profile,
    document: &mut Value,
    expected: &str,
    name: &str,
) -> Result<(), ProfileUpdateError> {
    if profile.instance.name != expected {
        return Err(ProfileUpdateError::ChangedField(directory.to_owned()));
    }
    let Some(info) = document.get_mut("instance").and_then(Value::as_object_mut) else {
        return Err(ProfileUpdateError::Read(invalid(
            &Document::Profile.path(directory),
            "instance details must be an object",
        )));
    };
    profile.instance.name = name.to_owned();
    info.insert("name".into(), name.into());
    Ok(())
}

fn edit_memory(
    directory: &Path,
    profile: &mut Profile,
    document: &mut Value,
    memory: Option<MaximumHeap>,
) -> Result<(), ProfileUpdateError> {
    if profile.game().is_none() {
        return Err(ProfileUpdateError::MissingGame(directory.to_owned()));
    }
    profile.memory = memory;
    let Some(fields) = document.as_object_mut() else {
        return Err(ProfileUpdateError::Read(invalid(
            &Document::Profile.path(directory),
            "the profile must be an object",
        )));
    };
    if let Some(heap) = memory {
        fields.insert(
            "memory".into(),
            serde_json::to_value(heap).expect("heap serializes"),
        );
    } else {
        fields.remove("memory");
    }
    Ok(())
}

fn edit_field(
    directory: &Path,
    expected_id: &str,
    begin_commit: impl FnOnce() -> bool,
    destination: Option<&Path>,
    edit: impl FnOnce(&mut Profile, &mut Value) -> Result<(), ProfileUpdateError>,
) -> Result<Profile, ProfileUpdateError> {
    check_directory(directory)?;
    let _lock = lock_profile(directory)?
        .ok_or_else(|| ProfileUpdateError::MissingProfile(directory.to_owned()))?;
    check_directory(directory)?;
    let path = Document::Profile.path(directory);
    let (bytes, mut profile) = read_marker(&path)
        .map_err(ProfileUpdateError::Read)?
        .ok_or_else(|| ProfileUpdateError::MissingProfile(directory.to_owned()))?;
    if profile.id() != expected_id {
        return Err(ProfileUpdateError::ChangedProfile(directory.to_owned()));
    }
    let mut document = serde_json::from_slice(&bytes).map_err(|source| {
        ProfileUpdateError::Read(DiscoveryError::Parse {
            path: path.clone(),
            source,
        })
    })?;
    edit(&mut profile, &mut document)?;
    if !begin_commit() {
        return Err(ProfileUpdateError::Cancelled);
    }
    check_directory(directory)?;
    if read_marker(&path)
        .map_err(ProfileUpdateError::Read)?
        .is_none_or(|(current, _)| current != bytes)
    {
        return Err(ProfileUpdateError::ChangedProfile(directory.to_owned()));
    }
    write_marker(path.clone(), &document).map_err(ProfileUpdateError::Write)?;
    if let Some(destination) = destination {
        move_folder(directory, destination, &path, &bytes, expected_id)?;
    }
    Ok(profile)
}

fn check_directory(directory: &Path) -> Result<(), ProfileUpdateError> {
    let metadata = std::fs::symlink_metadata(directory).map_err(|source| {
        ProfileUpdateError::Read(DiscoveryError::Read {
            path: directory.to_owned(),
            source,
        })
    })?;
    if !metadata.is_dir() || is_link(&metadata) {
        return Err(ProfileUpdateError::ChangedProfile(directory.to_owned()));
    }
    Ok(())
}

fn move_folder(
    directory: &Path,
    destination: &Path,
    marker: &Path,
    previous: &[u8],
    id: &str,
) -> Result<(), ProfileUpdateError> {
    let Err(source) = rename_directory(directory, destination, id) else {
        return Ok(());
    };
    let rollback = check_identity(directory, id).and_then(|()| write_atomic(marker, previous));
    if let Err(rollback) = rollback {
        return Err(edit_io(
            marker,
            io::Error::other(format!(
                "Could not move the folder to {} ({source}) or restore the previous display name ({rollback}).",
                destination.display(),
            )),
        ));
    }
    Err(edit_io(destination, source))
}

fn rename_directory(directory: &Path, destination: &Path, id: &str) -> io::Result<()> {
    if !same_directory(directory, destination) {
        move_instance_folder(directory, destination)?;
        return check_identity(destination, id)
            .or_else(|error| restore_after_move_failure(directory, destination, error));
    }
    // A case-insensitive volume resolves both spellings to the source. Claim
    // the new spelling through an unused name, retaining no-replace semantics.
    let holding = instance_move_path(directory)?;
    move_instance_folder(directory, &holding)?;
    check_identity(&holding, id)
        .and_then(|()| move_instance_folder(&holding, destination))
        .or_else(|error| restore_after_move_failure(directory, &holding, error))
}

fn same_directory(source: &Path, destination: &Path) -> bool {
    let Ok(metadata) = destination.symlink_metadata() else {
        return false;
    };
    if !metadata.is_dir() || is_link(&metadata) {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        source.symlink_metadata().is_ok_and(|source| {
            source.is_dir()
                && !is_link(&source)
                && (source.dev(), source.ino()) == (metadata.dev(), metadata.ino())
        })
    }
    #[cfg(not(unix))]
    {
        let Ok(original) = source.canonicalize() else {
            return false;
        };
        destination
            .canonicalize()
            .is_ok_and(|path| path == original)
    }
}

fn edit_io(path: &Path, source: io::Error) -> ProfileUpdateError {
    ProfileUpdateError::Write(ProfileWriteError::Write {
        path: path.to_owned(),
        source,
    })
}

pub(super) fn update(
    directory: &Path,
    change: impl FnOnce(&mut Profile) -> Result<(), ProfileUpdateError>,
) -> Result<Option<Profile>, ProfileUpdateError> {
    let Some(_lock) = lock_profile(directory)? else {
        return Ok(None);
    };
    let Some(mut profile) = read(directory).map_err(ProfileUpdateError::Read)? else {
        return Ok(None);
    };
    change(&mut profile)?;
    save(directory, &profile).map_err(ProfileUpdateError::Write)?;
    Ok(Some(profile))
}

fn lock_profile(directory: &Path) -> Result<Option<std::fs::File>, ProfileUpdateError> {
    match Document::Profile.lock(directory).map_err(io::Error::from) {
        Ok(lock) => Ok(Some(lock)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ProfileUpdateError::Write(ProfileWriteError::Write {
            path: directory.to_owned(),
            source,
        })),
    }
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/state/src/profile/update.rs"]
mod tests;
