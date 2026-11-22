// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Transient desktop installation state for the shared Minecraft files.

use std::path::{Path, PathBuf};

use axlauncher_core::install::{
    loader as loader_install,
    prepare::{Prepared, Progress},
    vanilla,
};
use axlauncher_core::launch::Target;
use axlauncher_core::loader::LoaderApi;
use axlauncher_core::network::ReqwestNetwork;
use axlauncher_core::spi::{LoaderKind, LoaderRequest};
use axlauncher_core::state::profile::Game;
use axlauncher_ui::{AppContext, AsyncApp, Context, WeakEntity};
use tokio::sync::watch;

use crate::RootView;

pub(crate) enum Installation {
    Running(Progress),
    Complete,
    Failed(String),
}

impl Installation {
    pub(crate) fn message(&self) -> String {
        match self {
            Self::Running(Progress::Version) => "Getting the version manifest".into(),
            Self::Running(Progress::Loader) => "Resolving the mod loader".into(),
            Self::Running(Progress::CheckingFiles) => "Checking local game files".into(),
            Self::Running(Progress::Downloading { processed, total }) => {
                format!("Processing downloads ({processed}/{total} files)")
            }
            Self::Running(Progress::Finishing) => "Finishing installation".into(),
            Self::Complete => "Game files ready".into(),
            Self::Failed(message) => message.clone(),
        }
    }
}

/// An original game version can be prepared without a loader installer.
pub(crate) fn vanilla_version(game: &Game) -> Option<&str> {
    if game
        .loader
        .as_ref()
        .is_some_and(|loader| loader.kind != "vanilla")
    {
        return None;
    }
    let id = game.version_id.as_deref().unwrap_or(&game.minecraft);
    (id == game.minecraft).then_some(id)
}

pub(crate) fn start(root: PathBuf, id: String, concurrency: usize, cx: &mut Context<'_, RootView>) {
    let (progress, mut updates) = watch::channel(Progress::Version);
    let version = id.clone();
    let work_root = root.clone();
    let task =
        cx.background_spawn(async move { install(&work_root, &version, concurrency, progress) });
    cx.spawn(async move |view, cx| {
        display_progress(&mut updates, &view, cx, &root, &id).await;
        let result = task.await;
        let _ = view.update(cx, |view, cx| view.set_installation(&root, &id, result, cx));
    })
    .detach();
}

async fn display_progress(
    updates: &mut watch::Receiver<Progress>,
    view: &WeakEntity<RootView>,
    cx: &mut AsyncApp,
    root: &Path,
    id: &str,
) {
    while updates.changed().await.is_ok() {
        let progress = *updates.borrow_and_update();
        let updated = view.update(cx, |view, cx| {
            view.set_installation(root, id, Installation::Running(progress), cx)
        });
        if updated.is_err() {
            return;
        }
    }
}

fn install(
    root: &Path,
    id: &str,
    concurrency: usize,
    progress: watch::Sender<Progress>,
) -> Installation {
    // Reqwest needs Tokio's I/O driver. The caller runs this on a GPUI worker.
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())
        .and_then(|runtime| runtime.block_on(download(root, id, concurrency, &progress)));

    match result {
        Ok(prepared) if prepared.is_complete() => Installation::Complete,
        Ok(prepared) => Installation::Failed(download_failure(&prepared)),
        Err(error) => Installation::Failed(error),
    }
}

async fn download(
    root: &Path,
    id: &str,
    concurrency: usize,
    progress: &watch::Sender<Progress>,
) -> Result<Prepared, String> {
    let network = ReqwestNetwork::desktop().map_err(|error| error.to_string())?;
    vanilla::install(&network, root, id, &Target::host(), concurrency, |stage| {
        if let Progress::Downloading { processed, total } = stage
            && processed != total
            && !processed.is_multiple_of((total / 100).max(1))
        {
            return;
        }
        progress.send_replace(stage);
    })
    .await
}

fn download_failure(prepared: &Prepared) -> String {
    format!(
        "Could not prepare {} file(s). {}",
        prepared.failures.len(),
        prepared.failures[0]
    )
}

/// Whether this build has a provider for a profile's loader.
///
/// A kind the UI can show but not install must not offer an install that would
/// fail at the end of a download.
pub(crate) fn supported_loader(kind: &str) -> bool {
    matches!(kind, "vanilla" | "fabric" | "quilt")
}

/// Starts installing a loader profile.
///
/// The version to display progress under is the Minecraft version, because a
/// loader profile has no version id until this install produces one. The id is
/// returned on completion, and the caller records it on the profile.
pub(crate) fn start_loader(
    data_root: PathBuf,
    game_root: PathBuf,
    directory: PathBuf,
    game: Game,
    concurrency: usize,
    cx: &mut Context<'_, RootView>,
) {
    let key = game.minecraft.clone();
    let (progress, mut updates) = watch::channel(Progress::Version);
    let display_key = key.clone();
    let display_root = game_root.clone();
    let task = cx
        .background_spawn(async move { install_loader(&game_root, &game, concurrency, progress) });
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
    game_root: &Path,
    game: &Game,
    concurrency: usize,
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
        game_root,
        kind,
        request,
        concurrency,
        progress,
    ))
}

async fn download_loader(
    game_root: &Path,
    kind: LoaderKind,
    request: LoaderRequest,
    concurrency: usize,
    progress: watch::Sender<Progress>,
) -> Result<String, String> {
    let network = ReqwestNetwork::desktop().map_err(|error| error.to_string())?;
    let api = match kind {
        LoaderKind::Fabric => LoaderApi::fabric(network.clone()),
        LoaderKind::Quilt => LoaderApi::quilt(network.clone()),
        other => return Err(format!("No provider for loader {other} is wired yet.")),
    };
    let installed = loader_install::install(
        &network,
        &api,
        game_root,
        request,
        &Target::host(),
        concurrency,
        |stage| {
            let _ = progress.send(stage);
        },
    )
    .await
    .map_err(|error| error.to_string())?;

    if installed.prepared.is_complete() {
        Ok(installed.version_id)
    } else {
        Err(download_failure(&installed.prepared))
    }
}
