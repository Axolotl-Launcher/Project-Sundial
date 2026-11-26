// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Desktop orchestration for vanilla and mod-loader installations.

use std::path::Path;

use axlauncher_install::{
    loader::{self, LoaderInstallError, LoaderInstalled},
    prepare::{Prepared, Progress},
    vanilla,
};
use axlauncher_launch::Target;
use axlauncher_spi::{LoaderKind, LoaderRequest, ModLoader, Network};
use axlauncher_state::profile::{self, Game};
use axlauncher_storage::DataDir;

use super::{java, on_worker, sources::DownloadOptions};

/// What a profile asks to install, independent of the version ID produced by a loader.
pub struct InstallTarget {
    game_version: String,
    loader: Option<LoaderSelection>,
}

struct LoaderSelection {
    kind: LoaderKind,
    version: Option<String>,
}

impl InstallTarget {
    /// Shared installations exclude isolated runtimes and unsupported loaders.
    pub fn for_game(game: &Game) -> Option<Self> {
        if game.is_runtime_isolated() {
            return None;
        }
        let game_version = game.minecraft.clone();
        if game.is_vanilla() {
            return Some(Self {
                game_version,
                loader: None,
            });
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
        Some(Self {
            game_version,
            loader: Some(LoaderSelection { kind, version }),
        })
    }

    /// A loader slot follows the requested game, loader kind and selected build.
    pub fn key(&self) -> String {
        let Some(loader) = &self.loader else {
            return self.game_version.clone();
        };
        let key = format!("{}:{}", self.game_version, loader.kind);
        match &loader.version {
            Some(version) => format!("{key}:{version}"),
            None => key,
        }
    }

    pub fn is_loader(&self) -> bool {
        self.loader.is_some()
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
    on_worker(async {
        if let Some(selection) = target.loader {
            return download_loader(data, target.game_version, selection, options, report).await;
        }
        download_vanilla(data, &target.game_version, options, report).await?;
        Ok(target.game_version)
    })
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

async fn download_vanilla(
    data: &DataDir,
    id: &str,
    options: DownloadOptions,
    mut report: impl FnMut(Progress),
) -> Result<(), String> {
    let concurrency = options.concurrency();
    let network = options.network()?;
    let prepared = vanilla::install(
        &network,
        &data.game(),
        id,
        &Target::host(),
        concurrency,
        &mut report,
    )
    .await?;
    ensure_complete(&prepared)?;
    java::prepare_for_version(&network, data, id, report).await
}

async fn download_loader(
    data: &DataDir,
    game_version: String,
    selection: LoaderSelection,
    options: DownloadOptions,
    report: impl FnMut(Progress),
) -> Result<String, String> {
    let concurrency = options.concurrency();
    let network = options.network()?;
    let provider = axlauncher_loader::provider(network.clone(), selection.kind)
        .map_err(|error| error.to_string())?;
    let request = LoaderRequest {
        game_version,
        loader_version: selection.version,
    };
    let installed = install_loader(data, &network, &provider, request, concurrency, report).await?;
    ensure_complete(&installed.prepared)?;
    Ok(installed.version_id)
}

async fn install_loader(
    data: &DataDir,
    network: &impl Network,
    provider: &impl ModLoader,
    request: LoaderRequest,
    concurrency: usize,
    mut report: impl FnMut(Progress),
) -> Result<LoaderInstalled, String> {
    let initial_java = java::managed(data);
    let operation = loader::InstallRequest {
        network,
        loader: provider,
        java: &initial_java,
        game_root: &data.game(),
        request,
        target: &Target::host(),
        concurrency,
    };
    let required = match operation.install(&mut report).await {
        Err(LoaderInstallError::Java { required }) => required,
        result => return result.map_err(|error| error.to_string()),
    };

    java::provision(network, data, required, &mut report).await?;
    // SystemJava takes its inventory at construction, before this runtime existed.
    let refreshed_java = java::managed(data);
    loader::InstallRequest {
        java: &refreshed_java,
        ..operation
    }
    .install(report)
    .await
    .map_err(|error| error.to_string())
}

fn ensure_complete(prepared: &Prepared) -> Result<(), String> {
    match prepared.failures.first() {
        None => Ok(()),
        Some(first) => Err(format!(
            "Could not prepare {} file(s). {first}",
            prepared.failures.len()
        )),
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/src/desktop/installation.rs"]
mod tests;
