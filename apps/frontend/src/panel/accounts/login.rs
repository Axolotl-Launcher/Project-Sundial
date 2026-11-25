// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Runs sign-in attempts off the GPUI thread and routes browser actions to it.

use std::sync::Arc;

use axlauncher_core::network::auth::{AuthHttp, BrowserCallback, CallbackError};
use axlauncher_core::spi::ServiceHost;
use axlauncher_core::state::accounts::microsoft::{
    BrowserLogin, CALLBACK_PATH, CALLBACK_PORT, DeviceLogin, VerifiedAccount,
};
use axlauncher_core::state::accounts::{AccountError, AccountId, SignInCommit};
use axlauncher_ui::prelude::*;
use axlauncher_ui::{AsyncApp, Context, Task, WeakEntity};
use tokio::sync::mpsc;

use super::{AccountSnapshot, DesktopAccounts, RootView, SignInCancellation};

type LoginResult = Result<Option<AccountSnapshot>, String>;

#[derive(Clone, Copy)]
pub(super) enum Method {
    Browser,
    Device,
}

pub(super) struct Attempt {
    pub(super) stage: Stage,
    pub(super) cancellation: SignInCancellation,
}

pub(super) enum Stage {
    Starting,
    Browser { url: String },
    Device { url: String, code: String },
}

impl RootView {
    pub(super) fn sign_in(
        &mut self,
        method: Method,
        expected: Option<AccountId>,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(service) = self.accounts.service.clone() else {
            return;
        };
        if self.accounts.is_busy() {
            return;
        }
        let cancellation = SignInCancellation::default();
        let commit = cancellation.0.clone();
        self.observe_sign_in_commit(commit.clone(), cx);
        let (sender, events) = mpsc::unbounded_channel();
        self.accounts.login = Some(Attempt {
            stage: Stage::Starting,
            cancellation,
        });
        self.accounts.error = None;
        let worker = Arc::clone(&service);
        let fallen_proxy = self.fallen_proxy();
        let task = cx.background_spawn(async move {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|_| "Could not start Microsoft sign-in.".to_owned())?;
            runtime.block_on(run(
                &worker,
                method,
                expected,
                fallen_proxy,
                &sender,
                &commit,
            ))
        });
        cx.spawn(async move |view, cx| {
            show_attempt(view, cx, service, events, task).await;
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
        if !self.accounts.is_current(service) {
            return;
        }
        let Some(attempt) = &mut self.accounts.login else {
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
        if !self.accounts.is_current(service) {
            return;
        }
        self.accounts.login = None;
        match result {
            Ok(snapshot) => self.finish_account_change(snapshot, None, cx),
            Err(error) => self.finish_account_change(None, Some(error), cx),
        }
    }

    pub(super) fn cancel_sign_in(&mut self, cx: &mut Context<'_, Self>) {
        if let Some(attempt) = &self.accounts.login {
            attempt.cancellation.0.cancel();
        }
        cx.notify();
    }
}

async fn show_attempt(
    view: WeakEntity<RootView>,
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

async fn run(
    service: &DesktopAccounts,
    method: Method,
    expected: Option<AccountId>,
    fallen_proxy: Option<ServiceHost>,
    sender: &mpsc::UnboundedSender<Stage>,
    commit: &SignInCommit,
) -> LoginResult {
    let network = AuthHttp::desktop()
        .map_err(|_| "Could not initialize Microsoft sign-in.".to_owned())?
        .with_fallen_proxy(fallen_proxy);
    let verified = tokio::select! {
        biased;
        _ = commit.cancelled() => return Ok(None),
        verified = authorize(method, &network, sender) => verified?,
    };
    match service
        .remember_with_commit(verified, expected, commit)
        .await
    {
        Ok(()) => (),
        Err(AccountError::Cancelled) => return Ok(None),
        Err(error) => return Err(error.to_string()),
    }
    let snapshot = service
        .snapshot()
        .await
        .map_err(|error| error.to_string())?;
    Ok(Some(snapshot))
}

async fn authorize(
    method: Method,
    network: &AuthHttp,
    sender: &mpsc::UnboundedSender<Stage>,
) -> Result<VerifiedAccount, String> {
    match method {
        Method::Browser => {
            let flow = BrowserLogin::begin();
            let callback = BrowserCallback::bind(CALLBACK_PORT, CALLBACK_PATH)
                .await
                .map_err(callback_message)?;
            sender
                .send(Stage::Browser {
                    url: flow.url().to_owned(),
                })
                .map_err(|_| "Sign-in was closed.")?;
            let code = callback
                .receive(flow.state())
                .await
                .map_err(callback_message)?;
            flow.finish(network, &code)
                .await
                .map_err(|error| error.to_string())
        }
        Method::Device => {
            let flow = DeviceLogin::begin(network)
                .await
                .map_err(|error| error.to_string())?;
            let stage = Stage::Device {
                url: flow.url().to_owned(),
                code: flow.user_code().to_owned(),
            };
            sender.send(stage).map_err(|_| "Sign-in was closed.")?;
            flow.finish(network)
                .await
                .map_err(|error| error.to_string())
        }
    }
}

fn callback_message(error: CallbackError) -> String {
    match error {
        CallbackError::Unavailable => {
            "The browser callback port is unavailable. Try signing in with a device code."
        }
        CallbackError::TimedOut => "Browser sign-in timed out. Start sign-in again.",
        CallbackError::Denied => "Microsoft sign-in was declined.",
    }
    .to_owned()
}
