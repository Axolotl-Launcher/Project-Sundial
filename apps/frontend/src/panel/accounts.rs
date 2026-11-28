// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Account selection and desktop account operations for the current data root.

use std::sync::Arc;

pub(crate) use axlauncher_core::desktop::accounts::DesktopAccounts;
use axlauncher_core::desktop::accounts::{
    Account, AccountSnapshot, Change, LauncherId, SignInCommit,
};
use axlauncher_core::desktop::{launching::Player, sources::fallen_proxy};
use axlauncher_core::launch::plan::Session;
use axlauncher_core::launch_accounts;
use axlauncher_core::network_spi::ServiceHost;
use axlauncher_core::state::SettingsData;
use axlauncher_core::storage::DataDir;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    App, AsyncApp, AsyncWindowContext, Context, Entity, EventEmitter, FocusHandle, Focusable,
    InputEvent, InputState, Task, WeakEntity, Window,
};

mod login;
mod render;
mod third_party;

type AccountResult = (Option<AccountSnapshot>, Option<String>);

#[derive(Clone, Copy)]
enum AccountForm {
    Offline,
    Launcher,
}

#[derive(Default)]
struct SignInCancellation(SignInCommit);

impl Drop for SignInCancellation {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

pub(super) struct AccountsPanel {
    service: Option<Arc<DesktopAccounts>>,
    offline_player: Entity<InputState>,
    proxy: Option<ServiceHost>,
    snapshot: Option<AccountSnapshot>,
    error: Option<String>,
    working: bool,
    add_offline: Option<LauncherId>,
    name: Entity<InputState>,
    uuid: Entity<InputState>,
    launcher_name: Entity<InputState>,
    offline_scope: FocusHandle,
    offline_trigger: FocusHandle,
    login: Option<login::Attempt>,
    third_party: third_party::Form,
}

pub(super) struct AccountSelectionChanged;

impl EventEmitter<AccountSelectionChanged> for AccountsPanel {}

impl AccountsPanel {
    pub(super) fn new(
        paths: Option<&DataDir>,
        offline_player: Entity<InputState>,
        settings: Option<&SettingsData>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx));
        let uuid =
            cx.new(|cx| InputState::new(window, cx).placeholder("Generate from player name"));
        let launcher_name =
            cx.new(|cx| InputState::new(window, cx).placeholder("New local account name"));
        for input in [&name, &uuid] {
            cx.subscribe_in(input, window, Self::offline_form_input)
                .detach();
        }
        cx.subscribe_in(&launcher_name, window, Self::launcher_form_input)
            .detach();
        cx.observe(&offline_player, |_, _, cx| cx.notify()).detach();
        let third_party = third_party::Form::new(window, cx, Self::third_party_input);
        let mut panel = Self {
            service: service(paths),
            offline_player,
            proxy: fallen_proxy(settings),
            snapshot: None,
            error: None,
            working: false,
            add_offline: None,
            name,
            uuid,
            launcher_name,
            offline_scope: cx.focus_handle(),
            offline_trigger: cx.focus_handle(),
            login: None,
            third_party,
        };
        panel.refresh_accounts(cx);
        panel
    }

    pub(super) fn reload(
        &mut self,
        paths: Option<&DataDir>,
        same_data: bool,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if same_data {
            self.update_paths(paths);
        } else {
            self.reset(paths, window, cx);
        }
        self.refresh_accounts(cx);
        cx.notify();
    }

    fn reset(&mut self, paths: Option<&DataDir>, window: &mut Window, cx: &mut App) {
        if let Some(service) = &self.service {
            service.close();
        }
        self.service = service(paths);
        self.snapshot = None;
        self.error = None;
        self.working = false;
        self.add_offline = None;
        self.launcher_name
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.login = None;
        self.third_party.reset(window, cx);
    }

    pub(super) fn selected(&self) -> Option<&Account> {
        self.snapshot.as_ref()?.selected()
    }

    fn update_paths(&self, paths: Option<&DataDir>) {
        if let (Some(service), Some(paths)) = (&self.service, paths) {
            service.update_paths(paths);
        }
    }

    pub(super) fn configure_sign_in(&mut self, settings: Option<&SettingsData>) {
        self.proxy = fallen_proxy(settings);
    }

    pub(super) fn player(&self, cx: &App) -> Player {
        launch_player(
            self.snapshot.as_ref(),
            self.service.as_ref(),
            self.proxy.clone(),
            || Session::offline(self.player_name(cx)),
        )
    }

    fn is_current(&self, current: &Arc<DesktopAccounts>) -> bool {
        self.service
            .as_ref()
            .is_some_and(|service| Arc::ptr_eq(service, current))
    }

    fn is_busy(&self) -> bool {
        self.working || self.login.is_some() || self.third_party.is_working()
    }
    fn observe_sign_in_commit(&self, commit: SignInCommit, cx: &mut Context<'_, Self>) {
        cx.spawn(async move |view, cx| {
            commit.settled().await;
            let _ = view.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    pub(super) fn player_name(&self, cx: &App) -> String {
        let value = self.offline_player.read(cx).value();
        let name = value.trim();
        if name.is_empty() {
            super::DEFAULT_PLAYER.to_owned()
        } else {
            name.to_owned()
        }
    }

    pub(super) fn preview_session(&self, cx: &App) -> Session {
        let Some(account) = self.selected() else {
            return Session::offline(self.player_name(cx));
        };
        let service = self.service.as_ref().expect("saved account service");
        launch_accounts::preview(account, &service.data_dir()).with_fallen_proxy(self.proxy.clone())
    }

    fn refresh_accounts(&mut self, cx: &mut Context<'_, Self>) {
        self.change_accounts(AccountAction::Read, cx);
    }

    fn change_accounts(&mut self, action: AccountAction, cx: &mut Context<'_, Self>) {
        self.start_account_action(action, None, cx);
    }

    fn start_account_action(
        &mut self,
        action: AccountAction,
        form_window: Option<(&mut Window, AccountForm)>,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(service) = self.service.clone() else {
            return;
        };
        if self.is_busy() {
            return;
        }
        self.working = true;
        self.error = None;
        let worker = Arc::clone(&service);
        let task = cx.background_spawn(async move { apply(action, &worker).await });
        let foreground = match form_window {
            Some((window, form)) => cx.spawn_in(window, async move |view, cx| {
                Self::show_form_result(view, cx, service, task, form).await;
            }),
            None => cx.spawn(async move |view, cx| {
                Self::show_account_action(view, cx, service, task).await;
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
        if !self.is_current(service) {
            return;
        }
        self.working = false;
        self.finish_account_change(snapshot, error, cx);
    }

    fn finish_account_change(
        &mut self,
        snapshot: Option<AccountSnapshot>,
        error: Option<String>,
        cx: &mut Context<'_, Self>,
    ) {
        if let Some(snapshot) = snapshot {
            self.snapshot = Some(snapshot);
            cx.emit(AccountSelectionChanged);
        }
        self.error = error;
        cx.notify();
    }

    pub(super) fn sync_launch_accounts(
        &mut self,
        service: &Arc<DesktopAccounts>,
        cx: &mut Context<'_, Self>,
    ) {
        if self.is_current(service) {
            self.refresh_accounts(cx);
        }
    }

    pub(super) fn account_selection_pending(&self) -> bool {
        self.working
    }

    fn open_offline_form(
        &mut self,
        launcher: LauncherId,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.add_offline = Some(launcher);
        for input in [&self.name, &self.uuid] {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.name.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn close_offline_form(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.offline_scope.contains_focused(window, cx) {
            self.offline_trigger.focus(window, cx);
        }
        self.add_offline = None;
        self.error = None;
        cx.notify();
    }

    fn submit_offline_form(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let Some(launcher) = self.add_offline else {
            return;
        };
        if self.is_busy() {
            return;
        }
        let name = self.name.read(cx).value().to_string();
        let uuid = self.uuid.read(cx).value().to_string();
        self.start_account_action(
            AccountAction::AddOffline {
                launcher,
                name,
                uuid,
            },
            Some((window, AccountForm::Offline)),
            cx,
        );
    }

    fn offline_form_input(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.add_offline.is_some()
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

    fn submit_launcher_form(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.snapshot.is_none() || self.add_offline.is_some() || self.third_party.is_open() {
            return;
        }
        let name = self.launcher_name.read(cx).value().to_string();
        self.start_account_action(
            AccountAction::CreateLauncher { name },
            Some((window, AccountForm::Launcher)),
            cx,
        );
    }

    fn launcher_form_input(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if matches!(
            event,
            InputEvent::PressEnter {
                secondary: false,
                shift: false
            }
        ) {
            self.submit_launcher_form(window, cx);
        }
    }

    fn finish_account_form(
        &mut self,
        service: &Arc<DesktopAccounts>,
        result: AccountResult,
        form: AccountForm,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.is_current(service) && result.1.is_none() {
            match form {
                AccountForm::Offline => self.close_offline_form(window, cx),
                AccountForm::Launcher => self.launcher_name.update(cx, |input, cx| {
                    input.set_value("", window, cx);
                }),
            }
        }
        self.finish_account_action(service, result, cx);
    }

    async fn show_account_action(
        view: WeakEntity<Self>,
        cx: &mut AsyncApp,
        service: Arc<DesktopAccounts>,
        task: Task<AccountResult>,
    ) {
        let result = task.await;
        let _ = view.update(cx, |view, cx| {
            view.finish_account_action(&service, result, cx)
        });
    }

    async fn show_form_result(
        view: WeakEntity<Self>,
        cx: &mut AsyncWindowContext,
        service: Arc<DesktopAccounts>,
        task: Task<AccountResult>,
        form: AccountForm,
    ) {
        let result = task.await;
        let _ = view.update_in(cx, |view, window, cx| {
            view.finish_account_form(&service, result, form, window, cx)
        });
    }
}

fn service(paths: Option<&DataDir>) -> Option<Arc<DesktopAccounts>> {
    paths.cloned().map(DesktopAccounts::new)
}

fn launch_player(
    snapshot: Option<&AccountSnapshot>,
    service: Option<&Arc<DesktopAccounts>>,
    fallen_proxy: Option<ServiceHost>,
    temporary: impl FnOnce() -> Session,
) -> Player {
    match snapshot.and_then(AccountSnapshot::selected_key) {
        Some(key) => Player::Saved {
            service: Arc::clone(service.expect("saved account service")),
            key,
            fallen_proxy,
        },
        None => Player::Offline(temporary()),
    }
}

use Change as AccountAction;

async fn apply(action: AccountAction, service: &DesktopAccounts) -> AccountResult {
    match service.change(action).await {
        Ok((snapshot, notice)) => (Some(snapshot), notice),
        Err(error) => (None, Some(error)),
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/frontend/src/panel/accounts.rs"]
mod tests;
