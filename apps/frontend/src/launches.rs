// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Desktop task and view state for a Minecraft process.
//!
//! The module starts the launch and hands the view what it observes. Presenting
//! the result belongs to the panel that owns the row.

use std::path::PathBuf;
use std::sync::Arc;

use axlauncher_core::desktop::{
    accounts::DesktopAccounts,
    launching::{LaunchFailure, LaunchJob, Player},
    sources::DownloadOptions,
};
use axlauncher_ui::{AppContext, Task};
use tokio::sync::{oneshot, watch};

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
    cx: &mut impl AppContext,
) -> LaunchTask {
    let game_root = job.game_root().to_path_buf();
    let directory = job.instance_directory().to_path_buf();
    let version = job.version_id().to_owned();
    let (sender, started) = watch::channel(false);
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
    LaunchTask {
        game_root,
        directory,
        version,
        started,
        account,
        task,
    }
}

/// A running launch: where it was started, what it reports, and its outcome.
pub(crate) struct LaunchTask {
    pub(crate) game_root: PathBuf,
    pub(crate) directory: PathBuf,
    pub(crate) version: String,
    pub(crate) started: watch::Receiver<bool>,
    pub(crate) account: oneshot::Receiver<Arc<DesktopAccounts>>,
    pub(crate) task: Task<Result<(), LaunchFailure>>,
}
