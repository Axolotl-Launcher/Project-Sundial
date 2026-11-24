// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The third-party sign-in form and its pending profile selection.

use std::sync::Arc;

use axlauncher_core::network::auth::AuthHttp;
use axlauncher_core::state::accounts::yggdrasil::{self, Profile, ProfileChoice, SignIn};
use axlauncher_core::state::accounts::{AccountError, AccountId, AccountSnapshot, SignInCommit};
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, AsyncWindowContext, Button, ButtonVariants, Context, Disableable,
    ElementId, Entity, FocusHandle, Focusable, Input, InputEvent, InputState, IntoElement, Label,
    Task, WeakEntity, Window, h_flex, v_flex,
};

use super::{DesktopAccounts, RootView, SignInCancellation};

type ResultState = Result<Outcome, String>;

enum Outcome {
    Profiles(ProfileChoice),
    Saved(AccountSnapshot),
    Cancelled,
}

struct Attempt {
    _task: Task<()>,
    cancellation: SignInCancellation,
}

pub(super) struct Form {
    url: Entity<InputState>,
    username: Entity<InputState>,
    password: Entity<InputState>,
    trigger: FocusHandle,
    open: bool,
    request: Option<Attempt>,
    choice: Option<ProfileChoice>,
    expected: Option<AccountId>,
}

impl Form {
    pub(super) fn new(window: &mut Window, cx: &mut Context<'_, RootView>) -> Self {
        let url = cx
            .new(|cx| InputState::new(window, cx).placeholder("https://example.com/api/yggdrasil"));
        let username = cx.new(|cx| InputState::new(window, cx));
        let password = cx.new(|cx| InputState::new(window, cx).masked(true));
        for input in [&url, &username, &password] {
            cx.subscribe_in(input, window, RootView::third_party_input)
                .detach();
        }
        Self {
            url,
            username,
            password,
            trigger: cx.focus_handle(),
            open: false,
            request: None,
            choice: None,
            expected: None,
        }
    }

    pub(super) fn reset(&mut self, window: &mut Window, cx: &mut Context<'_, RootView>) {
        self.open = false;
        self.request = None;
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

    pub(super) fn button(&self, disabled: bool, cx: &Context<'_, RootView>) -> Button {
        Button::new("third-party-sign-in")
            .label("Third-party sign-in…")
            .track_focus(&self.trigger)
            .disabled(disabled || self.open)
            .toggled(self.open)
            .on_click(cx.listener(|view, _, window, cx| view.open_third_party(None, window, cx)))
    }

    pub(super) fn render(&self, cx: &Context<'_, RootView>) -> AnyElement {
        let saving = self
            .request
            .as_ref()
            .is_some_and(|attempt| attempt.cancellation.0.is_saving());
        let status = if saving {
            "Saving account"
        } else {
            "Contacting the sign-in server"
        };
        let fields = match &self.choice {
            Some(choice) => self.render_profiles(choice, cx),
            None => self.render_fields(cx),
        };
        v_flex()
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
                    Button::new("third-party-cancel")
                        .ghost()
                        .label("Cancel")
                        .disabled(
                            self.request
                                .as_ref()
                                .is_some_and(|attempt| !attempt.cancellation.0.is_pending()),
                        )
                        .on_click(
                            cx.listener(|view, _, window, cx| view.cancel_third_party(window, cx)),
                        ),
                ),
            )
            .into_any_element()
    }

    fn render_fields(&self, cx: &Context<'_, RootView>) -> AnyElement {
        let disabled = self.is_working();
        v_flex()
            .gap_2()
            .child(field(
                &self.url,
                "third-party-url",
                "Yggdrasil API URL",
                disabled || self.expected.is_some(),
            ))
            .child(field(
                &self.username,
                "third-party-user",
                "Username",
                disabled,
            ))
            .child(field(
                &self.password,
                "third-party-password",
                "Password",
                disabled,
            ))
            .child(
                h_flex().child(
                    Button::new("third-party-submit")
                        .primary()
                        .label("Sign in")
                        .disabled(disabled)
                        .on_click(
                            cx.listener(|view, _, window, cx| view.submit_third_party(window, cx)),
                        ),
                ),
            )
            .into_any_element()
    }

    fn render_profiles(&self, choice: &ProfileChoice, cx: &Context<'_, RootView>) -> AnyElement {
        let buttons = choice
            .profiles()
            .iter()
            .cloned()
            .map(|profile| profile_button(profile, self.is_working(), cx));
        v_flex()
            .gap_2()
            .child(Label::new("Choose a Minecraft profile"))
            .child(h_flex().gap_2().flex_wrap().children(buttons))
            .into_any_element()
    }

    fn clear_password(&self, window: &mut Window, cx: &mut Context<'_, RootView>) {
        self.password
            .update(cx, |input, cx| input.set_value("", window, cx));
    }
}

fn profile_button(profile: Profile, disabled: bool, cx: &Context<'_, RootView>) -> Button {
    let id = ElementId::Name(format!("third-party-profile:{}", profile.id()).into());
    Button::new(id)
        .label(profile.name().to_owned())
        .disabled(disabled)
        .on_click(cx.listener(move |view, _, window, cx| {
            view.choose_third_party(profile.clone(), window, cx)
        }))
}

impl RootView {
    pub(super) fn open_third_party(
        &mut self,
        server: Option<(AccountId, String)>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.accounts.is_busy() {
            return;
        }
        self.accounts.add_offline = false;
        self.accounts.error = None;
        let form = &mut self.accounts.third_party;
        form.reset(window, cx);
        form.open = true;
        let (expected, url) =
            server.map_or_else(|| (None, String::new()), |(id, url)| (Some(id), url));
        form.expected = expected;
        form.url
            .update(cx, |input, cx| input.set_value(url, window, cx));
        form.username
            .update(cx, |input, cx| input.set_value("", window, cx));
        let focus = if expected.is_some() {
            &form.username
        } else {
            &form.url
        };
        focus.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn close_third_party(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let form = &mut self.accounts.third_party;
        form.reset(window, cx);
        form.trigger.focus(window, cx);
        self.accounts.error = None;
        cx.notify();
    }

    fn cancel_third_party(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let Some(attempt) = &mut self.accounts.third_party.request else {
            self.close_third_party(window, cx);
            return;
        };
        attempt.cancellation.0.cancel();
        cx.notify();
    }

    fn third_party_input(
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
        let form = &self.accounts.third_party;
        if !form.open || form.choice.is_some() || self.accounts.is_busy() {
            return;
        }
        let request = Request::SignIn {
            url: form.url.read(cx).value().to_string(),
            username: form.username.read(cx).value().to_string(),
            password: form.password.read(cx).value().to_string(),
        };
        form.clear_password(window, cx);
        self.run_third_party(request, window, cx);
    }

    fn choose_third_party(
        &mut self,
        profile: Profile,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.accounts.is_busy() {
            return;
        }
        let Some(choice) = self.accounts.third_party.choice.take() else {
            return;
        };
        self.run_third_party(Request::Choose { choice, profile }, window, cx);
    }

    fn run_third_party(
        &mut self,
        request: Request,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(service) = self.accounts.service.clone() else {
            return;
        };
        let worker = Arc::clone(&service);
        let expected = self.accounts.third_party.expected;
        self.accounts.error = None;
        let cancellation = SignInCancellation::default();
        let commit = cancellation.0.clone();
        self.observe_sign_in_commit(commit.clone(), cx);
        let task = cx.background_spawn(async move { run(request, &worker, expected, &commit) });
        let task = cx.spawn_in(window, async move |view, cx| {
            show_result(view, cx, service, task).await;
        });
        self.accounts.third_party.request = Some(Attempt {
            _task: task,
            cancellation,
        });
        cx.notify();
    }

    fn finish_third_party(
        &mut self,
        service: &Arc<DesktopAccounts>,
        result: ResultState,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if !self.accounts.is_current(service) {
            return;
        }
        let cancelled = self
            .accounts
            .third_party
            .request
            .take()
            .is_some_and(|attempt| {
                !attempt.cancellation.0.is_pending() && !attempt.cancellation.0.is_saving()
            });
        if cancelled {
            self.close_third_party(window, cx);
            return;
        }
        match result {
            Ok(Outcome::Profiles(choice)) => self.accounts.third_party.choice = Some(choice),
            Ok(Outcome::Cancelled) => self.close_third_party(window, cx),
            Ok(Outcome::Saved(snapshot)) => {
                self.close_third_party(window, cx);
                self.finish_account_change(Some(snapshot), None, cx);
            }
            Err(error) => self.accounts.error = Some(error),
        }
        cx.notify();
    }
}

enum Request {
    SignIn {
        url: String,
        username: String,
        password: String,
    },
    Choose {
        choice: ProfileChoice,
        profile: Profile,
    },
}

fn run(
    request: Request,
    service: &DesktopAccounts,
    expected: Option<AccountId>,
    commit: &SignInCommit,
) -> ResultState {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "Could not start third-party sign-in.".to_owned())?;
    runtime.block_on(sign_in(request, service, expected, commit))
}

async fn show_result(
    view: WeakEntity<RootView>,
    cx: &mut AsyncWindowContext,
    service: Arc<DesktopAccounts>,
    task: Task<ResultState>,
) {
    let result = task.await;
    let _ = view.update_in(cx, |view, window, cx| {
        view.finish_third_party(&service, result, window, cx)
    });
}

async fn sign_in(
    request: Request,
    service: &DesktopAccounts,
    expected: Option<AccountId>,
    commit: &SignInCommit,
) -> ResultState {
    let outcome = tokio::select! {
        biased;
        _ = commit.cancelled() => return Ok(Outcome::Cancelled),
        result = authenticate(request) => result?,
    };
    let verified = match outcome {
        SignIn::Choose(choice) if commit.is_pending() => return Ok(Outcome::Profiles(choice)),
        SignIn::Choose(_) => return Ok(Outcome::Cancelled),
        SignIn::Complete(verified) => verified,
    };
    match service
        .remember_yggdrasil_with_commit(verified, expected, commit)
        .await
    {
        Ok(()) => (),
        Err(AccountError::Cancelled) => return Ok(Outcome::Cancelled),
        Err(error) => return Err(error.to_string()),
    }
    service
        .snapshot()
        .await
        .map(Outcome::Saved)
        .map_err(|error| error.to_string())
}

async fn authenticate(request: Request) -> Result<SignIn, String> {
    let network =
        AuthHttp::desktop().map_err(|_| "Could not initialize third-party sign-in.".to_owned())?;
    match request {
        Request::SignIn {
            url,
            username,
            password,
        } => yggdrasil::sign_in(&network, &url, &username, &password).await,
        Request::Choose { choice, profile } => choice
            .finish(&network, profile.id())
            .await
            .map(SignIn::Complete),
    }
    .map_err(|error| error.to_string())
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
