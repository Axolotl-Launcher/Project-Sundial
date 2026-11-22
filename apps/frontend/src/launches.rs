// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The desktop lifecycle of a Minecraft process.

use std::path::{Path, PathBuf};

use axlauncher_core::install::vanilla;
use axlauncher_core::launch::{FlowError, LaunchRequest, ProcessLauncher, Target, flow};
use axlauncher_ui::{AppContext, AsyncApp, Context, WeakEntity};
use tokio::sync::watch;

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
    request: LaunchRequest,
    vanilla: bool,
    data_root: PathBuf,
    cx: &mut Context<'_, RootView>,
) {
    let root = request.game_root.clone();
    let directory = request.instance_directory.clone();
    let version = request.version_id.clone();
    let (sender, mut started) = watch::channel(false);
    let task = cx.background_spawn(async move {
        match tokio::runtime::Builder::new_current_thread().build() {
            Ok(runtime) => runtime.block_on(run(request, vanilla, &data_root, sender)),
            Err(error) => Launch::Failed(error.to_string()),
        }
    });
    cx.spawn(async move |view, cx| {
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
    outcome: Launch,
    cx: &mut Context<'_, RootView>,
) {
    if matches!(outcome, Launch::NeedsInstall(_)) {
        view.clear_completed_installation(root, &directory);
    }
    view.set_launch(&directory, version, outcome, cx);
}

async fn run(
    request: LaunchRequest,
    vanilla: bool,
    data_root: &Path,
    sender: watch::Sender<bool>,
) -> Launch {
    if vanilla
        && let Err(error) =
            vanilla::check_files(&request.game_root, &request.version_id, &Target::host())
    {
        return Launch::NeedsInstall(format!("Install game files to continue. {error}"));
    }
    let launcher = ProcessLauncher::reporting(move || {
        sender.send_replace(true);
    });
    match flow::launch(&crate::java::managed(data_root), &launcher, request).await {
        Ok(_) => Launch::Exited,
        Err(FlowError::NoMatchingJava { required, .. }) => Launch::Failed(format!(
            "Java {required} is required. Set JAVA_HOME or PATH to a Java {required} installation and reopen the launcher."
        )),
        Err(error) => Launch::Failed(error.to_string()),
    }
}
