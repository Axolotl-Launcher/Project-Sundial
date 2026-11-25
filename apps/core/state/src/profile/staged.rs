// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{Profile, ProfileWriteError, save, validate};

/// A new instance that becomes discoverable only when it is published.
pub struct StagedProfile {
    staging: tempfile::TempDir,
    directory: PathBuf,
}

impl StagedProfile {
    /// Places the staging tree on the same filesystem as the managed profiles.
    pub fn new(instances: &Path) -> io::Result<Self> {
        fs::create_dir_all(instances)?;
        let staging = tempfile::Builder::new()
            .prefix(".staging-")
            .tempdir_in(instances)?;
        let directory = staging.path().join("instance");
        fs::create_dir(&directory)?;
        Ok(Self { staging, directory })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// Saves the completed profile and atomically moves its directory into discovery.
    pub fn publish(self, profile: &Profile) -> Result<PathBuf, ProfileWriteError> {
        validate(profile).map_err(|reason| ProfileWriteError::Invalid { reason })?;
        let instances = self
            .staging
            .path()
            .parent()
            .expect("staged below the profile root");
        let destination = instances.join(&profile.id);
        if destination.exists() {
            return Err(ProfileWriteError::Write {
                path: destination,
                source: io::Error::from(io::ErrorKind::AlreadyExists),
            });
        }
        save(&self.directory, profile)?;
        fs::rename(&self.directory, &destination).map_err(|source| ProfileWriteError::Write {
            path: destination.clone(),
            source,
        })?;
        Ok(destination)
    }
}
