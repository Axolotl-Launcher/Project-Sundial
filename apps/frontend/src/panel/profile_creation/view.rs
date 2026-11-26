// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, Button, ButtonVariants, Context, Disableable, Div, Input, IntoElement,
    Label, Select, SharedString, h_flex, rems, v_flex,
};

use super::{CatalogStatus, LoaderNotice, ProfileCreation, RootView};

impl RootView {
    pub(in crate::panel) fn render_profile_creation(
        &self,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let state = &self.profile_creation;
        let disabled = self.paths.is_none();
        let has_loader = state.loader_kind(cx).is_some();
        let name = Input::new(&state.name)
            .id("new-profile-name")
            .aria_label("Instance name")
            .disabled(disabled);
        let loader = Select::new(&state.loader)
            .placeholder("Choose a loader")
            .accessibility_label("Mod loader")
            .disabled(disabled);
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_3()
                    .items_end()
                    .flex_wrap()
                    .child(field("profile-name-label", "Instance name", name).w_40())
                    .child(
                        field(
                            "profile-minecraft-label",
                            "Minecraft version",
                            state.minecraft_input(disabled),
                        )
                        .w_48(),
                    )
                    .child(field("profile-loader-label", "Mod loader", loader).w_32())
                    .when(has_loader, |row| {
                        row.child(
                            field(
                                "profile-loader-version-label",
                                "Loader version",
                                state.build_input(disabled),
                            )
                            .w_48(),
                        )
                    })
                    .child(
                        Button::new("new-profile-create")
                            .label("Create")
                            .primary()
                            .disabled(disabled || state.profile(cx).is_none())
                            .on_click(cx.listener(|view, _, _, cx| view.submit_profile_form(cx))),
                    ),
            )
            .when(!state.manual, |column| {
                column.child(self.catalog_feedback(cx))
            })
            .child(self.version_mode_button(disabled, cx))
            .into_any_element()
    }

    fn version_mode_button(&self, disabled: bool, cx: &mut Context<'_, Self>) -> Button {
        let label = if self.profile_creation.manual {
            "Browse versions"
        } else {
            "Enter versions manually"
        };
        Button::new("new-profile-version-mode")
            .self_start()
            .label(label)
            .ghost()
            .disabled(disabled)
            .on_click(cx.listener(|view, _, window, cx| view.toggle_manual_versions(window, cx)))
    }

    fn catalog_feedback(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let state = &self.profile_creation;
        let mut feedback = v_flex().gap_1();
        if let Some(status) = Self::minecraft_feedback(&state.minecraft_status, cx) {
            feedback = feedback.child(status);
        }
        if let Some(status) = Self::loader_feedback(&state.loader_catalog, cx) {
            feedback = feedback.child(status);
        }
        feedback.into_any_element()
    }

    fn minecraft_feedback(
        status: &CatalogStatus,
        cx: &mut Context<'_, Self>,
    ) -> Option<AnyElement> {
        match status {
            CatalogStatus::Empty => Some(
                Label::new("Mojang returned an empty version list.")
                    .text_sm()
                    .into_any_element(),
            ),
            CatalogStatus::Failed(error) => Some(retry_row(
                "minecraft-versions-error",
                format!("Could not load Minecraft versions. {error}"),
                Button::new("minecraft-versions-retry")
                    .label("Retry Minecraft versions")
                    .on_click(
                        cx.listener(|view, _, window, cx| view.load_minecraft_versions(window, cx)),
                    ),
                cx,
            )),
            _ => None,
        }
    }

    fn loader_feedback(
        catalog: &super::LoaderCatalog,
        cx: &mut Context<'_, Self>,
    ) -> Option<AnyElement> {
        match catalog.notice() {
            Some(LoaderNotice::Empty) => Some(
                Label::new("This loader has no builds for the selected Minecraft version.")
                    .text_sm()
                    .into_any_element(),
            ),
            Some(LoaderNotice::Failed(error)) => Some(retry_row(
                "loader-versions-error",
                format!("Could not load loader versions. {error}"),
                Button::new("loader-versions-retry")
                    .label("Retry loader versions")
                    .on_click(
                        cx.listener(|view, _, window, cx| view.load_loader_versions(window, cx)),
                    ),
                cx,
            )),
            _ => None,
        }
    }
}

impl ProfileCreation {
    fn minecraft_input(&self, disabled: bool) -> AnyElement {
        if self.manual {
            return Input::new(&self.manual_game)
                .id("new-profile-minecraft-manual")
                .aria_label("Minecraft version")
                .disabled(disabled)
                .into_any_element();
        }
        Select::new(&self.minecraft)
            .accessibility_label("Minecraft version")
            .placeholder(placeholder(&self.minecraft_status, "Choose a version"))
            .search_placeholder("Search versions or types")
            .menu_width(rems(20.))
            .disabled(disabled || !matches!(self.minecraft_status, CatalogStatus::Ready))
            .into_any_element()
    }

    fn build_input(&self, disabled: bool) -> AnyElement {
        if self.manual {
            return Input::new(&self.manual_build)
                .id("new-profile-loader-manual")
                .aria_label("Loader version")
                .disabled(disabled)
                .into_any_element();
        }
        Select::new(&self.loader_version)
            .accessibility_label("Loader version")
            .placeholder(self.loader_catalog.placeholder("Choose Minecraft first"))
            .search_placeholder("Search loader versions")
            .menu_width(rems(17.))
            .disabled(disabled || !self.loader_catalog.is_ready())
            .into_any_element()
    }
}

fn field(label_id: &'static str, label: &'static str, control: impl IntoElement) -> Div {
    v_flex()
        .gap_1()
        .child(Label::new(label).id(label_id).text_sm())
        .child(control)
}

fn placeholder(status: &CatalogStatus, idle: &'static str) -> SharedString {
    match status {
        CatalogStatus::Loading => "Loading versions…",
        CatalogStatus::Empty => "No versions available",
        CatalogStatus::Failed(_) => "Versions unavailable",
        _ => idle,
    }
    .into()
}

fn retry_row(
    label_id: &'static str,
    message: String,
    button: Button,
    cx: &Context<'_, RootView>,
) -> AnyElement {
    h_flex()
        .gap_2()
        .items_center()
        .flex_wrap()
        .child(
            Label::new(message)
                .id(label_id)
                .text_sm()
                .text_color(cx.theme().danger),
        )
        .child(button.ghost())
        .into_any_element()
}
