// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Desktop task and view state for a Minecraft process.

use std::path::{Path, PathBuf};

use axlauncher_core::desktop::{
    launching::{LaunchFailure, LaunchJob, Player},
    sources::DownloadOptions,
};
use axlauncher_ui::{AppContext, AsyncApp, Context, WeakEntity};
use tokio::sync::{oneshot, watch};

use crate::RootView;

pub(crate) enum Launch {
    Starting,
    Running,
    Exited,
    NeedsInstall(String),
    Failed(String),
}

impl Launch {
    pub(crate) fn is_active(&self) -> bool {
        matches!(self, Self::Starting | Self::Running)
    }

    pub(crate) fn message(&self) -> &str {
        match self {
            Self::Starting => "Checking files and starting Minecraft",
            Self::Running => "Minecraft running",
            Self::Exited => "Minecraft closed",
            Self::NeedsInstall(error) | Self::Failed(error) => error,
        }
    }
}

pub(crate) fn start(
    job: LaunchJob,
    player: Player,
    options: DownloadOptions,
    cx: &mut Context<'_, RootView>,
) {
    let root = job.game_root().to_path_buf();
    let directory = job.instance_directory().to_path_buf();
    let version = job.version_id().to_owned();
    let (sender, mut started) = watch::channel(false);
    let (settled, account) = oneshot::channel();
    let task = cx.background_spawn(async move {
        job.run(
            player,
            options,
            |service| {
                let _ = settled.send(service);
            },
            move || {
                sender.send_replace(true);
            },
        )
    });
    cx.spawn(async move |view, cx| {
        if let Ok(service) = account.await {
            let _ = view.update(cx, |view, cx| view.sync_launch_accounts(&service, cx));
        }
        if started.changed().await.is_ok() {
            show_running(&view, &directory, &version, cx);
        }
        let outcome = task.await;
        let _ = view.update(cx, |view, cx| {
            finish_launch(view, &root, directory, &version, outcome, cx)
        });
    })
    .detach();
}

fn show_running(view: &WeakEntity<RootView>, directory: &Path, version: &str, cx: &mut AsyncApp) {
    let _ = view.update(cx, |view, cx| {
        view.set_launch(directory, version, Launch::Running, cx)
    });
}

fn finish_launch(
    view: &mut RootView,
    root: &Path,
    directory: PathBuf,
    version: &str,
    outcome: Result<(), LaunchFailure>,
    cx: &mut Context<'_, RootView>,
) {
    let launch = match outcome {
        Ok(()) => Launch::Exited,
        Err(LaunchFailure::NeedsInstall(reason)) => {
            view.clear_completed_installation(root, &directory);
            Launch::NeedsInstall(reason)
        }
        Err(LaunchFailure::Failed(reason)) => Launch::Failed(reason),
    };
    view.set_launch(&directory, version, launch, cx);
}
