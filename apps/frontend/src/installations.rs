// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Transient desktop installation state for the shared Minecraft files.

use std::path::{Path, PathBuf};

use axlauncher_core::install::prepare::Progress;
use axlauncher_core::loader::SUPPORTED_KINDS;
use axlauncher_core::spi::LoaderKind;
use axlauncher_core::state::profile::Game;
use axlauncher_ui::{AppContext, AsyncApp, Context, WeakEntity};
use tokio::sync::watch;

use crate::RootView;
pub(crate) use crate::game_install::InstallOptions;
use crate::game_install::{download_failure, download_vanilla, install_loader};

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

/// Whether a profile's loader names no loader, whatever its spelling.
///
/// A hand-edited or imported marker can spell the kind with other case or
/// surrounding space. Every action must agree on which profiles that describes,
/// or a profile is accepted in one place and refused in another.
pub(crate) fn is_vanilla(kind: &str) -> bool {
    kind.trim().eq_ignore_ascii_case("vanilla")
}

/// An original game version can be prepared without a loader installer.
pub(crate) fn vanilla_version(game: &Game) -> Option<&str> {
    if game
        .loader
        .as_ref()
        .is_some_and(|loader| !is_vanilla(&loader.kind))
    {
        return None;
    }
    let id = game.version_id.as_deref().unwrap_or(&game.minecraft);
    (id == game.minecraft).then_some(id)
}

pub(crate) fn start(
    root: PathBuf,
    id: String,
    options: InstallOptions,
    cx: &mut Context<'_, RootView>,
) {
    let (progress, mut updates) = watch::channel(Progress::Version);
    let version = id.clone();
    let work_root = root.clone();
    let task = cx.background_spawn(async move { install(&work_root, &version, options, progress) });
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
    options: InstallOptions,
    progress: watch::Sender<Progress>,
) -> Installation {
    // Reqwest needs Tokio's I/O driver. The caller runs this on a GPUI worker.
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())
        .and_then(|runtime| runtime.block_on(download_vanilla(root, id, options, &progress)));

    match result {
        Ok(prepared) if prepared.is_complete() => Installation::Complete,
        Ok(prepared) => Installation::Failed(download_failure(&prepared)),
        Err(error) => Installation::Failed(error),
    }
}

/// Whether this build has a provider for a profile's loader.
///
/// A kind the UI can show but not install must not offer an install that would
/// fail at the end of a download. The name is read through the SPI's own parser,
/// so a spelling the port accepts is not refused here.
pub(crate) fn supported_loader(kind: &str) -> bool {
    is_vanilla(kind) || LoaderKind::parse(kind).is_some_and(|kind| SUPPORTED_KINDS.contains(&kind))
}

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
