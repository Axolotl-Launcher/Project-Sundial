// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Data root, paths and settings.

use std::path::{Path, PathBuf};

use axlauncher_core::desktop::{java::JavaEnvironment, launching::LAUNCHER_LANGUAGE};
use axlauncher_core::domain::normalize_game_language;
use axlauncher_core::state::{SettingsOrigin, ThemeChoice, update_settings};
use axlauncher_core::storage::DataDir;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, Button, ButtonVariants, Context, Disableable, ElementId, Entity,
    GroupBox, Hsla, Input, InputEvent, InputState, IntoElement, Label, Switch, Window, h_flex,
    v_flex,
};

use crate::{describe_origin, load_at, palette_for};

use super::RootView;

pub(super) struct EnvironmentState {
    java: JavaEnvironment,
    game_language: Entity<InputState>,
    error: Option<String>,
    notice: Option<String>,
}

impl EnvironmentState {
    pub(super) fn new(
        root: Option<&Path>,
        game_language: &str,
        window: &mut Window,
        cx: &mut Context<'_, RootView>,
    ) -> Self {
        let game_language = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(format!("Follow launcher ({LAUNCHER_LANGUAGE})"))
                .default_value(game_language.to_owned())
        });
        cx.subscribe(&game_language, RootView::game_language_changed)
            .detach();
        Self {
            java: JavaEnvironment::load(root),
            game_language,
            error: None,
            notice: None,
        }
    }

    pub(super) fn set_game_language(
        &self,
        value: String,
        window: &mut Window,
        cx: &mut Context<'_, RootView>,
    ) {
        self.game_language
            .update(cx, |input, cx| input.set_value(value, window, cx));
    }

    /// Reads the environment and the choices recorded under the panel's root.
    ///
    /// The root comes from the caller rather than a copy kept here: the panel
    /// has one root, and a second one would drift from it as soon as the user
    /// applies a different directory.
    pub(super) fn refresh(&mut self, root: Option<&Path>) {
        self.java = JavaEnvironment::load(root);
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

/// One line per major the user recorded, so a launch's runtime is visible.
fn chosen_rows(chosen: &[(u32, PathBuf)], muted: Hsla) -> Vec<AnyElement> {
    if chosen.is_empty() {
        return vec![
            Label::new("Chosen Java: none")
                .text_xs()
                .text_color(muted)
                .into_any_element(),
        ];
    }
    chosen
        .iter()
        .map(|(major, path)| {
            Label::new(format!("Java {major}: {}", path.display()))
                .text_xs()
                .text_color(muted)
                .into_any_element()
        })
        .collect()
}

impl RootView {
    fn game_language_changed(
        &mut self,
        _: Entity<InputState>,
        event: &InputEvent,
        cx: &mut Context<'_, Self>,
    ) {
        if matches!(event, InputEvent::Change) {
            self.environment.notice = None;
            self.environment.error = None;
            cx.notify();
        }
    }

    pub(super) fn render_environment(&mut self, cx: &mut Context<'_, Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let danger = cx.theme().danger;
        let success = cx.theme().success;

        let paths = self.environment_paths(muted);
        let read_only = self.settings_readonly();
        let language = self.environment.game_language.read(cx).value();
        let save_disabled = read_only
            || self.settings.as_ref().is_some_and(|settings| {
                settings.theme == self.theme && language.trim() == settings.default_game_language
            });

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
                    .child(self.settings_controls(cx, read_only, save_disabled))
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

    fn settings_controls(
        &self,
        cx: &mut Context<'_, Self>,
        read_only: bool,
        save_disabled: bool,
    ) -> AnyElement {
        h_flex()
            .gap_4()
            .flex_wrap()
            .items_end()
            .child(
                Switch::new("theme-switch")
                    .checked(self.theme == ThemeChoice::Light)
                    .label("Light theme")
                    .on_change(cx.listener(|this, light, _window, cx| {
                        this.toggle_theme(*light, cx);
                    })),
            )
            .child(
                v_flex()
                    .gap_1()
                    .w_48()
                    .child(Label::new("Default game language").text_sm())
                    .child(
                        Input::new(&self.environment.game_language)
                            .id("default-game-language")
                            .aria_label("Default game language for new instances")
                            .disabled(read_only),
                    ),
            )
            .child(
                Button::new("settings-save")
                    .label("Save settings")
                    .disabled(save_disabled)
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.save_settings(window, cx);
                    })),
            )
            .into_any_element()
    }

    fn java_rows(&self, muted: Hsla) -> Vec<AnyElement> {
        let java_home = self
            .environment
            .java
            .java_home()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "not set".to_string());
        let default_java = self
            .environment
            .java
            .default_java()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "not found".to_string());

        let mut rows = vec![
            Label::new(format!("JAVA_HOME: {java_home}"))
                .text_xs()
                .text_color(muted)
                .into_any_element(),
            Label::new(format!("Default Java: {default_java}"))
                .text_xs()
                .text_color(muted)
                .into_any_element(),
        ];
        // A chosen runtime decides a launch, so the user must be able to see
        // it here: the rows above describe the environment, which a choice
        // overrides for its major.
        rows.extend(chosen_rows(self.environment.java.chosen(), muted));
        rows
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

    pub(super) fn save_settings(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let Some(root) = self.root.clone() else {
            self.environment.error = Some("Choose a data directory first.".to_string());
            cx.notify();
            return;
        };
        let Some(settings) = self.settings.as_ref() else {
            self.environment.error = Some("Settings could not be loaded.".to_string());
            cx.notify();
            return;
        };

        let raw_language = self.environment.game_language.read(cx).value();
        let language_changed = raw_language.trim() != settings.default_game_language;
        let language = normalize_game_language(&raw_language);
        if language_changed && language.is_none() {
            self.environment.error = Some("Enter a Minecraft language code such as zh_cn, or leave it empty to follow the launcher.".into());
            self.environment.notice = None;
            cx.notify();
            return;
        }
        let theme = (settings.theme != self.theme).then_some(self.theme);
        let language = language.filter(|_| language_changed);
        let saved = update_settings(&root, |data| {
            data.theme = theme.unwrap_or(data.theme);
            data.default_game_language =
                language.unwrap_or_else(|| std::mem::take(&mut data.default_game_language));
        });
        match saved {
            Ok(data) => {
                self.environment
                    .set_game_language(data.default_game_language.clone(), window, cx);
                self.theme = data.theme;
                palette_for(self.theme).apply(cx);
                self.settings = Some(data);
                self.origin = Some(SettingsOrigin::Stored);
                self.download_debug
                    .reload_sources(self.settings.as_ref(), window, cx);
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
