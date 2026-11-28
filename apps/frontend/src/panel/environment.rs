// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Data root, paths and settings.

use std::path::Path;

use axlauncher_core::domain::normalize_game_language;
use axlauncher_core::state::{SettingsOrigin, ThemeChoice, update_settings};
use axlauncher_core::storage::{DataDir, DataPath, Directory, Document};
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, App, Button, ButtonVariants, Context, Disableable, ElementId, Entity,
    GroupBox, Hsla, Input, InputEvent, InputState, IntoElement, Label, SharedString, Switch,
    Window, h_flex, v_flex,
};

use crate::i18n::{interpolate, language, tr};
use crate::{describe_origin, load_at, palette_for};

use super::RootView;

pub(super) struct EnvironmentState {
    game_language: Entity<InputState>,
    error: Option<String>,
    notice: Option<String>,
}

impl EnvironmentState {
    fn new(game_language: Entity<InputState>) -> Self {
        Self {
            game_language,
            error: None,
            notice: None,
        }
    }

    pub(super) fn set_game_language(&self, value: String, window: &mut Window, cx: &mut App) {
        self.game_language
            .update(cx, |input, cx| input.set_value(value, window, cx));
    }

    pub(super) fn sync_saved_game_language(
        &self,
        previous: &str,
        saved: &str,
        window: &mut Window,
        cx: &mut App,
    ) {
        if previous != saved && self.language(cx).as_ref() == previous {
            self.set_game_language(saved.to_owned(), window, cx);
        }
    }

    pub(super) fn refresh_language(&self, window: &mut Window, cx: &mut App) {
        let placeholder = game_language_placeholder(cx);
        self.game_language.update(cx, |input, cx| {
            input.set_placeholder(placeholder, window, cx);
        });
    }

    pub(super) fn clear_feedback(&mut self) {
        self.error = None;
        self.notice = None;
    }

    fn language(&self, cx: &App) -> SharedString {
        self.game_language.read(cx).value()
    }

    fn language_input(&self, read_only: bool, cx: &App) -> Input {
        Input::new(&self.game_language)
            .id("default-game-language")
            .aria_label(tr(cx, "settings.game-language.accessibility"))
            .disabled(read_only)
    }

    fn feedback(&self, cx: &App) -> impl Iterator<Item = Label> + use<> {
        [
            self.notice
                .clone()
                .map(|notice| Label::new(notice).text_xs().text_color(cx.theme().success)),
            self.error
                .clone()
                .map(|error| Label::new(error).text_xs().text_color(cx.theme().danger)),
        ]
        .into_iter()
        .flatten()
    }

    pub(super) fn show_error(&mut self, message: impl Into<String>) {
        self.error = Some(message.into());
    }

    fn show_saved(&mut self, cx: &App) {
        self.error = None;
        self.notice = Some(tr(cx, "settings.saved").into());
    }
}

fn game_language_placeholder(cx: &App) -> String {
    interpolate(
        tr(cx, "settings.game-language.follow-launcher"),
        &[("code", language(cx).minecraft_code())],
    )
}

fn path_row(index: usize, label: &str, path: &std::path::Path, muted: Hsla) -> AnyElement {
    h_flex()
        .items_start()
        .gap_4()
        .child(
            Label::new(label)
                .id(ElementId::named_usize("environment-label", index))
                .w_32()
                .flex_shrink_0()
                .text_xs()
                .text_color(muted),
        )
        .child(
            Label::new(path.display().to_string())
                .id(ElementId::named_usize("environment-value", index))
                .flex_1()
                .min_w_0()
                .text_xs()
                .text_color(muted),
        )
        .into_any_element()
}

impl RootView {
    pub(super) fn new_environment(
        game_language: &str,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> EnvironmentState {
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder(game_language_placeholder(cx))
                .default_value(game_language.to_owned())
        });
        cx.subscribe(&input, Self::game_language_changed).detach();
        EnvironmentState::new(input)
    }

    fn game_language_changed(
        &mut self,
        _: Entity<InputState>,
        event: &InputEvent,
        cx: &mut Context<'_, Self>,
    ) {
        if matches!(event, InputEvent::Change) {
            self.environment.clear_feedback();
            cx.notify();
        }
    }

    pub(super) fn render_environment(
        &mut self,
        language_picker: impl IntoElement,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let paths = self.environment_paths(muted, cx);
        let read_only = self.settings_readonly();
        let language = self.environment.language(cx);
        let save_disabled = read_only
            || self.settings.as_ref().is_some_and(|settings| {
                settings.theme == self.theme && language.trim() == settings.default_game_language
            });

        GroupBox::new()
            .id("environment")
            .title(
                Label::new(tr(cx, "settings.title"))
                    .text_sm()
                    .text_color(muted),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(language_picker)
                    .child(self.root_controls(cx))
                    .children(paths)
                    .child(self.settings_controls(cx, read_only, save_disabled))
                    .child(
                        Label::new(format!("settings.json: {}", self.settings_origin(cx)))
                            .text_xs()
                            .text_color(muted),
                    )
                    .children(self.environment.feedback(cx))
                    .when(self.paths.is_none(), |column| {
                        column.child(
                            Label::new(tr(cx, "settings.data-directory.hint"))
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
                    .aria_label(tr(cx, "settings.data-directory.label"))
                    .min_w_0()
                    .flex_1(),
            )
            .child(
                Button::new("environment-apply")
                    .label(tr(cx, "settings.data-directory.apply"))
                    .primary()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.apply_root(window, cx);
                    })),
            )
            .into_any_element()
    }

    fn environment_paths(&self, muted: Hsla, cx: &App) -> Vec<AnyElement> {
        let Some(data) = &self.paths else {
            return Vec::new();
        };
        let game = data.game();
        [
            (tr(cx, "settings.paths.root"), data.path(DataPath::Root)),
            (
                tr(cx, "settings.paths.profiles"),
                data.path(DataPath::Profiles),
            ),
            (tr(cx, "settings.paths.game"), game.root()),
            (tr(cx, "settings.paths.versions"), game.versions()),
            (tr(cx, "settings.paths.libraries"), game.libraries()),
            (tr(cx, "settings.paths.assets"), game.assets()),
            (
                tr(cx, "settings.paths.settings"),
                &Document::Settings.path(data.path(DataPath::Root)),
            ),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (label, path))| path_row(index, label, path, muted))
        .collect()
    }

    fn settings_controls(
        &self,
        cx: &mut Context<'_, Self>,
        read_only: bool,
        save_disabled: bool,
    ) -> AnyElement {
        v_flex()
            .gap_3()
            .child(
                Switch::new("theme-switch")
                    .self_start()
                    .checked(self.theme == ThemeChoice::Light)
                    .label(tr(cx, "settings.theme.light"))
                    .on_change(cx.listener(|this, light, _window, cx| {
                        this.toggle_theme(*light, cx);
                    })),
            )
            .child(
                h_flex()
                    .gap_3()
                    .flex_wrap()
                    .items_end()
                    .child(
                        v_flex()
                            .gap_1()
                            .w_48()
                            .child(Label::new(tr(cx, "settings.game-language.label")).text_sm())
                            .child(self.environment.language_input(read_only, cx)),
                    )
                    .child(
                        Button::new("settings-save")
                            .label(tr(cx, "settings.save"))
                            .disabled(save_disabled)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.save_settings(window, cx);
                            })),
                    ),
            )
            .into_any_element()
    }

    fn settings_origin(&self, cx: &App) -> String {
        self.origin
            .as_ref()
            .map(|origin| describe_origin(origin, cx))
            .unwrap_or_else(|| tr(cx, "settings.origin.not-loaded").to_string())
    }

    pub(super) fn apply_root(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let text = self.root_input.read(cx).value().trim().to_string();
        let loaded = match self.load_selected_root(Path::new(&text)) {
            Ok(loaded) => loaded,
            Err(error) => {
                self.environment.show_error(error);
                cx.notify();
                return;
            }
        };
        self.apply_loaded(loaded, window, cx);
        self.record_data_root();
        cx.notify();
    }

    fn load_selected_root(&self, root: &Path) -> Result<crate::Loaded, String> {
        let paths = match self.paths.clone() {
            Some(mut paths) => paths
                .set_directory(Directory::AppData, Some(root))
                .map(|()| paths),
            None => DataDir::isolated(root),
        }
        .map_err(|error| error.to_string())?;
        let loaded = load_at(paths);
        if loaded.settings.is_none() {
            return Err(loaded
                .notice
                .expect("failed settings load reports an error"));
        }
        Ok(loaded)
    }

    pub(super) fn save_settings(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let Some(paths) = self.paths.as_ref() else {
            self.environment
                .show_error(tr(cx, "settings.data-directory.required"));
            cx.notify();
            return;
        };
        let Some(settings) = self.settings.as_ref() else {
            self.environment.show_error(tr(cx, "settings.load-error"));
            cx.notify();
            return;
        };

        let raw_language = self.environment.language(cx);
        let language_changed = raw_language.trim() != settings.default_game_language;
        let language = normalize_game_language(&raw_language);
        if language_changed && language.is_none() {
            self.environment.clear_feedback();
            self.environment
                .show_error(tr(cx, "settings.game-language.invalid"));
            cx.notify();
            return;
        }
        let theme = (settings.theme != self.theme).then_some(self.theme);
        let language = language.filter(|_| language_changed);
        let saved = update_settings(paths.path(DataPath::Root), |data| {
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
                self.set_settings(Some(data), cx);
                self.origin = Some(SettingsOrigin::Stored);
                self.reload_sources(window, cx);
                self.environment.show_saved(cx);
                self.activity.success("Saved settings");
            }
            Err(error) => {
                let message = error.to_string();
                self.environment.show_error(message.clone());
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
        self.environment.clear_feedback();
        cx.notify();
    }
}
