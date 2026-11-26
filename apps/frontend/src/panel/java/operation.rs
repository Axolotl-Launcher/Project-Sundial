// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Java controls and their corresponding desktop service operations.

use std::path::PathBuf;

use axlauncher_core::desktop::java::{self, JavaCatalog};
use axlauncher_core::storage::DataDir;

pub(super) enum Activity {
    Loading,
    Starting,
    Discovering,
    ChoosingFile,
    SavingCatalog,
    SavingProfile(String),
}

impl Activity {
    pub(super) fn profile(&self) -> Option<&str> {
        match self {
            Self::SavingProfile(profile) => Some(profile),
            _ => None,
        }
    }

    pub(super) fn label(&self) -> &'static str {
        match self {
            Self::Loading | Self::Starting => "Loading Java list…",
            Self::Discovering => "Discovering Java installations…",
            Self::ChoosingFile => "Choose a Java executable.",
            Self::SavingCatalog | Self::SavingProfile(_) => "Saving Java settings…",
        }
    }

    pub(super) fn writes(&self) -> bool {
        matches!(
            self,
            Self::Discovering | Self::SavingCatalog | Self::SavingProfile(_)
        )
    }
}

pub(super) enum Operation {
    Startup,
    Refresh,
    Discover,
    Add(PathBuf),
    Remove(PathBuf),
    Prefer {
        major: u32,
        path: PathBuf,
    },
    Select {
        profile: String,
        path: Option<PathBuf>,
    },
}

#[derive(Debug)]
pub(super) enum OperationError {
    LoadOrSave(String),
    RefreshAfterSave(String),
}

impl Operation {
    pub(super) fn activity(&self) -> Activity {
        match self {
            Self::Startup => Activity::Starting,
            Self::Refresh => Activity::Loading,
            Self::Discover => Activity::Discovering,
            Self::Select { profile, .. } => Activity::SavingProfile(profile.clone()),
            _ => Activity::SavingCatalog,
        }
    }

    pub(super) fn run(self, paths: &DataDir) -> Result<JavaCatalog, OperationError> {
        let change = match self {
            Self::Startup => return java::startup(paths).map_err(load_error),
            Self::Discover => return java::discover(paths).map_err(load_error),
            Self::Refresh => {
                return java::list(paths).map_err(load_error);
            }
            Self::Add(path) => java::add(paths, &path).map(|_| ()),
            Self::Remove(path) => java::remove(paths, &path).map(|_| ()),
            Self::Prefer { major, path } => java::set_preferred(paths, major, &path),
            Self::Select { profile, path } => {
                java::set_profile_preference(paths, &profile, path.as_deref())
            }
        };
        change.map_err(load_error)?;
        java::list(paths).map_err(|error| {
            OperationError::RefreshAfterSave(format!(
                "Java settings were saved. Reload the launcher data to see them. {error}"
            ))
        })
    }
}

fn load_error(error: java::JavaCatalogError) -> OperationError {
    OperationError::LoadOrSave(error.to_string())
}
