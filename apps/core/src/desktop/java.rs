// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Desktop policy for choosing and provisioning Java.

use std::path::{Path, PathBuf};

use axlauncher_install::{prepare::Progress, runtime};
use axlauncher_launch::{Java, JavaMode, SystemJava, Version, version};
use axlauncher_spi::Network;
use axlauncher_state::javas::{self, ChosenJava};
use axlauncher_storage::DataDir;

/// The environment fallback and the choices recorded for the current data root.
#[derive(Default)]
pub struct JavaEnvironment {
    java_home: Option<PathBuf>,
    default_java: Option<PathBuf>,
    chosen: Vec<(u32, PathBuf)>,
}

impl JavaEnvironment {
    pub fn load(root: Option<&Path>) -> Self {
        let provider = SystemJava::from_environment();
        Self {
            java_home: provider.java_home().map(Path::to_path_buf),
            default_java: provider.executable(None, JavaMode::Windowed),
            chosen: root.map(chosen).unwrap_or_default(),
        }
    }

    pub fn java_home(&self) -> Option<&Path> {
        self.java_home.as_deref()
    }

    pub fn default_java(&self) -> Option<&Path> {
        self.default_java.as_deref()
    }

    pub fn chosen(&self) -> &[(u32, PathBuf)] {
        &self.chosen
    }
}

/// A recorded runtime outranks managed discovery and the environment for its major.
pub(super) fn managed(data: &DataDir) -> SystemJava {
    SystemJava::from_environment()
        .with_runtime_roots(vec![data.java_versions()])
        .with_chosen(chosen(data.root()))
}

fn chosen(root: &Path) -> Vec<(u32, PathBuf)> {
    let Ok(loaded) = javas::load(root) else {
        return Vec::new();
    };
    loaded
        .javas
        .chosen()
        .iter()
        .map(|runtime| (runtime.major_version, PathBuf::from(&runtime.path)))
        .collect()
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
    let installed = runtime::install(network, &data.java_versions(), major, |stage| {
        report(match stage {
            Progress::Downloading { .. } => stage,
            _ => Progress::Java,
        });
    })
    .await
    .map_err(|error| error.to_string())?;
    let _ = javas::choose_if_unset(
        data.root(),
        ChosenJava {
            major_version: installed.major,
            full_version: installed.full_version,
            architecture: std::env::consts::ARCH.to_string(),
            path: installed.java.display().to_string(),
        },
    );
    Ok(())
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/src/desktop/java.rs"]
mod tests;
