// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::fs;
use std::path::{Path, PathBuf};

use super::{Game, Profile, ProfileWriteError, save, validate};

/// A new managed profile under its UUID, independent of its display name.
///
/// The directory is reserved before writing so concurrent creations cannot
/// replace an existing profile. Invalid input never creates a directory.
pub fn create(instances: &Path, name: &str, mut game: Game) -> Result<PathBuf, ProfileWriteError> {
    let version = game.minecraft.trim();
    if !valid_version(version) {
        return Err(ProfileWriteError::Invalid {
            reason: "Enter a Minecraft version ID such as 1.21.11.".into(),
        });
    }
    game.minecraft = version.to_owned();
    let profile = Profile::new(name.trim(), Some(game));
    validate(&profile).map_err(|reason| ProfileWriteError::Invalid { reason })?;

    let directory = instances.join(profile.id());
    fs::create_dir_all(instances).map_err(|source| ProfileWriteError::Write {
        path: instances.to_path_buf(),
        source,
    })?;
    fs::create_dir(&directory).map_err(|source| ProfileWriteError::Write {
        path: directory.clone(),
        source,
    })?;
    if let Err(error) = save(&directory, &profile) {
        let _ = fs::remove_dir(&directory);
        return Err(error);
    }
    Ok(directory)
}

fn valid_version(version: &str) -> bool {
    !version.is_empty()
        && !version.ends_with('.')
        && version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_' | b'+'))
}
