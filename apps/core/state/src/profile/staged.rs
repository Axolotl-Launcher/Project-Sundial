// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use axlauncher_domain::{is_portable_directory_name, is_visible_instance_name};
use axlauncher_storage::{is_reserved_instance_directory, publish_instance, stage_instance};

use super::{Profile, ProfileWriteError, installation::set_ready, save, validate};

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
        let destination = instances.join(&self.0);
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
}

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
        let destination = self.instances().join(&folder.0);
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
        publish_instance(&self.directory, &destination).map_err(|source| {
            ProfileWriteError::Write {
                path: destination.clone(),
                source,
            }
        })?;
        Ok(destination)
    }
}
