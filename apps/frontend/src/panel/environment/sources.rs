// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::path::Path;

use axlauncher_core::state::{
    SettingsData, SettingsError, SettingsOrigin, SourcePreference, update_settings,
};
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, App, Context, Disableable, Entity, GroupBox, IndexPath, IntoElement,
    Label, Select, SelectEvent, SelectItem, SelectState, SharedString, Switch, Window, h_flex,
    v_flex,
};

use super::RootView;

#[derive(Clone, Copy)]
struct SourceOption(SourcePreference, &'static str);

impl SelectItem for SourceOption {
    type Value = SourcePreference;

    fn title(&self) -> SharedString {
        self.1.into()
    }

    fn value(&self) -> &SourcePreference {
        &self.0
    }
}

type SourceSelect = Entity<SelectState<Vec<SourceOption>>>;

#[derive(Clone, Copy)]
enum SourceField {
    Metadata,
    Files,
    MavenCentral,
}

impl SourceField {
    fn read(self, settings: Option<&SettingsData>) -> SourcePreference {
        settings.map_or(SourcePreference::default(), |settings| match self {
            Self::Metadata => settings.minecraft_metadata_source,
            Self::Files => settings.minecraft_file_source,
            Self::MavenCentral => settings.maven_central_source,
        })
    }

    fn save(
        self,
        root: &Path,
        preference: SourcePreference,
    ) -> Result<SettingsData, SettingsError> {
        update_settings(root, |settings| match self {
            Self::Metadata => settings.minecraft_metadata_source = preference,
            Self::Files => settings.minecraft_file_source = preference,
            Self::MavenCentral => settings.maven_central_source = preference,
        })
    }

    fn select(
        self,
        settings: Option<&SettingsData>,
        window: &mut Window,
        cx: &mut App,
    ) -> SourceSelect {
        let value = self.read(settings);
        let options = vec![
            SourceOption(SourcePreference::Auto, "Automatic"),
            SourceOption(SourcePreference::MirrorPreferred, "Trust built-in mirror"),
            SourceOption(SourcePreference::OfficialOnly, "Official only"),
        ];
        let index = options.iter().position(|option| option.0 == value);
        cx.new(|cx| SelectState::new(options, index.map(IndexPath::new), window, cx))
    }
}

pub(super) struct DownloadSources {
    metadata: SourceSelect,
    files: SourceSelect,
    maven_central: SourceSelect,
    pub(super) error: Option<String>,
}

impl DownloadSources {
    pub(super) fn new(settings: Option<&SettingsData>, window: &mut Window, cx: &mut App) -> Self {
        Self {
            metadata: SourceField::Metadata.select(settings, window, cx),
            files: SourceField::Files.select(settings, window, cx),
            maven_central: SourceField::MavenCentral.select(settings, window, cx),
            error: None,
        }
    }

    pub(super) fn sync(&self, settings: Option<&SettingsData>, window: &mut Window, cx: &mut App) {
        for (field, select) in self.selectors() {
            sync_select(select, field.read(settings), window, cx);
        }
    }

    fn selectors(&self) -> [(SourceField, &SourceSelect); 3] {
        [
            (SourceField::Metadata, &self.metadata),
            (SourceField::Files, &self.files),
            (SourceField::MavenCentral, &self.maven_central),
        ]
    }
}

fn sync_select(select: &SourceSelect, value: SourcePreference, window: &mut Window, cx: &mut App) {
    if select.read(cx).selected_value() == Some(&value) {
        return;
    }
    select.update(cx, |select, cx| {
        select.set_selected_value(&value, window, cx);
        cx.notify();
    });
}

impl RootView {
    pub(super) fn subscribe_sources(&self, window: &mut Window, cx: &mut Context<'_, Self>) {
        for (field, select) in self.environment.sources.selectors() {
            Self::subscribe_source(field, select, window, cx);
        }
    }

    fn subscribe_source(
        field: SourceField,
        select: &SourceSelect,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        cx.subscribe_in(select, window, move |view, _, event, window, cx| {
            view.source_confirmed(field, event, window, cx);
        })
        .detach();
    }

    fn source_confirmed(
        &mut self,
        field: SourceField,
        event: &SelectEvent<Vec<SourceOption>>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if let SelectEvent::Confirm(Some(value)) = event {
            self.save_download_source(field, *value, window, cx);
        }
    }

    fn save_download_source(
        &mut self,
        field: SourceField,
        value: SourcePreference,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(paths) = self.paths.as_ref().filter(|_| !self.settings_readonly()) else {
            self.environment
                .reload_sources(self.settings.as_ref(), window, cx);
            return;
        };
        if field.read(self.settings.as_ref()) == value {
            self.environment.sources.error = None;
            cx.notify();
            return;
        }

        let saved = field.save(paths.root(), value);
        self.finish_source_save(saved, window, cx);
    }

    fn save_fallen_proxy(
        &mut self,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(paths) = self.paths.as_ref().filter(|_| !self.settings_readonly()) else {
            return;
        };
        let saved = update_settings(paths.root(), |settings| settings.use_fallen_proxy = enabled);
        if saved.is_ok() {
            self.invalidate_launch_preview();
        }
        self.finish_source_save(saved, window, cx);
    }

    fn finish_source_save(
        &mut self,
        saved: Result<SettingsData, SettingsError>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let error = match saved {
            Ok(settings) => {
                self.replace_settings(Some(settings), cx);
                self.origin = Some(SettingsOrigin::Stored);
                None
            }
            Err(error) => Some(format!("Could not save the source preference. {error}")),
        };
        self.environment
            .reload_sources(self.settings.as_ref(), window, cx);
        self.environment.sources.error = error;
        cx.notify();
    }

    pub(in crate::panel) fn render_service_sources(
        &self,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let sources = &self.environment.sources;
        let disabled = self.settings_readonly();
        GroupBox::new()
            .id("service-sources")
            .title(
                Label::new("Service sources")
                    .text_sm()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_4()
                            .child(source_select(
                                "Minecraft metadata source",
                                "download-metadata-source",
                                &sources.metadata,
                                disabled,
                            ))
                            .child(source_select(
                                "Minecraft file source",
                                "download-file-source",
                                &sources.files,
                                disabled,
                            ))
                            .child(source_select(
                                "Maven Central source",
                                "maven-central-source",
                                &sources.maven_central,
                                disabled,
                            )),
                    )
                    .child(self.render_fallen_proxy(cx, disabled))
                    .when_some(sources.error.clone(), |column, error| {
                        column.child(
                            Label::new(error)
                                .id("download-source-error")
                                .text_xs()
                                .text_color(cx.theme().danger),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_fallen_proxy(&self, cx: &mut Context<'_, Self>, disabled: bool) -> impl IntoElement {
        v_flex()
            .gap_1()
            .pt_2()
            .child(
                Switch::new("fallen-proxy")
                    .label("Use Fallen-Proxy for Minecraft authentication")
                    .checked(self.fallen_proxy().is_some())
                    .disabled(disabled)
                    .on_change(cx.listener(|view, enabled, window, cx| {
                        view.save_fallen_proxy(*enabled, window, cx);
                    })),
            )
            .child(
                Label::new("Forwards Minecraft sign-in and in-game session requests through a third-party service.")
                    .id("fallen-proxy-description")
                    .text_xs()
                    .text_color(cx.theme().muted_foreground),
            )
    }
}

fn source_select(
    label: &'static str,
    id: &'static str,
    select: &SourceSelect,
    disabled: bool,
) -> impl IntoElement {
    v_flex()
        .gap_1()
        .w_56()
        .child(Label::new(label).id(format!("{id}-label")).text_sm())
        .child(
            Select::new(select)
                .id(id)
                .accessibility_label(label)
                .disabled(disabled),
        )
}

#[cfg(test)]
#[path = "../../../../../tests/apps/frontend/src/panel/environment/sources.rs"]
mod tests;
