// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The development panel: one scrollable page for manual verification.

use std::path::PathBuf;

use axlauncher_core::BRAND_NAME;
use axlauncher_core::state::{SettingsData, SettingsOrigin, ThemeChoice};
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, Button, ButtonVariants, Context, Disableable, Entity, IntoElement,
    Label, Render, ScrollableElement, StyledExt, Window, div, h_flex, v_flex,
};

use crate::{Loaded, Startup, load_at, palette_for};

mod activity;
mod environment;
mod instances;
mod launch;

pub(crate) struct RootView {
    root: Option<PathBuf>,
    notice: Option<String>,
    settings: Option<SettingsData>,
    origin: Option<SettingsOrigin>,
    theme: ThemeChoice,
    root_input: Entity<axlauncher_ui::InputState>,
    instances: instances::InstancesState,
    launch: launch::LaunchSection,
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
            axlauncher_ui::InputState::new(window, cx)
                .placeholder("Data directory")
                .default_value(root_text)
        });
        let theme = loaded
            .settings
            .as_ref()
            .map(|settings| settings.theme)
            .unwrap_or_default();
        let instances =
            instances::InstancesState::new(&loaded.instances, loaded.instances_error, window, cx);
        let launch = launch::LaunchSection::new(window, cx);
        let environment = environment::EnvironmentState::new();

        let mut view = Self {
            root: loaded.root,
            notice: loaded.notice,
            settings: loaded.settings,
            origin: loaded.origin,
            theme,
            root_input,
            instances,
            launch,
            environment,
            activity: activity::ActivityLog::new(),
        };

        if let Some(first) = view.instances.items.first().cloned() {
            view.select_instance(first, window, cx);
        }
        match &view.root {
            Some(root) => view.activity.info(format!("Data root: {}", root.display())),
            None => view.activity.error("Data root unknown"),
        }
        view
    }

    pub(super) fn reload(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let loaded = load_at(self.root.clone());
        self.apply_loaded(loaded, window, cx);
        self.activity.info("Reloaded settings and instances");
        cx.notify();
    }

    fn apply_loaded(&mut self, loaded: Loaded, window: &mut Window, cx: &mut Context<'_, Self>) {
        let selected_id = self
            .instances
            .selected
            .as_ref()
            .map(|instance| instance.id.clone());

        self.root = loaded.root;
        self.settings = loaded.settings;
        self.origin = loaded.origin;
        self.notice = loaded.notice;
        self.theme = self
            .settings
            .as_ref()
            .map(|settings| settings.theme)
            .unwrap_or_default();
        self.environment.refresh();
        self.instances
            .replace(loaded.instances, loaded.instances_error, window, cx);

        let next = selected_id
            .and_then(|id| {
                self.instances
                    .items
                    .iter()
                    .find(|instance| instance.id == id)
                    .cloned()
            })
            .or_else(|| self.instances.items.first().cloned());
        match next {
            Some(instance) => self.select_instance(instance, window, cx),
            None => {
                self.instances.clear_selection();
                self.launch.clear();
            }
        }

        let root_text = self
            .root
            .as_deref()
            .map(|root| root.display().to_string())
            .unwrap_or_default();
        self.root_input
            .update(cx, |state, cx| state.set_value(root_text, window, cx));
        palette_for(self.theme).apply(cx);
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
                    .child(self.render_instances(cx))
                    .child(self.render_launch(cx))
                    .child(self.render_environment(cx))
                    .child(self.render_activity(cx)),
            )
    }
}
