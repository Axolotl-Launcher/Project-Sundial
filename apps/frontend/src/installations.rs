// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The install service's progress and outcome as seen by the desktop.
//!
//! The module starts the background work and hands the view its progress and
//! result. Presenting them belongs to the panel that owns the row.

use std::path::PathBuf;

use axlauncher_core::desktop::{
    installation::{self, InstallTarget},
    sources::DownloadOptions,
};
use axlauncher_core::install::prepare::Progress;
use axlauncher_core::storage::DataDir;
use axlauncher_ui::{AppContext, Task};
use tokio::sync::watch;

use crate::i18n::tr;

use crate::installation_progress::{progress_message, should_report};

pub(crate) enum Installation {
    Running(Progress),
    Complete,
    Failed(String),
}

impl Installation {
    pub(crate) fn message(&self, cx: &axlauncher_ui::App) -> String {
        match self {
            Self::Running(stage) => progress_message(*stage, cx),
            Self::Complete => tr(cx, "install.ready").into(),
            Self::Failed(message) => message.clone(),
        }
    }
}

pub(crate) fn start(
    paths: DataDir,
    directory: PathBuf,
    target: InstallTarget,
    options: DownloadOptions,
    cx: &mut impl AppContext,
) -> InstallationJob {
    let key = target.key();
    let is_loader = target.is_loader();
    let game_root = paths.game().root().to_path_buf();
    let (progress, updates) = watch::channel(Progress::Version);
    let task_paths = paths.clone();
    let task_directory = directory.clone();
    let task = cx.background_spawn(async move {
        installation::install(&task_paths, &task_directory, target, options, |stage| {
            report_progress(&progress, stage);
        })
    });
    InstallationJob {
        key,
        directory,
        is_loader,
        paths,
        game_root,
        updates,
        task,
    }
}

/// A running install: the key its row watches, its progress, and its result.
pub(crate) struct InstallationJob {
    pub(crate) key: String,
    pub(crate) directory: PathBuf,
    pub(crate) is_loader: bool,
    pub(crate) paths: DataDir,
    pub(crate) game_root: PathBuf,
    pub(crate) updates: watch::Receiver<Progress>,
    pub(crate) task: Task<Result<Option<String>, String>>,
}

fn report_progress(progress: &watch::Sender<Progress>, stage: Progress) {
    if should_report(stage) {
        progress.send_replace(stage);
    }
}

#[cfg(test)]
#[path = "../../../tests/apps/frontend/src/installations.rs"]
mod tests;
