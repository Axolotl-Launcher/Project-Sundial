// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::path::Path;

use axlauncher_core::state::{
    SettingsData, SettingsError, SettingsOrigin, SourcePreference, update_settings,
};
use axlauncher_core::storage::DataPath;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, App, Context, Disableable, Entity, GroupBox, IndexPath, IntoElement,
    Label, Select, SelectEvent, SelectItem, SelectState, SharedString, Switch, Window, h_flex,
    v_flex,
};

use crate::i18n::tr;

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

fn source_options(cx: &App) -> Vec<SourceOption> {
    vec![
        SourceOption(SourcePreference::Auto, tr(cx, "settings.sources.automatic")),
        SourceOption(
            SourcePreference::MirrorPreferred,
            tr(cx, "settings.sources.mirror"),
        ),
        SourceOption(
            SourcePreference::OfficialOnly,
            tr(cx, "settings.sources.official"),
        ),
    ]
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
        let options = source_options(cx);
        let index = options.iter().position(|option| option.0 == value);
        cx.new(|cx| SelectState::new(options, index.map(IndexPath::new), window, cx))
    }
}

pub(in crate::panel) struct DownloadSources {
    metadata: SourceSelect,
    files: SourceSelect,
    maven_central: SourceSelect,
    error: Option<String>,
}

impl DownloadSources {
    fn new(settings: Option<&SettingsData>, window: &mut Window, cx: &mut App) -> Self {
        Self {
            metadata: SourceField::Metadata.select(settings, window, cx),
            files: SourceField::Files.select(settings, window, cx),
            maven_central: SourceField::MavenCentral.select(settings, window, cx),
            error: None,
        }
    }

    pub(super) fn refresh_language(&self, window: &mut Window, cx: &mut App) {
        for (_, select) in self.selectors() {
            refresh_source_options(select, window, cx);
        }
    }

    fn sync(
        &mut self,
        settings: Option<&SettingsData>,
        error: Option<String>,
        window: &mut Window,
        cx: &mut App,
    ) {
        for (field, select) in self.selectors() {
            sync_select(select, field.read(settings), window, cx);
        }
        self.error = error;
    }

    fn selectors(&self) -> [(SourceField, &SourceSelect); 3] {
        [
            (SourceField::Metadata, &self.metadata),
            (SourceField::Files, &self.files),
            (SourceField::MavenCentral, &self.maven_central),
        ]
    }
}

fn refresh_source_options(select: &SourceSelect, window: &mut Window, cx: &mut App) {
    let selection = select.read(cx).selected_value().copied();
    select.update(cx, |state, cx| {
        state.set_items(source_options(cx), window, cx)
    });
    if let Some(selection) = selection {
        sync_select(select, selection, window, cx);
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
    pub(in crate::panel) fn new_download_sources(
        settings: Option<&SettingsData>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> DownloadSources {
        let sources = DownloadSources::new(settings, window, cx);
        for (field, select) in sources.selectors() {
            Self::watch_source(field, select, window, cx);
        }
        sources
    }

    fn watch_source(
        field: SourceField,
        select: &SourceSelect,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        cx.subscribe_in(
            select,
            window,
            move |view, _, event, window, cx| match event {
                SelectEvent::Confirm(Some(value)) => {
                    view.save_download_source(field, *value, window, cx)
                }
                SelectEvent::Confirm(None) => (),
            },
        )
        .detach();
    }

    pub(in crate::panel) fn reload_sources(
        &mut self,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.download_sources
            .sync(self.settings.as_ref(), None, window, cx);
    }

    fn save_download_source(
        &mut self,
        field: SourceField,
        value: SourcePreference,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(paths) = self.paths.as_ref().filter(|_| !self.settings_readonly()) else {
            self.reload_sources(window, cx);
            return;
        };
        if field.read(self.settings.as_ref()) == value {
            self.reload_sources(window, cx);
            cx.notify();
            return;
        }

        let saved = field.save(paths.path(DataPath::Root), value);
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
        let saved = update_settings(paths.path(DataPath::Root), |settings| {
            settings.use_fallen_proxy = enabled;
        });
        if saved.is_ok() {
            self.invalidate_inspection();
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
                self.set_settings(Some(settings), cx);
                self.origin = Some(SettingsOrigin::Stored);
                None
            }
            Err(error) => Some(format!("Could not save the source preference. {error}")),
        };
        self.download_sources
            .sync(self.settings.as_ref(), error, window, cx);
        cx.notify();
    }

    pub(in crate::panel) fn render_service_sources(
        &self,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let sources = &self.download_sources;
        let disabled = self.settings_readonly();
        GroupBox::new()
            .id("service-sources")
            .title(
                Label::new(tr(cx, "settings.sources.title"))
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
                                tr(cx, "settings.sources.metadata"),
                                "download-metadata-source",
                                &sources.metadata,
                                disabled,
                            ))
                            .child(source_select(
                                tr(cx, "settings.sources.files"),
                                "download-file-source",
                                &sources.files,
                                disabled,
                            ))
                            .child(source_select(
                                tr(cx, "settings.sources.maven"),
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
                    .label(tr(cx, "settings.sources.fallen-proxy.label"))
                    .checked(self.fallen_proxy().is_some())
                    .disabled(disabled)
                    .on_change(cx.listener(|view, enabled, window, cx| {
                        view.save_fallen_proxy(*enabled, window, cx);
                    })),
            )
            .child(
                Label::new(tr(cx, "settings.sources.fallen-proxy.description"))
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
#[path = "../../../../tests/apps/frontend/src/panel/service_sources.rs"]
mod tests;
