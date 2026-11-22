// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Data root, paths and settings.

use std::path::PathBuf;

use axlauncher_core::launch::{Java, JavaMode, SystemJava};
use axlauncher_core::state::{SettingsOrigin, ThemeChoice, save_settings};
use axlauncher_core::storage::DataDir;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, Button, ButtonVariants, Context, Disableable, ElementId, GroupBox,
    Hsla, Input, IntoElement, Label, Switch, Window, h_flex, v_flex,
};

use crate::{describe_origin, load_at, palette_for};

use super::RootView;

pub(super) struct EnvironmentState {
    java_home: Option<PathBuf>,
    default_java: Option<PathBuf>,
    error: Option<String>,
    notice: Option<String>,
}

impl EnvironmentState {
    pub(super) fn new() -> Self {
        let mut state = Self {
            java_home: None,
            default_java: None,
            error: None,
            notice: None,
        };
        state.refresh();
        state
    }

    pub(super) fn refresh(&mut self) {
        let java = SystemJava::from_environment();
        self.java_home = java.java_home().map(PathBuf::from);
        self.default_java = java.executable(None, JavaMode::Windowed);
        self.error = None;
        self.notice = None;
    }
}

fn path_row(index: usize, label: &str, path: &std::path::Path, muted: Hsla) -> AnyElement {
    h_flex()
        .justify_between()
        .gap_4()
        .child(
            Label::new(label)
                .id(ElementId::named_usize("environment-label", index))
                .text_xs()
                .text_color(muted),
        )
        .child(
            Label::new(path.display().to_string())
                .id(ElementId::named_usize("environment-value", index))
                .text_xs()
                .text_color(muted),
        )
        .into_any_element()
}

impl RootView {
    pub(super) fn render_environment(&mut self, cx: &mut Context<'_, Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let danger = cx.theme().danger;
        let success = cx.theme().success;

        let paths = self.environment_paths(muted);
        let save_disabled = self.settings.is_none()
            || self.root.is_none()
            || matches!(
                self.origin,
                Some(SettingsOrigin::ReadOnlyOtherSchema { .. })
            )
            || self
                .settings
                .as_ref()
                .is_some_and(|settings| settings.theme == self.theme);

        GroupBox::new()
            .id("environment")
            .title(
                Label::new("Environment & settings")
                    .text_sm()
                    .text_color(muted),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(self.root_controls(cx))
                    .children(paths)
                    .child(self.settings_controls(cx, save_disabled))
                    .child(
                        Label::new(format!("settings.json: {}", self.settings_origin()))
                            .text_xs()
                            .text_color(muted),
                    )
                    .children(self.java_rows(muted))
                    .when_some(self.environment.notice.clone(), |column, notice| {
                        column.child(Label::new(notice).text_xs().text_color(success))
                    })
                    .when_some(self.environment.error.clone(), |column, error| {
                        column.child(Label::new(error).text_xs().text_color(danger))
                    })
                    .when(self.root.is_none(), |column| {
                        column.child(
                            Label::new("Choose a data directory to enable the other sections.")
                                .text_xs()
                                .text_color(muted),
                        )
                    }),
            )
            .into_any_element()
    }

    fn root_controls(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        h_flex()
            .gap_3()
            .child(
                Input::new(&self.root_input)
                    .id("environment-root-input")
                    .aria_label("Data directory")
                    .flex_1(),
            )
            .child(
                Button::new("environment-apply")
                    .label("Apply")
                    .primary()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.apply_root(window, cx);
                    })),
            )
            .into_any_element()
    }

    fn environment_paths(&self, muted: Hsla) -> Vec<AnyElement> {
        self.root
            .as_ref()
            .map(|root| {
                let data = DataDir::new(root);
                vec![
                    ("Data root", data.root().to_path_buf()),
                    ("Profiles", data.instances()),
                    ("Shared game files", data.meta()),
                    ("Versions", data.versions()),
                    ("Libraries", data.libraries()),
                    ("Assets", data.assets()),
                    ("Settings", data.settings()),
                ]
            })
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(index, (label, path))| path_row(index, label, &path, muted))
            .collect()
    }

    fn settings_controls(&self, cx: &mut Context<'_, Self>, save_disabled: bool) -> AnyElement {
        h_flex()
            .gap_4()
            .items_center()
            .child(
                Switch::new("theme-switch")
                    .checked(self.theme == ThemeChoice::Light)
                    .label("Light theme")
                    .on_change(cx.listener(|this, light, _window, cx| {
                        this.toggle_theme(*light, cx);
                    })),
            )
            .child(
                Button::new("settings-save")
                    .label("Save settings")
                    .disabled(save_disabled)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.save_settings(cx);
                    })),
            )
            .into_any_element()
    }

    fn java_rows(&self, muted: Hsla) -> Vec<AnyElement> {
        let java_home = self
            .environment
            .java_home
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "not set".to_string());
        let default_java = self
            .environment
            .default_java
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "not found".to_string());

        vec![
            Label::new(format!("JAVA_HOME: {java_home}"))
                .text_xs()
                .text_color(muted)
                .into_any_element(),
            Label::new(format!("Default Java: {default_java}"))
                .text_xs()
                .text_color(muted)
                .into_any_element(),
        ]
    }

    fn settings_origin(&self) -> String {
        self.origin
            .as_ref()
            .map(describe_origin)
            .unwrap_or_else(|| "not loaded".to_string())
    }

    pub(super) fn apply_root(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let text = self.root_input.read(cx).value().trim().to_string();
        if text.is_empty() {
            self.environment.error = Some("Enter a data directory.".to_string());
            cx.notify();
            return;
        }

        let path = match std::path::absolute(&text) {
            Ok(path) => path,
            Err(error) => {
                self.environment.error = Some(error.to_string());
                cx.notify();
                return;
            }
        };
        let loaded = load_at(Some(path));
        self.apply_loaded(loaded, window, cx);
        self.record_data_root();
        cx.notify();
    }

    pub(super) fn save_settings(&mut self, cx: &mut Context<'_, Self>) {
        let Some(root) = self.root.clone() else {
            self.environment.error = Some("Choose a data directory first.".to_string());
            cx.notify();
            return;
        };
        let Some(settings) = self.settings.clone() else {
            self.environment.error = Some("Settings could not be loaded.".to_string());
            cx.notify();
            return;
        };

        let mut data = settings;
        data.theme = self.theme;
        match save_settings(&root, &data) {
            Ok(()) => {
                self.settings = Some(data);
                self.origin = Some(SettingsOrigin::Stored);
                self.environment.notice = Some("Settings saved.".to_string());
                self.environment.error = None;
                self.activity.success("Saved settings");
            }
            Err(error) => {
                let message = error.to_string();
                self.environment.error = Some(message.clone());
                self.activity
                    .error(format!("Settings save failed: {message}"));
            }
        }
        cx.notify();
    }

    pub(super) fn toggle_theme(&mut self, light: bool, cx: &mut Context<'_, Self>) {
        self.theme = if light {
            ThemeChoice::Light
        } else {
            ThemeChoice::Dark
        };
        palette_for(self.theme).apply(cx);
        self.environment.notice = None;
        self.environment.error = None;
        cx.notify();
    }
}
