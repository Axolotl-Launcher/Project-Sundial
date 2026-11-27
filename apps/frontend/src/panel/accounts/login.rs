// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Runs sign-in attempts off the GPUI thread and routes browser actions to it.

use std::sync::Arc;

use axlauncher_core::desktop::accounts::{AccountId, Action, Reply, SignInCommit};
use axlauncher_core::network_spi::ServiceHost;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{AsyncApp, Context, Task, WeakEntity};
use tokio::sync::mpsc;

use super::{AccountSnapshot, AccountsPanel, DesktopAccounts, SignInCancellation};

type LoginResult = Result<Option<AccountSnapshot>, String>;

pub(super) use axlauncher_core::desktop::accounts::Method;

pub(super) struct Attempt {
    pub(super) stage: Stage,
    pub(super) cancellation: SignInCancellation,
}

pub(super) enum Stage {
    Starting,
    Browser { url: String },
    Device { url: String, code: String },
}

impl AccountsPanel {
    pub(super) fn sign_in(
        &mut self,
        method: Method,
        expected: Option<AccountId>,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(service) = self.service.clone() else {
            return;
        };
        if self.is_busy() {
            return;
        }
        let cancellation = SignInCancellation::default();
        let commit = cancellation.0.clone();
        self.observe_sign_in_commit(commit.clone(), cx);
        let (sender, events) = mpsc::unbounded_channel();
        self.login = Some(Attempt {
            stage: Stage::Starting,
            cancellation,
        });
        self.error = None;
        let worker = Arc::clone(&service);
        let fallen_proxy = self.proxy.clone();
        let task = cx.background_spawn(async move {
            run(&worker, method, expected, fallen_proxy, &sender, &commit).await
        });
        cx.spawn(async move |view, cx| {
            Self::show_attempt(view, cx, service, events, task).await;
        })
        .detach();
        cx.notify();
    }

    fn show_sign_in(
        &mut self,
        service: &Arc<DesktopAccounts>,
        stage: Stage,
        cx: &mut Context<'_, Self>,
    ) {
        if !self.is_current(service) {
            return;
        }
        let Some(attempt) = &mut self.login else {
            return;
        };
        if attempt.cancellation.0.is_pending()
            && let Stage::Browser { url } = &stage
        {
            cx.open_url(url);
        }
        attempt.stage = stage;
        cx.notify();
    }

    fn finish_sign_in(
        &mut self,
        service: &Arc<DesktopAccounts>,
        result: LoginResult,
        cx: &mut Context<'_, Self>,
    ) {
        if !self.is_current(service) {
            return;
        }
        self.login = None;
        match result {
            Ok(snapshot) => self.finish_account_change(snapshot, None, cx),
            Err(error) => self.finish_account_change(None, Some(error), cx),
        }
    }

    pub(super) fn cancel_sign_in(&mut self, cx: &mut Context<'_, Self>) {
        if let Some(attempt) = &self.login {
            attempt.cancellation.0.cancel();
        }
        cx.notify();
    }
    async fn show_attempt(
        view: WeakEntity<Self>,
        cx: &mut AsyncApp,
        service: Arc<DesktopAccounts>,
        mut events: mpsc::UnboundedReceiver<Stage>,
        task: Task<LoginResult>,
    ) {
        while let Some(stage) = events.recv().await {
            let _ = view.update(cx, |view, cx| view.show_sign_in(&service, stage, cx));
        }
        let result = task.await;
        let _ = view.update(cx, |view, cx| view.finish_sign_in(&service, result, cx));
    }
}

async fn run(
    service: &DesktopAccounts,
    method: Method,
    expected: Option<AccountId>,
    fallen_proxy: Option<ServiceHost>,
    sender: &mpsc::UnboundedSender<Stage>,
    commit: &SignInCommit,
) -> LoginResult {
    let request = Action::Microsoft {
        method,
        expected,
        proxy: fallen_proxy,
    };
    match service
        .sign_in(request, commit, |reply| match reply {
            Reply::Saving => commit.confirm_commit(),
            Reply::Browser { url } => {
                let _ = sender.send(Stage::Browser { url });
            }
            Reply::Device { url, code } => {
                let _ = sender.send(Stage::Device { url, code });
            }
            _ => (),
        })
        .await?
    {
        Some(Reply::Snapshot { snapshot, .. }) => Ok(Some(snapshot)),
        None => Ok(None),
        _ => Err("The account worker sent an invalid sign-in result.".into()),
    }
}
