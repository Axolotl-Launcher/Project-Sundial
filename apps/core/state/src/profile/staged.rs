// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use axlauncher_storage::stage_instance;

use super::{Profile, ProfileWriteError, installation::set_ready, save, validate};

/// A new instance that becomes discoverable only when it is published.
pub struct StagedProfile {
    staging: tempfile::TempDir,
    directory: PathBuf,
}

impl StagedProfile {
    /// Places the staging tree on the same filesystem as the managed profiles.
    pub fn new(instances: &Path) -> io::Result<Self> {
        let (staging, directory) = stage_instance(instances)?;
        Ok(Self { staging, directory })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// A staged profile becomes visible with the version that was prepared for it.
    pub fn publish_installed(
        self,
        mut profile: Profile,
        version_id: &str,
    ) -> Result<PathBuf, ProfileWriteError> {
        if !set_ready(&mut profile, version_id) {
            return Err(ProfileWriteError::Invalid {
                reason: "an installed profile must name a game".into(),
            });
        }
        validate(&profile).map_err(|reason| ProfileWriteError::Invalid { reason })?;
        let instances = self
            .staging
            .path()
            .parent()
            .expect("staged below the profile root");
        let destination = instances.join(profile.id());
        if destination.exists() {
            return Err(ProfileWriteError::Write {
                path: destination,
                source: io::Error::from(io::ErrorKind::AlreadyExists),
            });
        }
        save(&self.directory, &profile)?;
        fs::rename(&self.directory, &destination).map_err(|source| ProfileWriteError::Write {
            path: destination.clone(),
            source,
        })?;
        Ok(destination)
    }
}
