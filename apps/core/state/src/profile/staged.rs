// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::io;
use std::path::{Path, PathBuf};

use axlauncher_storage::{move_instance_folder, stage_instance};

use super::{
    Profile, ProfileFolderName, ProfileWriteError, installation::set_ready, save, validate,
};

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
        profile: Profile,
        version_id: &str,
    ) -> Result<PathBuf, ProfileWriteError> {
        let destination = self.instances().join(profile.id());
        self.publish(profile, version_id, destination)
    }

    /// Publishes to the chosen folder and keeps staging intact if publishing fails.
    pub fn publish_named(
        &self,
        profile: Profile,
        version_id: &str,
        folder: &ProfileFolderName,
    ) -> Result<PathBuf, ProfileWriteError> {
        let destination = folder.path_in(self.instances());
        self.publish(profile, version_id, destination)
    }

    fn instances(&self) -> &Path {
        self.staging
            .path()
            .parent()
            .expect("staged below the profile root")
    }

    fn publish(
        &self,
        mut profile: Profile,
        version_id: &str,
        destination: PathBuf,
    ) -> Result<PathBuf, ProfileWriteError> {
        if !set_ready(&mut profile, version_id) {
            return Err(ProfileWriteError::Invalid {
                reason: "an installed profile must name a game".into(),
            });
        }
        validate(&profile).map_err(|reason| ProfileWriteError::Invalid { reason })?;
        save(&self.directory, &profile)?;
        move_instance_folder(&self.directory, &destination).map_err(|source| {
            ProfileWriteError::Write {
                path: destination.clone(),
                source,
            }
        })?;
        Ok(destination)
    }
}
