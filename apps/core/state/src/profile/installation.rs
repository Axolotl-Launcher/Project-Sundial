// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::fmt;
use std::path::{Path, PathBuf};

use super::{DiscoveryError, InstallStatus, ProfileWriteError, read, save};

#[derive(Debug)]
pub enum InstallUpdateError {
    Read(DiscoveryError),
    Write(ProfileWriteError),
    MissingProfile(PathBuf),
    MissingGame(PathBuf),
}

impl fmt::Display for InstallUpdateError {
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
        }
    }
}

impl std::error::Error for InstallUpdateError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read(error) => Some(error),
            Self::Write(error) => Some(error),
            Self::MissingProfile(_) | Self::MissingGame(_) => None,
        }
    }
}

/// The newly installed version belongs to the current profile on disk.
pub fn mark_installed(directory: &Path, version_id: &str) -> Result<(), InstallUpdateError> {
    let mut profile = read(directory)
        .map_err(InstallUpdateError::Read)?
        .ok_or_else(|| InstallUpdateError::MissingProfile(directory.to_path_buf()))?;
    let game = profile
        .game
        .as_mut()
        .ok_or_else(|| InstallUpdateError::MissingGame(directory.to_path_buf()))?;
    game.version_id = Some(version_id.to_owned());
    profile.install.status = InstallStatus::Ready;
    save(directory, &profile).map_err(InstallUpdateError::Write)
}

/// A failed attempt leaves the last installed version ID in place.
pub fn mark_failed(directory: &Path) -> Result<(), InstallUpdateError> {
    let Some(mut profile) = read(directory).map_err(InstallUpdateError::Read)? else {
        return Ok(());
    };
    profile.install.status = InstallStatus::Failed;
    save(directory, &profile).map_err(InstallUpdateError::Write)
}
