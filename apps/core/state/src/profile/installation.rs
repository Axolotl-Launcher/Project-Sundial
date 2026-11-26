// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::path::Path;

use super::{
    InstallStatus, Profile,
    update::{ProfileUpdateError, update},
};

/// The newly installed version belongs to the current profile on disk.
pub fn mark_installed(directory: &Path, version_id: &str) -> Result<(), ProfileUpdateError> {
    update(directory, |profile| {
        if !set_ready(profile, version_id) {
            return Err(ProfileUpdateError::MissingGame(directory.to_path_buf()));
        }
        Ok(())
    })?
    .ok_or_else(|| ProfileUpdateError::MissingProfile(directory.to_path_buf()))?;
    Ok(())
}

pub(super) fn set_ready(profile: &mut Profile, version_id: &str) -> bool {
    let Some(game) = profile.game.as_mut() else {
        return false;
    };
    game.version_id = Some(version_id.to_owned());
    profile.install.status = InstallStatus::Ready;
    true
}

/// A failed attempt leaves the last installed version ID in place.
pub fn mark_failed(directory: &Path) -> Result<(), ProfileUpdateError> {
    update(directory, |profile| {
        profile.install.status = InstallStatus::Failed;
        Ok(())
    })?;
    Ok(())
}
