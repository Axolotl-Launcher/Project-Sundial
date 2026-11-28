// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use axlauncher_core::state::{LauncherLanguage, SettingsOrigin, update_settings};
use axlauncher_core::storage::DataPath;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, Context, Entity, IndexPath, IntoElement, Label, Select, SelectEvent, SelectItem,
    SelectState, SharedString, Window, rems, v_flex,
};

use crate::i18n::{interpolate, language, set_language, tr};

use super::RootView;

#[derive(Clone, Copy)]
pub(super) struct LanguageOption(LauncherLanguage);

impl SelectItem for LanguageOption {
    type Value = LauncherLanguage;

    fn title(&self) -> SharedString {
        match self.0 {
            LauncherLanguage::English => "English",
            LauncherLanguage::SimplifiedChinese => "简体中文",
        }
        .into()
    }

    fn value(&self) -> &LauncherLanguage {
        &self.0
    }
}

const OPTIONS: [LanguageOption; 2] = [
    LanguageOption(LauncherLanguage::English),
    LanguageOption(LauncherLanguage::SimplifiedChinese),
];

pub(super) type LanguagePicker = Entity<SelectState<Vec<LanguageOption>>>;

impl RootView {
    pub(super) fn new_language_picker(
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> LanguagePicker {
        let index = OPTIONS.iter().position(|option| option.0 == language(cx));
        let picker =
            cx.new(|cx| SelectState::new(OPTIONS.to_vec(), index.map(IndexPath::new), window, cx));
        cx.subscribe_in(&picker, window, Self::launcher_language_confirmed)
            .detach();
        picker
    }

    fn launcher_language_confirmed(
        &mut self,
        _: &LanguagePicker,
        event: &SelectEvent<Vec<LanguageOption>>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let SelectEvent::Confirm(Some(selected)) = event else {
            return;
        };
        self.save_launcher_language(*selected, window, cx);
    }

    pub(super) fn render_language_picker(
        &self,
        cx: &Context<'_, Self>,
        read_only: bool,
    ) -> impl IntoElement + use<> {
        let label = tr(cx, "settings.launcher-language.label");
        v_flex()
            .gap_1()
            .child(Label::new(label).text_sm())
            .child(
                Select::new(&self.language_picker)
                    .id("launcher-language")
                    .accessibility_label(label)
                    .w_48()
                    .menu_width(rems(12.))
                    .disabled(read_only),
            )
            .child(
                Label::new(tr(cx, "settings.launcher-language.description"))
                    .text_xs()
                    .text_color(cx.theme().muted_foreground),
            )
    }

    fn save_launcher_language(
        &mut self,
        selected: LauncherLanguage,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.settings_readonly() || language(cx) == selected {
            return;
        }
        let Some((paths, settings)) = self.paths.as_ref().zip(self.settings.as_mut()) else {
            return;
        };
        match update_settings(paths.path(DataPath::Root), |data| {
            data.launcher_language = selected
        }) {
            Ok(_) => {
                settings.launcher_language = selected;
                self.origin = Some(SettingsOrigin::Stored);
                self.environment.clear_feedback();
                self.apply_launcher_language(selected, window, cx);
            }
            Err(error) => {
                self.apply_launcher_language(language(cx), window, cx);
                self.environment.show_error(interpolate(
                    tr(cx, "settings.launcher-language.save-error"),
                    &[("error", &error.to_string())],
                ));
            }
        }
        cx.notify();
    }

    pub(super) fn apply_launcher_language(
        &mut self,
        selected: LauncherLanguage,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.sync_language_picker(selected, window, cx);
        if language(cx) == selected {
            return;
        }
        set_language(selected, cx);
        let root_placeholder = tr(cx, "settings.data-directory.label");
        self.root_input.update(cx, |input, cx| {
            input.set_placeholder(root_placeholder, window, cx);
        });
        self.environment.refresh_language(window, cx);
        self.download_sources.refresh_language(window, cx);
    }

    fn sync_language_picker(
        &self,
        selected: LauncherLanguage,
        window: &mut Window,
        cx: &mut axlauncher_ui::App,
    ) {
        if self.language_picker.read(cx).selected_value() == Some(&selected) {
            return;
        }
        self.language_picker.update(cx, |picker, cx| {
            picker.set_selected_value(&selected, window, cx);
            cx.notify();
        });
    }
}
