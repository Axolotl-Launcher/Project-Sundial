// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The development panel: one scrollable page for manual verification.

use std::collections::HashMap;
use std::path::PathBuf;

use axlauncher_core::BRAND_NAME;
use axlauncher_core::state::{SettingsData, SettingsOrigin, ThemeChoice};
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, Button, ButtonVariants, Context, Disableable, Entity, InputEvent,
    InputState, IntoElement, Label, Render, ScrollableElement, StyledExt, Window, div, h_flex,
    v_flex,
};

use crate::installations::Installation;
use crate::launches::Launch;
use crate::{Loaded, Startup, load_at, palette_for};

pub(crate) mod accounts;
mod actions;
mod activity;
mod create_instance;
mod download_debug;
mod environment;
mod instances;
mod launch;

pub(crate) use instances::{install_key, profile_version};

const DEFAULT_PLAYER: &str = "Player";

pub(crate) struct RootView {
    root: Option<PathBuf>,
    notice: Option<String>,
    settings: Option<SettingsData>,
    origin: Option<SettingsOrigin>,
    theme: ThemeChoice,
    root_input: Entity<InputState>,
    player_input: Entity<InputState>,
    accounts: accounts::AccountsPanel,
    new_name: Entity<InputState>,
    new_minecraft: Entity<InputState>,
    new_loader: Entity<InputState>,
    new_loader_version: Entity<InputState>,
    instances: instances::InstancesState,
    create_instance: create_instance::CreateInstance,
    download_debug: download_debug::DownloadDebug,
    inspection: launch::Inspection,
    installations: HashMap<PathBuf, HashMap<String, Installation>>,
    launches: HashMap<PathBuf, (String, Launch)>,
    environment: environment::EnvironmentState,
    activity: activity::ActivityLog,
}

impl RootView {
    pub(crate) fn new(startup: Startup, window: &mut Window, cx: &mut Context<'_, Self>) -> Self {
        let Startup { palette: _, loaded } = startup;
        let root_text = loaded
            .root
            .as_deref()
            .map(|root| root.display().to_string())
            .unwrap_or_default();
        let root_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Data directory")
                .default_value(root_text)
        });
        let player_input = cx.new(|cx| InputState::new(window, cx).placeholder(DEFAULT_PLAYER));
        let new_name = cx.new(|cx| InputState::new(window, cx).placeholder("New profile name"));
        let new_minecraft =
            cx.new(|cx| InputState::new(window, cx).placeholder("Minecraft, e.g. 1.21.11"));
        let new_loader =
            cx.new(|cx| InputState::new(window, cx).placeholder("Loader: fabric or quilt"));
        let new_loader_version =
            cx.new(|cx| InputState::new(window, cx).placeholder("Loader version (optional)"));
        cx.subscribe(&player_input, Self::player_changed).detach();
        let theme = loaded
            .settings
            .as_ref()
            .map(|settings| settings.theme)
            .unwrap_or_default();
        let create_instance = create_instance::CreateInstance::new(window, cx);
        let download_debug = download_debug::DownloadDebug::new(window, cx);
        let accounts = accounts::AccountsPanel::new(loaded.root.as_deref(), window, cx);
        let environment = environment::EnvironmentState::new(loaded.root.as_deref());
        let mut view = Self {
            root: loaded.root,
            notice: loaded.notice,
            settings: loaded.settings,
            origin: loaded.origin,
            theme,
            root_input,
            player_input,
            accounts,
            new_name,
            new_minecraft,
            new_loader,
            new_loader_version,
            instances: instances::InstancesState::new(loaded.profiles),
            create_instance,
            download_debug,
            inspection: launch::Inspection::default(),
            installations: HashMap::new(),
            launches: HashMap::new(),
            environment,
            activity: activity::ActivityLog::new(),
        };

        view.record_data_root();
        view.refresh_accounts(cx);
        view
    }

    fn player_changed(
        &mut self,
        _: Entity<InputState>,
        event: &InputEvent,
        cx: &mut Context<'_, Self>,
    ) {
        if matches!(event, InputEvent::Change) {
            self.inspection = launch::Inspection::default();
            cx.notify();
        }
    }

    fn player_name(&self, cx: &Context<'_, Self>) -> String {
        let value = self.player_input.read(cx).value();
        let name = value.trim();
        if name.is_empty() {
            DEFAULT_PLAYER.to_owned()
        } else {
            name.to_owned()
        }
    }

    pub(super) fn reload(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let loaded = load_at(self.root.clone());
        self.apply_loaded(loaded, window, cx);
        self.activity.info("Reloaded settings and instances");
        cx.notify();
    }

    fn apply_loaded(&mut self, loaded: Loaded, window: &mut Window, cx: &mut Context<'_, Self>) {
        let same_root = self.root == loaded.root;
        self.root = loaded.root;
        if !same_root {
            self.accounts.reset(self.root.as_deref(), window, cx);
        }
        self.refresh_accounts(cx);
        self.settings = loaded.settings;
        self.origin = loaded.origin;
        self.notice = loaded.notice;
        self.theme = self
            .settings
            .as_ref()
            .map(|settings| settings.theme)
            .unwrap_or_default();
        self.environment.refresh(self.root.as_deref());
        self.instances.replace(loaded.profiles, same_root);
        self.inspection = launch::Inspection::default();

        let root_text = self
            .root
            .as_deref()
            .map(|root| root.display().to_string())
            .unwrap_or_default();
        self.root_input
            .update(cx, |state, cx| state.set_value(root_text, window, cx));
        palette_for(self.theme).apply(cx);
    }

    fn record_data_root(&mut self) {
        match &self.root {
            Some(root) => self.activity.info(format!("Data root: {}", root.display())),
            None => self.activity.error("Data root unknown"),
        }
    }

    fn render_header(&mut self, cx: &mut Context<'_, Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let disabled = self.root.is_none();

        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_3()
                            .child(Label::new(BRAND_NAME).text_xl())
                            .child(Label::new("Development").text_sm().text_color(muted)),
                    )
                    .child(
                        Button::new("reload-all")
                            .label("Reload")
                            .ghost()
                            .disabled(disabled)
                            .on_click(cx.listener(|this, _, window, cx| this.reload(window, cx))),
                    ),
            )
            .when_some(self.notice.clone(), |header, notice| {
                header.child(Label::new(notice).text_xs().text_color(muted))
            })
            .into_any_element()
    }
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let background = cx.theme().background;

        div()
            .v_flex()
            .size_full()
            .bg(background)
            .p_6()
            .gap_4()
            .child(self.render_header(cx))
            .child(
                div()
                    .id("panel-scroll")
                    .v_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_5()
                    .overflow_y_scrollbar()
                    .child(self.render_accounts(cx))
                    .child(self.render_instances(cx))
                    .child(self.render_download_debug(cx))
                    .child(self.render_launch(cx))
                    .child(self.render_environment(cx))
                    .child(self.render_activity(cx)),
            )
    }
}
