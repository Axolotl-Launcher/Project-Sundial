// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The install service's progress and outcome as seen by the desktop.

use std::path::PathBuf;

use axlauncher_core::desktop::{
    installation::{self, InstallTarget},
    sources::DownloadOptions,
};
use axlauncher_core::install::prepare::Progress;
use axlauncher_core::storage::DataDir;
use axlauncher_ui::{AppContext, AsyncApp, Context, WeakEntity};
use tokio::sync::watch;

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

pub(crate) enum InstallEvent {
    Progress(Progress),
    Finished(Result<Option<String>, String>),
}

pub(crate) fn start<T: 'static>(
    paths: DataDir,
    directory: PathBuf,
    target: InstallTarget,
    options: DownloadOptions,
    cx: &mut Context<'_, T>,
    on_event: impl Fn(&mut T, InstallEvent, &mut Context<'_, T>) + 'static,
) {
    let (progress, mut updates) = watch::channel(Progress::Version);
    let task = cx.background_spawn(async move {
        installation::install(&paths, &directory, target, options, |stage| {
            report_progress(&progress, stage);
        })
    });
    cx.spawn(async move |view, cx| {
        stream_progress(&mut updates, &view, cx, &on_event).await;
        let outcome = task.await;
        let _ = view.update(cx, |view, cx| {
            on_event(view, InstallEvent::Finished(outcome), cx)
        });
    })
    .detach();
}

async fn stream_progress<T: 'static>(
    updates: &mut watch::Receiver<Progress>,
    view: &WeakEntity<T>,
    cx: &mut AsyncApp,
    on_event: &impl Fn(&mut T, InstallEvent, &mut Context<'_, T>),
) {
    while updates.changed().await.is_ok() {
        let progress = *updates.borrow_and_update();
        let updated = view.update(cx, |view, cx| {
            on_event(view, InstallEvent::Progress(progress), cx)
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
