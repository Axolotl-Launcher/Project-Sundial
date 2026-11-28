// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The Accounts section of the development panel.

use axlauncher_core::desktop::accounts::{Account, AccountId, Persistence};
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, Button, ButtonVariants, ClipboardItem, Context, Disableable,
    ElementId, GroupBox, Input, IntoElement, Label, Render, Window, h_flex, v_flex,
};

use crate::i18n::{interpolate, tr};

use super::login::{Method, Stage};
use super::{AccountAction, AccountSnapshot, AccountsPanel};

impl Render for AccountsPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let danger = cx.theme().danger;
        let selected = self.selected();
        let unavailable = self.service.is_none() || self.is_busy();
        let identity = selected.map_or_else(
            || {
                interpolate(
                    tr(cx, "accounts.identity.temporary"),
                    &[("name", &self.player_name(cx))],
                )
            },
            |account| {
                interpolate(
                    tr(cx, "accounts.identity.selected"),
                    &[
                        ("name", account.name()),
                        ("provider", account_provider(account, cx)),
                    ],
                )
            },
        );
        let rows = self
            .snapshot
            .as_ref()
            .map(|snapshot| Self::account_rows(snapshot, unavailable, cx));
        GroupBox::new()
            .id("accounts")
            .title(
                Label::new(tr(cx, "accounts.title"))
                    .text_sm()
                    .text_color(muted),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(Label::new(identity))
                    .child(self.account_commands(unavailable, cx))
                    .when(self.add_offline, |panel| panel.child(self.offline_form(cx)))
                    .when(self.login.is_some(), |panel| {
                        panel.child(self.login_status(cx))
                    })
                    .when(self.third_party.is_open(), |panel| {
                        panel.child(self.render_third_party(cx))
                    })
                    .when_some(self.error.clone(), |panel, error| {
                        panel.child(Label::new(error).text_sm().text_color(danger))
                    })
                    .children(rows.into_iter().flatten()),
            )
    }
}

impl AccountsPanel {
    fn account_commands(&self, disabled: bool, cx: &mut Context<'_, Self>) -> AnyElement {
        let disabled = disabled || self.third_party.is_open();
        let temporary = self
            .selected()
            .map(|_| Self::temporary_button(disabled, cx));
        h_flex()
            .gap_2()
            .flex_wrap()
            .child(
                Button::new("sign-in-microsoft")
                    .label(tr(cx, "accounts.microsoft.sign-in"))
                    .disabled(disabled)
                    .on_click(
                        cx.listener(|view, _, _, cx| view.sign_in(Method::Browser, None, cx)),
                    ),
            )
            .child(
                Button::new("sign-in-device")
                    .label(tr(cx, "accounts.microsoft.device"))
                    .ghost()
                    .disabled(disabled)
                    .on_click(cx.listener(|view, _, _, cx| view.sign_in(Method::Device, None, cx))),
            )
            .child(
                Button::new("add-offline-account")
                    .label(tr(cx, "accounts.offline.open"))
                    .track_focus(&self.offline_trigger)
                    .disabled(disabled || self.add_offline)
                    .toggled(self.add_offline)
                    .on_click(
                        cx.listener(|view, _, window, cx| view.open_offline_form(window, cx)),
                    ),
            )
            .child(self.third_party_button(disabled, cx))
            .children(temporary)
            .into_any_element()
    }

    fn offline_form(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        v_flex()
            .track_focus(&self.offline_scope)
            .gap_2()
            .child(Label::new(tr(cx, "accounts.offline.player-name")).text_sm())
            .child(
                Input::new(&self.name)
                    .id("saved-offline-name")
                    .aria_label(tr(cx, "accounts.offline.player-name"))
                    .w_64(),
            )
            .child(Label::new(tr(cx, "accounts.offline.uuid")).text_sm())
            .child(
                Input::new(&self.uuid)
                    .id("saved-offline-uuid")
                    .aria_label(tr(cx, "accounts.offline.uuid"))
                    .w_80(),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("save-offline-account")
                            .label(tr(cx, "accounts.offline.add"))
                            .disabled(self.is_busy())
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.submit_offline_form(window, cx)
                            })),
                    )
                    .child(
                        Button::new("cancel-offline-account")
                            .ghost()
                            .label(tr(cx, "accounts.offline.cancel"))
                            .disabled(self.is_busy())
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.close_offline_form(window, cx)
                            })),
                    ),
            )
            .into_any_element()
    }

    fn login_status(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let attempt = self.login.as_ref().expect("sign-in in progress");
        let starting = tr(cx, "accounts.microsoft.starting");
        let controls = if attempt.cancellation.0.is_saving() {
            Label::new(tr(cx, "accounts.status.saving")).into_any_element()
        } else {
            match &attempt.stage {
                Stage::Starting => Label::new(starting).into_any_element(),
                Stage::Browser { url } => h_flex()
                    .gap_2()
                    .child(Label::new(tr(cx, "accounts.microsoft.continue-browser")))
                    .child(Self::open_browser(url, cx))
                    .into_any_element(),
                Stage::Device { url, code } => Self::device_controls(url, code, cx),
            }
        };
        h_flex()
            .gap_2()
            .flex_wrap()
            .child(controls)
            .child(
                Button::new("cancel-sign-in")
                    .ghost()
                    .label(tr(cx, "accounts.microsoft.cancel"))
                    .disabled(!attempt.cancellation.0.is_pending())
                    .on_click(cx.listener(|view, _, _, cx| view.cancel_sign_in(cx))),
            )
            .into_any_element()
    }
    fn account_rows(
        snapshot: &AccountSnapshot,
        disabled: bool,
        cx: &Context<'_, Self>,
    ) -> Vec<AnyElement> {
        let selected = snapshot.selected();
        snapshot
            .iter()
            .map(|account| {
                Self::account_row(
                    account,
                    snapshot.persistence(account.id()),
                    selected == Some(account),
                    disabled,
                    cx,
                )
            })
            .collect()
    }

    fn account_row(
        account: &Account,
        persistence: Option<Persistence>,
        selected: bool,
        disabled: bool,
        cx: &Context<'_, Self>,
    ) -> AnyElement {
        let id = account.id();
        let label = account_label(account, persistence, cx);
        let details = if selected {
            interpolate(tr(cx, "accounts.selected"), &[("account", &label)])
        } else {
            label
        };
        let reauth = id
            .is_online()
            .then(|| Self::reauth_button(account, disabled, cx));
        h_flex()
            .gap_3()
            .justify_between()
            .flex_wrap()
            .child(
                Label::new(details)
                    .id(account_element("account", id))
                    .text_sm()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new(account_element("select-account", id))
                            .label(tr(cx, "accounts.use"))
                            .accessibility_label(account_action_label(
                                "accounts.use-name",
                                account,
                                cx,
                            ))
                            .disabled(disabled || selected)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.change_accounts(AccountAction::Select(Some(id)), cx)
                            })),
                    )
                    .children(reauth)
                    .child(
                        Button::new(account_element("remove-account", id))
                            .label(tr(cx, "accounts.remove"))
                            .accessibility_label(account_action_label(
                                "accounts.remove-name",
                                account,
                                cx,
                            ))
                            .ghost()
                            .disabled(disabled)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.change_accounts(AccountAction::Remove(id), cx)
                            })),
                    ),
            )
            .into_any_element()
    }

    fn reauth_button(account: &Account, disabled: bool, cx: &Context<'_, Self>) -> Button {
        let id = account.id();
        let server = account.server().map(|server| (id, server.url().to_owned()));
        Button::new(account_element("reauthenticate", id))
            .label(tr(cx, "accounts.reauthenticate"))
            .accessibility_label(interpolate(
                tr(cx, "accounts.reauthenticate-name"),
                &[("name", account.name())],
            ))
            .ghost()
            .disabled(disabled)
            .on_click(cx.listener(move |view, _, window, cx| {
                view.reauthenticate(id, server.clone(), window, cx);
            }))
    }

    fn reauthenticate(
        &mut self,
        id: AccountId,
        server: Option<(AccountId, String)>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        match id {
            AccountId::Microsoft(_) => self.sign_in(Method::Browser, Some(id), cx),
            AccountId::Yggdrasil { .. } => self.open_third_party(server, window, cx),
            AccountId::Offline(_) => (),
        }
    }

    fn temporary_button(disabled: bool, cx: &Context<'_, Self>) -> Button {
        Button::new("use-temporary-player")
            .label(tr(cx, "accounts.temporary.use"))
            .ghost()
            .disabled(disabled)
            .on_click(
                cx.listener(|view, _, _, cx| view.change_accounts(AccountAction::Select(None), cx)),
            )
    }

    fn device_controls(url: &str, code: &str, cx: &Context<'_, Self>) -> AnyElement {
        let code_to_copy = code.to_owned();
        h_flex()
            .gap_2()
            .child(Label::new(crate::i18n::interpolate(
                tr(cx, "accounts.microsoft.code"),
                &[("code", code)],
            )))
            .child(
                Button::new("copy-device-code")
                    .ghost()
                    .label(tr(cx, "accounts.microsoft.copy-code"))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(code_to_copy.clone()));
                    })),
            )
            .child(Self::open_browser(url, cx))
            .into_any_element()
    }

    fn open_browser(url: &str, cx: &Context<'_, Self>) -> Button {
        let url = url.to_owned();
        Button::new("open-sign-in-browser")
            .ghost()
            .label(tr(cx, "accounts.microsoft.open-browser"))
            .on_click(cx.listener(move |_, _, _, cx| cx.open_url(&url)))
    }
}

fn account_label(
    account: &Account,
    persistence: Option<Persistence>,
    cx: &axlauncher_ui::App,
) -> String {
    let kind = account_provider(account, cx);
    let status = match persistence {
        Some(Persistence::SessionOnly) => tr(cx, "accounts.persistence.session"),
        Some(Persistence::SignInRequired) => tr(cx, "accounts.persistence.sign-in"),
        Some(Persistence::StoreUnavailable) => tr(cx, "accounts.persistence.unavailable"),
        _ => "",
    };
    format!("{} · {kind}{status}", account.name())
}

fn account_provider<'a>(account: &'a Account, cx: &axlauncher_ui::App) -> &'a str {
    account
        .server()
        .map_or_else(|| account_kind(account.id(), cx), |server| server.name())
}

fn account_action_label(key: &'static str, account: &Account, cx: &axlauncher_ui::App) -> String {
    interpolate(
        tr(cx, key),
        &[
            ("name", account.name()),
            ("kind", account_kind(account.id(), cx)),
        ],
    )
}

fn account_kind(id: AccountId, cx: &axlauncher_ui::App) -> &'static str {
    match id {
        AccountId::Offline(_) => tr(cx, "accounts.kind.offline"),
        AccountId::Microsoft(_) => "Microsoft",
        AccountId::Yggdrasil { .. } => tr(cx, "accounts.kind.third-party"),
    }
}

fn account_element(role: &str, id: AccountId) -> ElementId {
    ElementId::Name(format!("{role}:{id:?}").into())
}
