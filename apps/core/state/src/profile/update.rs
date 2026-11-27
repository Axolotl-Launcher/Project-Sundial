// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use axlauncher_domain::memory::MaximumHeap;
use axlauncher_storage::lock_profile;

use super::{DiscoveryError, Profile, ProfileWriteError, read, save};

#[derive(Debug)]
pub enum ProfileUpdateError {
    Read(DiscoveryError),
    Write(ProfileWriteError),
    MissingProfile(PathBuf),
    MissingGame(PathBuf),
    ChangedProfile(PathBuf),
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
        }
    }
}

impl std::error::Error for ProfileUpdateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read(error) => Some(error),
            Self::Write(error) => Some(error),
            Self::MissingProfile(_) | Self::MissingGame(_) | Self::ChangedProfile(_) => None,
        }
    }
}

/// Writes the manual heap override, or removes it to follow global settings.
pub fn set_memory(
    directory: &Path,
    expected_id: &str,
    memory: Option<MaximumHeap>,
) -> Result<Profile, ProfileUpdateError> {
    update(directory, |profile| {
        if profile.id() != expected_id {
            return Err(ProfileUpdateError::ChangedProfile(directory.to_path_buf()));
        }
        if profile.game().is_none() {
            return Err(ProfileUpdateError::MissingGame(directory.to_path_buf()));
        }
        profile.memory = memory;
        Ok(())
    })?
    .ok_or_else(|| ProfileUpdateError::MissingProfile(directory.to_path_buf()))
}

pub(super) fn update(
    directory: &Path,
    change: impl FnOnce(&mut Profile) -> Result<(), ProfileUpdateError>,
) -> Result<Option<Profile>, ProfileUpdateError> {
    let _lock = match lock_profile(directory) {
        Ok(lock) => lock,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(ProfileUpdateError::Write(ProfileWriteError::Write {
                path: directory.to_path_buf(),
                source,
            }));
        }
    };
    let Some(mut profile) = read(directory).map_err(ProfileUpdateError::Read)? else {
        return Ok(None);
    };
    change(&mut profile)?;
    save(directory, &profile).map_err(ProfileUpdateError::Write)?;
    Ok(Some(profile))
}
