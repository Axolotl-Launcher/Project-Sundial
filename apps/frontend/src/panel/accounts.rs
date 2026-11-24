// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Account selection and desktop account operations for the current data root.

use std::path::Path;
use std::sync::Arc;

use axlauncher_core::launch::plan::Session;
use axlauncher_core::launch_accounts;
use axlauncher_core::state::accounts::{
    Account, AccountId, AccountService, AccountSnapshot, SignInCommit,
};
use axlauncher_core::storage::{DataDir, SystemCredentials};
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    AsyncApp, AsyncWindowContext, Context, Entity, FocusHandle, Focusable, InputEvent, InputState,
    Task, WeakEntity, Window,
};

use super::RootView;

mod login;
mod render;
mod third_party;

pub(crate) type DesktopAccounts = AccountService<SystemCredentials>;
type AccountResult = (Option<AccountSnapshot>, Option<String>);

#[derive(Default)]
struct SignInCancellation(SignInCommit);

impl Drop for SignInCancellation {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

pub(super) struct AccountsPanel {
    service: Option<Arc<DesktopAccounts>>,
    snapshot: Option<AccountSnapshot>,
    error: Option<String>,
    working: bool,
    add_offline: bool,
    name: Entity<InputState>,
    uuid: Entity<InputState>,
    offline_trigger: FocusHandle,
    login: Option<login::Attempt>,
    third_party: third_party::Form,
}

impl AccountsPanel {
    pub(super) fn new(
        root: Option<&Path>,
        window: &mut Window,
        cx: &mut Context<'_, RootView>,
    ) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx));
        let uuid =
            cx.new(|cx| InputState::new(window, cx).placeholder("Generate from player name"));
        for input in [&name, &uuid] {
            cx.subscribe_in(input, window, RootView::offline_form_input)
                .detach();
        }
        Self {
            service: service(root),
            snapshot: None,
            error: None,
            working: false,
            add_offline: false,
            name,
            uuid,
            offline_trigger: cx.focus_handle(),
            login: None,
            third_party: third_party::Form::new(window, cx),
        }
    }

    pub(super) fn reset(
        &mut self,
        root: Option<&Path>,
        window: &mut Window,
        cx: &mut Context<'_, RootView>,
    ) {
        self.service = service(root);
        self.snapshot = None;
        self.error = None;
        self.working = false;
        self.add_offline = false;
        self.login = None;
        self.third_party.reset(window, cx);
    }

    pub(super) fn selected(&self) -> Option<&Account> {
        self.snapshot.as_ref()?.selected()
    }

    pub(super) fn selected_online(&self) -> Option<(Arc<DesktopAccounts>, AccountId)> {
        let id = self.selected()?.id();
        id.is_online()
            .then(|| (self.service.clone().expect("saved account service"), id))
    }

    fn is_current(&self, current: &Arc<DesktopAccounts>) -> bool {
        self.service
            .as_ref()
            .is_some_and(|service| Arc::ptr_eq(service, current))
    }

    fn is_busy(&self) -> bool {
        self.working || self.login.is_some() || self.third_party.is_working()
    }
}

fn service(root: Option<&Path>) -> Option<Arc<DesktopAccounts>> {
    let data = DataDir::new(root?);
    Some(Arc::new(AccountService::new(data, SystemCredentials)))
}

impl RootView {
    fn observe_sign_in_commit(&self, commit: SignInCommit, cx: &mut Context<'_, Self>) {
        cx.spawn(async move |view, cx| {
            commit.settled().await;
            let _ = view.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }
    pub(super) fn preview_session(&self, cx: &Context<'_, Self>) -> Session {
        let Some(account) = self.accounts.selected() else {
            return Session::offline(self.player_name(cx));
        };
        let service = self
            .accounts
            .service
            .as_ref()
            .expect("saved account service");
        launch_accounts::preview(account, service.data_dir())
    }

    pub(super) fn refresh_accounts(&mut self, cx: &mut Context<'_, Self>) {
        self.change_accounts(AccountAction::Read, cx);
    }

    fn change_accounts(&mut self, action: AccountAction, cx: &mut Context<'_, Self>) {
        self.start_account_action(action, None, cx);
    }

    fn start_account_action(
        &mut self,
        action: AccountAction,
        form_window: Option<&mut Window>,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(service) = self.accounts.service.clone() else {
            return;
        };
        if self.accounts.is_busy() {
            return;
        }
        self.accounts.working = true;
        self.accounts.error = None;
        let worker = Arc::clone(&service);
        let task = cx.background_spawn(async move { apply(action, &worker).await });
        let foreground = match form_window {
            Some(window) => cx.spawn_in(window, async move |view, cx| {
                show_offline_result(view, cx, service, task).await;
            }),
            None => cx.spawn(async move |view, cx| {
                show_account_action(view, cx, service, task).await;
            }),
        };
        foreground.detach();
        cx.notify();
    }

    fn finish_account_action(
        &mut self,
        service: &Arc<DesktopAccounts>,
        (snapshot, error): AccountResult,
        cx: &mut Context<'_, Self>,
    ) {
        if !self.accounts.is_current(service) {
            return;
        }
        self.accounts.working = false;
        self.finish_account_change(snapshot, error, cx);
    }

    fn finish_account_change(
        &mut self,
        snapshot: Option<AccountSnapshot>,
        error: Option<String>,
        cx: &mut Context<'_, Self>,
    ) {
        if let Some(snapshot) = snapshot {
            self.accounts.snapshot = Some(snapshot);
            self.inspection = super::launch::Inspection::default();
        }
        self.accounts.error = error;
        cx.notify();
    }

    pub(crate) fn sync_launch_accounts(
        &mut self,
        service: &Arc<DesktopAccounts>,
        cx: &mut Context<'_, Self>,
    ) {
        if self.accounts.is_current(service) {
            self.refresh_accounts(cx);
        }
    }

    pub(crate) fn account_selection_pending(&self) -> bool {
        self.accounts.working
    }

    fn open_offline_form(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.accounts.add_offline = true;
        for input in [&self.accounts.name, &self.accounts.uuid] {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.accounts
            .name
            .read(cx)
            .focus_handle(cx)
            .focus(window, cx);
        cx.notify();
    }

    fn close_offline_form(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.accounts.add_offline = false;
        self.accounts.error = None;
        self.accounts.offline_trigger.focus(window, cx);
        cx.notify();
    }

    fn submit_offline_form(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if !self.accounts.add_offline || self.accounts.is_busy() {
            return;
        }
        let name = self.accounts.name.read(cx).value().to_string();
        let uuid = self.accounts.uuid.read(cx).value().to_string();
        self.start_account_action(AccountAction::AddOffline { name, uuid }, Some(window), cx);
    }

    fn offline_form_input(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.accounts.add_offline
            && matches!(
                event,
                InputEvent::PressEnter {
                    secondary: false,
                    shift: false
                }
            )
        {
            self.submit_offline_form(window, cx);
        }
    }

    fn finish_offline_form(
        &mut self,
        service: &Arc<DesktopAccounts>,
        result: AccountResult,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.accounts.is_current(service) && result.1.is_none() {
            self.close_offline_form(window, cx);
        }
        self.finish_account_action(service, result, cx);
    }
}

async fn show_account_action(
    view: WeakEntity<RootView>,
    cx: &mut AsyncApp,
    service: Arc<DesktopAccounts>,
    task: Task<AccountResult>,
) {
    let result = task.await;
    let _ = view.update(cx, |view, cx| {
        view.finish_account_action(&service, result, cx)
    });
}

async fn show_offline_result(
    view: WeakEntity<RootView>,
    cx: &mut AsyncWindowContext,
    service: Arc<DesktopAccounts>,
    task: Task<AccountResult>,
) {
    let result = task.await;
    let _ = view.update_in(cx, |view, window, cx| {
        view.finish_offline_form(&service, result, window, cx)
    });
}

enum AccountAction {
    Read,
    AddOffline { name: String, uuid: String },
    Select(Option<AccountId>),
    Remove(AccountId),
}

async fn apply(action: AccountAction, service: &DesktopAccounts) -> AccountResult {
    let outcome = match action {
        AccountAction::Read => Ok(None),
        AccountAction::AddOffline { name, uuid } => {
            service.add_offline(&name, Some(&uuid)).map(|()| None)
        }
        AccountAction::Select(id) => service.select(id).map(|()| None),
        AccountAction::Remove(id) => service.remove(id).await.map(|cleared| {
            (!cleared).then_some(
                "The account was removed. The launcher will retry clearing its system credential.",
            )
        }),
    };
    match (outcome, service.snapshot().await) {
        (Ok(warning), Ok(snapshot)) => (Some(snapshot), warning.map(str::to_owned)),
        (Err(error), Ok(snapshot)) => (Some(snapshot), Some(error.to_string())),
        (_, Err(error)) => (None, Some(error.to_string())),
    }
}
