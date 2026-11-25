// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The install service's progress and outcome as seen by the desktop.

use std::path::{Path, PathBuf};

use axlauncher_core::desktop::{
    installation::{self, InstallTarget},
    sources::DownloadOptions,
};
use axlauncher_core::install::prepare::Progress;
use axlauncher_core::storage::DataDir;
use axlauncher_ui::{AppContext, AsyncApp, Context, WeakEntity};
use tokio::sync::watch;

use crate::RootView;
use crate::installation_progress::{progress_message, should_report};

pub(crate) enum Installation {
    Running(Progress),
    Complete,
    Failed(String),
}

impl Installation {
    pub(crate) fn message(&self) -> String {
        match self {
            Self::Running(stage) => progress_message(*stage),
            Self::Complete => "Game files ready".into(),
            Self::Failed(message) => message.clone(),
        }
    }
}

pub(crate) fn start(
    data_root: PathBuf,
    directory: PathBuf,
    target: InstallTarget,
    options: DownloadOptions,
    cx: &mut Context<'_, RootView>,
) {
    let game_root = DataDir::new(&data_root).meta();
    let key = target.key();
    let is_loader = target.is_loader();
    let (progress, mut updates) = watch::channel(Progress::Version);
    let task_root = data_root.clone();
    let task_directory = directory.clone();
    let task = cx.background_spawn(async move {
        installation::install(&task_root, &task_directory, target, options, |stage| {
            report_progress(&progress, stage);
        })
    });
    cx.spawn(async move |view, cx| {
        display_progress(&mut updates, &view, cx, &game_root, &key).await;
        let outcome = task.await;
        let _ = view.update(cx, |view, cx| {
            view.finish_installation(&data_root, &directory, &key, is_loader, outcome, cx)
        });
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

fn report_progress(progress: &watch::Sender<Progress>, stage: Progress) {
    if should_report(stage) {
        progress.send_replace(stage);
    }
}

#[cfg(test)]
#[path = "../../../tests/apps/frontend/src/installations.rs"]
mod tests;
