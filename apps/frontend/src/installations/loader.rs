// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Connects the shared loader installer to the instance panel.

use std::path::PathBuf;

use axlauncher_core::install::prepare::Progress;
use axlauncher_core::state::profile::Game;
use axlauncher_ui::{AppContext, Context};
use tokio::sync::watch;

use super::{InstallOptions, display_progress};
use crate::RootView;
use crate::game_install::install_loader;

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
        install_loader(&work_data, &game_root, &game, options, |stage| {
            progress.send_replace(stage);
        })
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
