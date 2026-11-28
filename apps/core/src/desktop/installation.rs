// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Desktop orchestration for vanilla and mod-loader installations.

use std::path::Path;

use axlauncher_install::prepare::Progress;
use axlauncher_spi::LoaderKind;
use axlauncher_state::profile::{self, Game};
use axlauncher_storage::DataDir;
use axlauncher_worker::{Cancellation, installation as worker};

use super::{java, on_worker, sources::DownloadOptions};

/// What a profile asks to install, independent of the version ID produced by a loader.
pub struct InstallTarget(worker::Target);

impl InstallTarget {
    /// Shared installations exclude isolated runtimes and unsupported loaders.
    pub fn for_game(game: &Game) -> Option<Self> {
        if game.is_runtime_isolated() {
            return None;
        }
        let game_version = game.minecraft.clone();
        if game.is_vanilla() {
            return Some(Self(worker::Target::new(game_version, None)));
        }
        let loader = game.loader.as_ref()?;
        let kind = LoaderKind::parse(&loader.kind)?;
        if !axlauncher_loader::supported_kinds().any(|supported| supported == kind) {
            return None;
        }
        let version = loader
            .version
            .as_deref()
            .map(str::trim)
            .filter(|version| !version.is_empty())
            .map(str::to_owned);
        Some(Self(worker::Target::new(
            game_version,
            Some((kind, version)),
        )))
    }

    /// A loader slot follows the requested game, loader kind and selected build.
    pub fn key(&self) -> String {
        let minecraft = self.0.game_version();
        let Some((kind, build)) = self.0.loader() else {
            return minecraft.to_owned();
        };
        let key = format!("{minecraft}:{kind}");
        match build {
            Some(version) => format!("{key}:{version}"),
            None => key,
        }
    }

    pub fn is_loader(&self) -> bool {
        self.0.loader().is_some()
    }
}

/// An install slot for the front end, if the profile is installable.
pub fn install_key(game: &Game) -> Option<String> {
    Some(InstallTarget::for_game(game)?.key())
}

/// A completed loader install returns its recorded version; vanilla files are shared.
/// Call from a worker so downloads and profile writes stay off the UI thread.
pub fn install(
    data: &DataDir,
    directory: &Path,
    target: InstallTarget,
    options: DownloadOptions,
    report: impl FnMut(Progress),
) -> Result<Option<String>, String> {
    let is_loader = target.is_loader();
    let installed = prepare(data, target, options, report);
    if is_loader {
        persist_loader_result(directory, installed).map(Some)
    } else {
        installed.map(|_| None)
    }
}

/// The version ready in the shared game directory, without writing a profile.
/// This also serves staged modpacks that cannot publish until all files land.
pub fn prepare(
    data: &DataDir,
    target: InstallTarget,
    options: DownloadOptions,
    report: impl FnMut(Progress),
) -> Result<String, String> {
    let command = worker::Command::new(
        data,
        target.0,
        options.mirrors(),
        options.concurrency(),
        java::install_snapshot(data),
    );
    on_worker(worker::install(
        data,
        command,
        options.downloader(data),
        Cancellation::default(),
        |path, major| java::register_provisioned(data, &path, major),
        report,
    ))
    .map_err(|error| error.to_string())?
}

fn persist_loader_result(
    directory: &Path,
    installed: Result<String, String>,
) -> Result<String, String> {
    let recorded = installed.and_then(|version| {
        profile::mark_installed(directory, &version).map_err(|error| error.to_string())?;
        Ok(version)
    });
    if recorded.is_err() {
        let _ = profile::mark_failed(directory);
    }
    recorded
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/src/desktop/installation.rs"]
mod tests;
