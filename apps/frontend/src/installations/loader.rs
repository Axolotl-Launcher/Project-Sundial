// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The loader install path: resolve a profile's loader, run its provider, and
//! fetch the runtime a loader build asks for before retrying.

use std::path::{Path, PathBuf};

use axlauncher_core::install::{loader as loader_install, prepare::Progress, runtime};
use axlauncher_core::launch::{SystemJava, Target};
use axlauncher_core::loader::{ForgeApi, LoaderApi, NeoForgeApi};
use axlauncher_core::network::ReqwestNetwork;
use axlauncher_core::spi::{LoaderKind, LoaderRequest};
use axlauncher_core::state::profile::Game;
use axlauncher_ui::{AppContext, Context};
use tokio::sync::watch;

use super::{InstallOptions, display_progress, download_failure, record_runtime, report_progress};
use crate::RootView;

/// Starts installing a loader profile.
///
/// Progress is reported under `key`, the identity the row looks the install up
/// by, because a loader profile has no version id until this install produces
/// one. The id is returned on completion, and the caller records it on the
/// profile.
pub(crate) fn start_loader(
    data_root: PathBuf,
    game_root: PathBuf,
    directory: PathBuf,
    game: Game,
    key: String,
    options: InstallOptions,
    cx: &mut Context<'_, RootView>,
) {
    let (progress, mut updates) = watch::channel(Progress::Version);
    let display_key = key.clone();
    let display_root = game_root.clone();
    let work_data = data_root.clone();
    let task = cx.background_spawn(async move {
        install_loader(&work_data, &game_root, &game, options, progress)
    });
    cx.spawn(async move |view, cx| {
        display_progress(&mut updates, &view, cx, &display_root, &display_key).await;
        let outcome = task.await;
        let _ = view.update(cx, |view, cx| {
            view.finish_loader_install(&data_root, &directory, &key, outcome, cx)
        });
    })
    .detach();
}

fn install_loader(
    data_root: &Path,
    game_root: &Path,
    game: &Game,
    options: InstallOptions,
    progress: watch::Sender<Progress>,
) -> Result<String, String> {
    let kind = game
        .loader
        .as_ref()
        .and_then(|loader| LoaderKind::parse(&loader.kind))
        .ok_or_else(|| "This profile names no loader to install.".to_string())?;
    let request = LoaderRequest {
        game_version: game.minecraft.clone(),
        loader_version: game
            .loader
            .as_ref()
            .and_then(|loader| loader.version.clone()),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(download_loader(
        data_root, game_root, kind, request, options, progress,
    ))
}

async fn download_loader(
    data_root: &Path,
    game_root: &Path,
    kind: LoaderKind,
    request: LoaderRequest,
    options: InstallOptions,
    progress: watch::Sender<Progress>,
) -> Result<String, String> {
    let network =
        ReqwestNetwork::desktop_with_mirrors(options.mirrors).map_err(|error| error.to_string())?;
    let concurrency = options.concurrency;

    let first = run_provider(
        kind,
        &network,
        &crate::java::managed(data_root),
        game_root,
        request.clone(),
        concurrency,
        &progress,
    )
    .await;

    let installed = match first {
        Err(loader_install::LoaderInstallError::Java { required }) => {
            let installed = runtime::install(
                &network,
                &axlauncher_core::storage::DataDir::new(data_root).java_versions(),
                required,
                |stage| report_progress(&progress, stage),
            )
            .await
            .map_err(|error| error.to_string())?;
            record_runtime(data_root, &installed);
            run_provider(
                kind,
                &network,
                &crate::java::managed(data_root),
                game_root,
                request,
                concurrency,
                &progress,
            )
            .await
            .map_err(|error| error.to_string())?
        }
        other => other.map_err(|error| error.to_string())?,
    };

    if installed.prepared.is_complete() {
        Ok(installed.version_id)
    } else {
        Err(download_failure(&installed.prepared))
    }
}

/// Each provider is its own type, so the match is over concrete values rather
/// than a trait object: the port is generic over the network provider.
async fn run_provider(
    kind: LoaderKind,
    network: &ReqwestNetwork,
    java: &SystemJava,
    game_root: &Path,
    request: LoaderRequest,
    concurrency: usize,
    progress: &watch::Sender<Progress>,
) -> Result<loader_install::LoaderInstalled, loader_install::LoaderInstallError> {
    let api = match kind {
        LoaderKind::Fabric => LoaderApi::fabric(network.clone()),
        LoaderKind::LegacyFabric => LoaderApi::legacy_fabric(network.clone()),
        LoaderKind::Quilt => LoaderApi::quilt(network.clone()),
        LoaderKind::Forge => {
            let api = ForgeApi::new(network.clone());
            return install_with(
                network,
                java,
                &api,
                game_root,
                request,
                concurrency,
                progress,
            )
            .await;
        }
        LoaderKind::NeoForge => {
            let api = NeoForgeApi::new(network.clone());
            return install_with(
                network,
                java,
                &api,
                game_root,
                request,
                concurrency,
                progress,
            )
            .await;
        }
        other => {
            return Err(loader_install::LoaderInstallError::Vanilla(format!(
                "No provider for loader {other} is wired yet."
            )));
        }
    };
    install_with(
        network,
        java,
        &api,
        game_root,
        request,
        concurrency,
        progress,
    )
    .await
}

async fn install_with(
    network: &ReqwestNetwork,
    java: &SystemJava,
    loader: &impl axlauncher_core::spi::ModLoader,
    game_root: &Path,
    request: LoaderRequest,
    concurrency: usize,
    progress: &watch::Sender<Progress>,
) -> Result<loader_install::LoaderInstalled, loader_install::LoaderInstallError> {
    loader_install::InstallRequest {
        network,
        loader,
        java,
        game_root,
        request,
        target: &Target::host(),
        concurrency,
    }
    .install(|stage| report_progress(progress, stage))
    .await
}
