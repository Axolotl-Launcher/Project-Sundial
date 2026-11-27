// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Desktop task and view state for a Minecraft process.

use std::sync::Arc;

use axlauncher_core::desktop::{
    accounts::DesktopAccounts,
    launching::{LaunchFailure, LaunchJob, Player},
    sources::DownloadOptions,
};
use axlauncher_ui::{AppContext, AsyncApp, Context, WeakEntity};
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

pub(crate) enum LaunchEvent {
    AccountResolved(Arc<DesktopAccounts>),
    Status(Launch),
}

pub(crate) fn start<T: 'static>(
    job: LaunchJob,
    player: Player,
    options: DownloadOptions,
    cx: &mut Context<'_, T>,
    on_event: impl Fn(&mut T, LaunchEvent, &mut Context<'_, T>) + 'static,
) {
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
            notify(&view, cx, &on_event, LaunchEvent::AccountResolved(service));
        }
        if started.changed().await.is_ok() {
            notify(&view, cx, &on_event, LaunchEvent::Status(Launch::Running));
        }
        let status = match task.await {
            Ok(()) => Launch::Exited,
            Err(LaunchFailure::NeedsInstall(reason)) => Launch::NeedsInstall(reason),
            Err(LaunchFailure::Failed(reason)) => Launch::Failed(reason),
        };
        notify(&view, cx, &on_event, LaunchEvent::Status(status));
    })
    .detach();
}

fn notify<T: 'static>(
    view: &WeakEntity<T>,
    cx: &mut AsyncApp,
    on_event: &impl Fn(&mut T, LaunchEvent, &mut Context<'_, T>),
    event: LaunchEvent,
) {
    let _ = view.update(cx, |view, cx| on_event(view, event, cx));
}
