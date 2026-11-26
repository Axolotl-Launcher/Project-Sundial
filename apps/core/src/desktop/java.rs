// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Desktop policy for choosing and provisioning Java.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use axlauncher_install::{prepare::Progress, runtime};
use axlauncher_launch::{Java, JavaMode, JavaRuntime, SystemJava, Version, version};
use axlauncher_spi::Network;
use axlauncher_state::javas::{self, Javas, JavasError};
use axlauncher_storage::DataDir;

/// A recorded runtime outranks the newest discovered runtime for its major.
pub(super) fn managed(data: &DataDir) -> SystemJava {
    provider(data, &preferences(data.root()))
}

fn preferences(root: &Path) -> Javas {
    javas::load(root).unwrap_or_default()
}

pub(super) fn provider(data: &DataDir, javas: &Javas) -> SystemJava {
    SystemJava::from_environment()
        .with_runtime_roots(vec![data.java_versions()])
        .with_chosen(chosen(javas))
        .with_registered(javas.manual_paths().map(Path::to_path_buf).collect())
        .with_system_discovery()
}

fn chosen(javas: &Javas) -> Vec<(u32, PathBuf)> {
    javas
        .preferred()
        .map(|(major, path)| (major, path.to_owned()))
        .collect()
}

/// The available installations and the preferences saved on this machine.
#[derive(PartialEq, Eq)]
pub struct JavaCatalog {
    available: Vec<JavaRuntime>,
    defaults: BTreeMap<u32, PathBuf>,
    saved: Javas,
    profile_runtimes: BTreeMap<PathBuf, JavaRuntime>,
}

impl JavaCatalog {
    pub fn available(&self) -> &[JavaRuntime] {
        &self.available
    }

    /// The executable automatic selection would use for this major.
    pub fn default_for(&self, major: u32) -> Option<&Path> {
        self.defaults.get(&major).map(PathBuf::as_path)
    }

    /// Includes registrations whose files are currently unavailable.
    pub fn manual_paths(&self) -> impl Iterator<Item = &Path> {
        self.saved.manual_paths()
    }

    pub fn preferred(&self) -> impl Iterator<Item = (u32, &Path)> {
        self.saved.preferred()
    }

    pub fn profile_preference(&self, profile_id: &str) -> Option<&Path> {
        self.saved.profile_preference(profile_id)
    }

    /// Resolves a saved choice even when its path is no longer in the scan.
    pub fn profile_runtime(&self, profile_id: &str) -> Option<&JavaRuntime> {
        let path = self.profile_preference(profile_id)?;
        self.available
            .iter()
            .find(|runtime| runtime.path() == path)
            .or_else(|| self.profile_runtimes.get(path))
    }
}

#[derive(Debug)]
pub enum JavaCatalogError {
    State(JavasError),
    InvalidRuntime {
        path: PathBuf,
        required: Option<u32>,
    },
}

impl fmt::Display for JavaCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::State(error) => error.fmt(formatter),
            Self::InvalidRuntime {
                path,
                required: Some(major),
            } => {
                write!(
                    formatter,
                    "{} does not provide a usable Java {major}",
                    path.display()
                )
            }
            Self::InvalidRuntime {
                path,
                required: None,
            } => {
                write!(
                    formatter,
                    "{} is not a usable Java installation",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for JavaCatalogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::State(error) => Some(error),
            Self::InvalidRuntime { .. } => None,
        }
    }
}

impl From<JavasError> for JavaCatalogError {
    fn from(error: JavasError) -> Self {
        Self::State(error)
    }
}

/// Rescans local installations and includes saved manual entries and preferences.
pub fn list(data: &DataDir) -> Result<JavaCatalog, JavaCatalogError> {
    let saved = javas::load(data.root())?;
    let provider = provider(data, &saved);
    let available = provider.discover();
    let defaults = available
        .iter()
        .map(JavaRuntime::major)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|major| {
            Some((
                major,
                provider.default_for(major, &available)?.path().to_owned(),
            ))
        })
        .collect();
    let discovered: BTreeSet<_> = available.iter().map(JavaRuntime::path).collect();
    let profile_runtimes = saved
        .profile_paths()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|path| !discovered.contains(path))
        .filter_map(|path| Some((path.to_owned(), JavaRuntime::inspect(path)?)))
        .collect();
    Ok(JavaCatalog {
        available,
        defaults,
        saved,
        profile_runtimes,
    })
}

/// Registers a Java executable after checking its installation.
pub fn add(data: &DataDir, path: &Path) -> Result<JavaRuntime, JavaCatalogError> {
    let runtime = validate(path, None)?;
    javas::update(data.root(), |javas| {
        javas.add_manual(runtime.path().to_owned())
    })?;
    Ok(runtime)
}

/// Removes a manual catalog entry using its listed path.
pub fn remove(data: &DataDir, path: &Path) -> Result<bool, JavaCatalogError> {
    Ok(javas::update(data.root(), |javas| {
        javas.remove_manual(path)
    })?)
}

/// Pins a runtime for a Java major, or restores automatic selection for it.
pub fn set_preferred(
    data: &DataDir,
    major: u32,
    path: Option<&Path>,
) -> Result<(), JavaCatalogError> {
    let path = validated_path(path, Some(major))?;
    Ok(javas::update(data.root(), |javas| {
        javas.set_chosen(major, path)
    })?)
}

/// Pins a runtime for a local instance, or restores its automatic selection.
pub fn set_profile_preference(
    data: &DataDir,
    profile_id: &str,
    path: Option<&Path>,
) -> Result<(), JavaCatalogError> {
    let path = validated_path(path, None)?;
    Ok(javas::update(data.root(), |javas| {
        javas.set_profile_preference(profile_id, path)
    })?)
}

fn validated_path(
    path: Option<&Path>,
    required: Option<u32>,
) -> Result<Option<PathBuf>, JavaCatalogError> {
    path.map(|path| validate(path, required).map(|runtime| runtime.path().to_owned()))
        .transpose()
}

fn validate(path: &Path, required: Option<u32>) -> Result<JavaRuntime, JavaCatalogError> {
    JavaRuntime::inspect(path)
        .filter(|runtime| required.is_none_or(|major| runtime.major() == major))
        .ok_or_else(|| JavaCatalogError::InvalidRuntime {
            path: path.to_owned(),
            required,
        })
}

/// A manifest with no Java requirement uses the system's default at launch.
pub(super) async fn prepare_for_version(
    network: &impl Network,
    data: &DataDir,
    id: &str,
    report: impl FnMut(Progress),
) -> Result<(), String> {
    let document = version::load(&data.game().versions(), id)
        .map_err(|error| format!("Cannot read the installed Minecraft {id} manifest: {error}"))?;
    let Some(major) = missing_java(&document, |major| {
        managed(data)
            .executable_for(major, JavaMode::Windowed)
            .is_some()
    }) else {
        return Ok(());
    };
    provision(network, data, major, report)
        .await
        .map_err(|error| {
            format!(
                "Minecraft {id} needs Java {major}, and its runtime could not be installed: {error}"
            )
        })
}

fn missing_java(version: &Version, available: impl FnOnce(u32) -> bool) -> Option<u32> {
    let required = version.java_version.as_ref()?.major;
    (!available(required)).then_some(required)
}

/// Java's named phases differ from the game's, but its file counts do not.
pub(super) async fn provision(
    network: &impl Network,
    data: &DataDir,
    major: u32,
    mut report: impl FnMut(Progress),
) -> Result<(), String> {
    report(Progress::Java);
    runtime::install(network, &data.java_versions(), major, |stage| {
        report(match stage {
            Progress::Downloading { .. } => stage,
            _ => Progress::Java,
        });
    })
    .await
    .map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/src/desktop/java.rs"]
mod tests;
#[cfg(test)]
pub(super) use tests::test_runtime;
