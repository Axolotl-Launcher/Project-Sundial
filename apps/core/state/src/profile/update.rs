// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use axlauncher_domain::memory::MaximumHeap;
use axlauncher_storage::Document;
use serde_json::Value;

use super::{
    DiscoveryError, Profile, ProfileWriteError, invalid, is_link, read, read_marker, save,
    write_marker,
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
    edit_field(directory, expected_id, begin_commit, |profile, document| {
        if profile.memory != expected {
            return Err(ProfileUpdateError::ChangedField(directory.to_owned()));
        }
        edit_memory(directory, profile, document, memory)
    })
}

/// A display name edit never moves the directory or replaces the profile ID.
pub fn rename_if_current(
    directory: &Path,
    expected_id: &str,
    expected: &str,
    name: &str,
    begin_commit: impl FnOnce() -> bool,
) -> Result<Profile, ProfileUpdateError> {
    let name = name.trim();
    if !axlauncher_domain::is_visible_instance_name(name) {
        return Err(ProfileUpdateError::Write(ProfileWriteError::Invalid {
            reason: "instance name must be visible text".into(),
        }));
    }
    edit_field(directory, expected_id, begin_commit, |profile, document| {
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
    })
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
    edit: impl FnOnce(&mut Profile, &mut Value) -> Result<(), ProfileUpdateError>,
) -> Result<Profile, ProfileUpdateError> {
    let metadata = std::fs::symlink_metadata(directory).map_err(|source| {
        ProfileUpdateError::Read(DiscoveryError::Read {
            path: directory.to_owned(),
            source,
        })
    })?;
    if !metadata.is_dir() || is_link(&metadata) {
        return Err(ProfileUpdateError::ChangedProfile(directory.to_owned()));
    }
    let _lock = lock_profile(directory)?
        .ok_or_else(|| ProfileUpdateError::MissingProfile(directory.to_owned()))?;
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
    write_marker(path, &document).map_err(ProfileUpdateError::Write)?;
    Ok(profile)
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
