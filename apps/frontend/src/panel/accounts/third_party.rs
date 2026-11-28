// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The third-party sign-in form and its pending profile selection.

use std::sync::Arc;

use axlauncher_core::desktop::accounts::{
    AccountId, AccountSnapshot, Action, ChoiceId, Profile, Reply, SignInCommit,
};
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, App, AsyncWindowContext, Button, ButtonVariants, Context, Disableable,
    ElementId, Entity, FocusHandle, Focusable, Input, InputEvent, InputState, IntoElement, Label,
    Task, WeakEntity, Window, h_flex, v_flex,
};

use crate::i18n::tr;

use super::{AccountsPanel, DesktopAccounts, SignInCancellation};

type ResultState = Result<Outcome, String>;

enum Outcome {
    Profiles(ProfileChoice),
    Saved(AccountSnapshot),
    Cancelled,
}

struct ProfileChoice {
    id: ChoiceId,
    profiles: Vec<Profile>,
}

struct Attempt {
    _task: Task<()>,
    cancellation: SignInCancellation,
}

pub(super) struct Form {
    url: Entity<InputState>,
    username: Entity<InputState>,
    password: Entity<InputState>,
    focus_scope: FocusHandle,
    trigger: FocusHandle,
    open: bool,
    request: Option<Attempt>,
    choice: Option<ProfileChoice>,
    expected: Option<AccountId>,
}

impl Form {
    pub(super) fn new<Owner: 'static>(
        window: &mut Window,
        cx: &mut Context<'_, Owner>,
        on_input: impl FnMut(
            &mut Owner,
            &Entity<InputState>,
            &InputEvent,
            &mut Window,
            &mut Context<'_, Owner>,
        ) + Copy
        + 'static,
    ) -> Self {
        let url = cx
            .new(|cx| InputState::new(window, cx).placeholder("https://example.com/api/yggdrasil"));
        let username = cx.new(|cx| InputState::new(window, cx));
        let password = cx.new(|cx| InputState::new(window, cx).masked(true));
        for input in [&url, &username, &password] {
            cx.subscribe_in(input, window, on_input).detach();
        }
        Self {
            url,
            username,
            password,
            focus_scope: cx.focus_handle(),
            trigger: cx.focus_handle(),
            open: false,
            request: None,
            choice: None,
            expected: None,
        }
    }

    pub(super) fn reset(&mut self, window: &mut Window, cx: &mut App) {
        self.open = false;
        self.finish_attempt();
        self.choice = None;
        self.expected = None;
        self.clear_password(window, cx);
    }

    pub(super) fn is_open(&self) -> bool {
        self.open
    }

    pub(super) fn is_working(&self) -> bool {
        self.request.is_some()
    }

    fn track_attempt(&mut self, task: Task<()>, cancellation: SignInCancellation) {
        self.request = Some(Attempt {
            _task: task,
            cancellation,
        });
    }

    fn finish_attempt(&mut self) {
        self.request = None;
    }

    fn button(&self, disabled: bool, cx: &App) -> Button {
        Button::new("third-party-sign-in")
            .label(tr(cx, "accounts.third-party.open"))
            .track_focus(&self.trigger)
            .disabled(disabled || self.open)
            .toggled(self.open)
    }

    fn render(
        &self,
        submit: Button,
        cancel: Button,
        choose: impl Fn(Profile) -> Button,
        cx: &App,
    ) -> AnyElement {
        let saving = self
            .request
            .as_ref()
            .is_some_and(|attempt| attempt.cancellation.0.is_saving());
        let status = if saving {
            tr(cx, "accounts.status.saving")
        } else {
            tr(cx, "accounts.third-party.contacting")
        };
        let fields = match &self.choice {
            Some(choice) => {
                let buttons = choice.profiles.iter().cloned().map(choose);
                v_flex()
                    .gap_2()
                    .child(Label::new(tr(cx, "accounts.third-party.choose-profile")))
                    .child(h_flex().gap_2().flex_wrap().children(buttons))
                    .into_any_element()
            }
            None => self.render_fields(submit, cx),
        };
        v_flex()
            .track_focus(&self.focus_scope)
            .gap_2()
            .child(fields)
            .when(self.is_working(), |column| {
                column.child(
                    Label::new(status)
                        .text_sm()
                        .text_color(cx.theme().muted_foreground),
                )
            })
            .child(
                h_flex().child(
                    cancel.disabled(
                        self.request
                            .as_ref()
                            .is_some_and(|attempt| !attempt.cancellation.0.is_pending()),
                    ),
                ),
            )
            .into_any_element()
    }

    fn render_fields(&self, submit: Button, cx: &App) -> AnyElement {
        let disabled = self.is_working();
        v_flex()
            .gap_2()
            .child(field(
                &self.url,
                "third-party-url",
                tr(cx, "accounts.third-party.url"),
                disabled || self.expected.is_some(),
            ))
            .child(field(
                &self.username,
                "third-party-user",
                tr(cx, "accounts.third-party.username"),
                disabled,
            ))
            .child(field(
                &self.password,
                "third-party-password",
                tr(cx, "accounts.third-party.password"),
                disabled,
            ))
            .child(h_flex().child(submit.disabled(disabled)))
            .into_any_element()
    }

    fn clear_password(&self, window: &mut Window, cx: &mut App) {
        self.password
            .update(cx, |input, cx| input.set_value("", window, cx));
    }

    fn retain_focus_when_replacing_fields(&self, window: &mut Window, cx: &mut App) {
        if self.focus_scope.contains_focused(window, cx) {
            self.focus_scope.focus(window, cx);
        }
    }
    fn open(&mut self, server: Option<(AccountId, String)>, window: &mut Window, cx: &mut App) {
        self.reset(window, cx);
        self.open = true;
        let (expected, url) =
            server.map_or_else(|| (None, String::new()), |(id, url)| (Some(id), url));
        self.expected = expected;
        self.url
            .update(cx, |input, cx| input.set_value(url, window, cx));
        self.username
            .update(cx, |input, cx| input.set_value("", window, cx));
        let focus = if expected.is_some() {
            &self.username
        } else {
            &self.url
        };
        focus.read(cx).focus_handle(cx).focus(window, cx);
    }

    fn close(&mut self, service: Option<&DesktopAccounts>, window: &mut Window, cx: &mut App) {
        if let (Some(service), Some(choice)) = (service, self.choice.take()) {
            service.discard_choice(choice.id);
        }
        self.reset(window, cx);
        if self.focus_scope.contains_focused(window, cx) {
            self.trigger.focus(window, cx);
        }
    }

    fn cancel_attempt(&self) -> bool {
        let Some(attempt) = &self.request else {
            return false;
        };
        attempt.cancellation.0.cancel();
        true
    }

    fn sign_in_request(&self, window: &mut Window, cx: &mut App) -> Option<Action> {
        if !self.open || self.choice.is_some() {
            return None;
        }
        let request = Action::Yggdrasil {
            url: self.url.read(cx).value().to_string(),
            username: self.username.read(cx).value().to_string(),
            password: self.password.read(cx).value().to_string(),
            expected: self.expected,
        };
        self.clear_password(window, cx);
        Some(request)
    }

    fn choose_request(
        &mut self,
        profile: Profile,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Action> {
        let choice = self.choice.take()?;
        self.retain_focus_when_replacing_fields(window, cx);
        Some(Action::Choose {
            choice: choice.id,
            profile,
            expected: self.expected,
        })
    }

    fn show_profiles(&mut self, choice: ProfileChoice, window: &mut Window, cx: &mut App) {
        self.retain_focus_when_replacing_fields(window, cx);
        self.choice = Some(choice);
    }
}

impl AccountsPanel {
    pub(super) fn third_party_button(&self, disabled: bool, cx: &Context<'_, Self>) -> Button {
        self.third_party
            .button(disabled, cx)
            .on_click(cx.listener(|view, _, window, cx| view.open_third_party(None, window, cx)))
    }

    pub(super) fn render_third_party(&self, cx: &Context<'_, Self>) -> AnyElement {
        let submit = Button::new("third-party-submit")
            .primary()
            .label(tr(cx, "accounts.third-party.sign-in"))
            .on_click(cx.listener(|view, _, window, cx| view.submit_third_party(window, cx)));
        let cancel = Button::new("third-party-cancel")
            .ghost()
            .label(tr(cx, "accounts.third-party.cancel"))
            .on_click(cx.listener(|view, _, window, cx| view.cancel_third_party(window, cx)));
        self.third_party.render(
            submit,
            cancel,
            |profile| Self::profile_button(profile, self.third_party.is_working(), cx),
            cx,
        )
    }

    fn profile_button(profile: Profile, disabled: bool, cx: &Context<'_, Self>) -> Button {
        let id = ElementId::Name(format!("third-party-profile:{}", profile.id()).into());
        Button::new(id)
            .label(profile.name().to_owned())
            .disabled(disabled)
            .on_click(cx.listener(move |view, _, window, cx| {
                view.choose_third_party(profile.clone(), window, cx)
            }))
    }

    pub(super) fn open_third_party(
        &mut self,
        server: Option<(AccountId, String)>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.is_busy() {
            return;
        }
        self.add_offline = false;
        self.error = None;
        self.third_party.open(server, window, cx);
        cx.notify();
    }

    fn close_third_party(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.third_party.close(self.service.as_deref(), window, cx);
        self.error = None;
        cx.notify();
    }

    fn cancel_third_party(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if !self.third_party.cancel_attempt() {
            self.close_third_party(window, cx);
            return;
        }
        cx.notify();
    }

    pub(super) fn third_party_input(
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
            self.submit_third_party(window, cx);
        }
    }

    fn submit_third_party(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.is_busy() {
            return;
        }
        if let Some(request) = self.third_party.sign_in_request(window, cx) {
            self.run_third_party(request, window, cx);
        }
    }

    fn choose_third_party(
        &mut self,
        profile: Profile,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.is_busy() {
            return;
        }
        if let Some(request) = self.third_party.choose_request(profile, window, cx) {
            self.run_third_party(request, window, cx);
        }
    }

    fn run_third_party(
        &mut self,
        request: Action,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(service) = self.service.clone() else {
            return;
        };
        let worker = Arc::clone(&service);
        self.error = None;
        let cancellation = SignInCancellation::default();
        let commit = cancellation.0.clone();
        self.observe_sign_in_commit(commit.clone(), cx);
        let task = cx.background_spawn(async move { run(request, &worker, &commit).await });
        let task = cx.spawn_in(window, async move |view, cx| {
            Self::show_result(view, cx, service, task).await;
        });
        self.third_party.track_attempt(task, cancellation);
        cx.notify();
    }

    fn finish_third_party(
        &mut self,
        service: &Arc<DesktopAccounts>,
        result: ResultState,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if !self.is_current(service) {
            return;
        }
        let form = &mut self.third_party;
        form.finish_attempt();
        match result {
            Ok(Outcome::Profiles(choice)) => form.show_profiles(choice, window, cx),
            Ok(Outcome::Cancelled) => self.close_third_party(window, cx),
            Ok(Outcome::Saved(snapshot)) => {
                self.close_third_party(window, cx);
                self.finish_account_change(Some(snapshot), None, cx);
            }
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }

    async fn show_result(
        view: WeakEntity<Self>,
        cx: &mut AsyncWindowContext,
        service: Arc<DesktopAccounts>,
        task: Task<ResultState>,
    ) {
        let result = task.await;
        let _ = view.update_in(cx, |view, window, cx| {
            view.finish_third_party(&service, result, window, cx)
        });
    }
}

async fn run(action: Action, service: &DesktopAccounts, commit: &SignInCommit) -> ResultState {
    match service
        .sign_in(action, commit, |reply| {
            if matches!(reply, Reply::Saving) {
                commit.confirm_commit();
            }
        })
        .await?
    {
        Some(Reply::Snapshot { snapshot, .. }) => Ok(Outcome::Saved(snapshot)),
        Some(Reply::Profiles { choice, profiles }) if commit.is_pending() => {
            Ok(Outcome::Profiles(ProfileChoice {
                id: choice,
                profiles,
            }))
        }
        Some(Reply::Profiles { choice, .. }) => {
            service.discard_choice(choice);
            Ok(Outcome::Cancelled)
        }
        None => Ok(Outcome::Cancelled),
        _ => Err("The account worker sent an invalid sign-in result.".into()),
    }
}

fn field(
    input: &Entity<InputState>,
    id: &'static str,
    name: &'static str,
    disabled: bool,
) -> AnyElement {
    v_flex()
        .gap_1()
        .child(Label::new(name).id(format!("{id}-label")).text_sm())
        .child(
            Input::new(input)
                .id(id)
                .aria_label(name)
                .w_80()
                .disabled(disabled),
        )
        .into_any_element()
}
