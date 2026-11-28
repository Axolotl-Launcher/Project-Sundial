// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, App, Button, ButtonVariants, Context, Disableable, Div, Input,
    IntoElement, Label, Render, Select, SharedString, Window, h_flex, rems, v_flex,
};

use crate::i18n::tr;

use super::{CatalogStatus, LoaderCatalog, ProfileCreation};

enum LoaderNotice<'a> {
    Empty,
    Failed(&'a str),
}

impl CatalogStatus {
    fn placeholder(&self, cx: &App, idle: &'static str) -> SharedString {
        match self {
            Self::Loading => tr(cx, "instances.create.status.loading"),
            Self::Empty => tr(cx, "instances.create.status.empty"),
            Self::Failed(_) => tr(cx, "instances.create.status.failed"),
            Self::Idle | Self::Ready => idle,
        }
        .into()
    }
}

impl LoaderCatalog {
    fn notice(&self) -> Option<LoaderNotice<'_>> {
        match &self.status {
            CatalogStatus::Empty => Some(LoaderNotice::Empty),
            CatalogStatus::Failed(error) => Some(LoaderNotice::Failed(error)),
            _ => None,
        }
    }

    fn placeholder(&self, cx: &App) -> SharedString {
        self.status
            .placeholder(cx, tr(cx, "instances.create.minecraft.choose-first"))
    }
}

impl Render for ProfileCreation {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let disabled = !self.enabled;
        let has_loader = self.loader_kind(cx).is_some();
        let name = Input::new(&self.name)
            .id("new-profile-name")
            .aria_label(tr(cx, "instances.create.name"))
            .disabled(disabled);
        let loader = Select::new(&self.loader)
            .placeholder(tr(cx, "instances.create.loader.choose"))
            .accessibility_label(tr(cx, "instances.create.loader"))
            .disabled(disabled);
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_3()
                    .items_end()
                    .flex_wrap()
                    .child(
                        field("profile-name-label", tr(cx, "instances.create.name"), name).w_40(),
                    )
                    .child(
                        field(
                            "profile-minecraft-label",
                            tr(cx, "instances.create.minecraft.label"),
                            self.minecraft_input(disabled, cx),
                        )
                        .w_48(),
                    )
                    .child(
                        field(
                            "profile-loader-label",
                            tr(cx, "instances.create.loader"),
                            loader,
                        )
                        .w_32(),
                    )
                    .when(has_loader, |row| {
                        row.child(
                            field(
                                "profile-loader-version-label",
                                tr(cx, "instances.create.loader.version"),
                                self.build_input(disabled, cx),
                            )
                            .w_48(),
                        )
                    })
                    .child(
                        Button::new("new-profile-create")
                            .label(tr(cx, "instances.create.submit"))
                            .primary()
                            .disabled(disabled || self.profile(cx).is_none())
                            .on_click(cx.listener(|form, _, _, cx| form.submit(cx))),
                    ),
            )
            .when(!self.manual, |column| {
                column.child(self.catalog_feedback(cx))
            })
            .child(self.version_mode_button(disabled, cx))
    }
}

impl ProfileCreation {
    fn version_mode_button(&self, disabled: bool, cx: &mut Context<'_, Self>) -> Button {
        let label = if self.manual {
            tr(cx, "instances.create.browse")
        } else {
            tr(cx, "instances.create.manual")
        };
        Button::new("new-profile-version-mode")
            .self_start()
            .label(label)
            .ghost()
            .disabled(disabled)
            .on_click(cx.listener(|view, _, window, cx| view.toggle_manual_versions(window, cx)))
    }

    fn catalog_feedback(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let mut feedback = v_flex().gap_1();
        if let Some(status) = Self::minecraft_feedback(&self.minecraft_status, cx) {
            feedback = feedback.child(status);
        }
        if let Some(status) = Self::loader_feedback(&self.loader_catalog, cx) {
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
                Label::new(tr(cx, "instances.create.minecraft.empty"))
                    .text_sm()
                    .into_any_element(),
            ),
            CatalogStatus::Failed(error) => Some(retry_row(
                "minecraft-versions-error",
                format!("Could not load Minecraft versions. {error}"),
                Button::new("minecraft-versions-retry")
                    .label(tr(cx, "instances.create.minecraft.retry"))
                    .on_click(
                        cx.listener(|view, _, window, cx| view.load_minecraft_versions(window, cx)),
                    ),
                cx,
            )),
            _ => None,
        }
    }

    fn loader_feedback(catalog: &LoaderCatalog, cx: &mut Context<'_, Self>) -> Option<AnyElement> {
        match catalog.notice() {
            Some(LoaderNotice::Empty) => Some(
                Label::new(tr(cx, "instances.create.loader.empty"))
                    .text_sm()
                    .into_any_element(),
            ),
            Some(LoaderNotice::Failed(error)) => Some(retry_row(
                "loader-versions-error",
                format!("Could not load loader versions. {error}"),
                Button::new("loader-versions-retry")
                    .label(tr(cx, "instances.create.loader.retry"))
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
    fn minecraft_input(&self, disabled: bool, cx: &App) -> AnyElement {
        if self.manual {
            return Input::new(&self.manual_game)
                .id("new-profile-minecraft-manual")
                .aria_label(tr(cx, "instances.create.minecraft.label"))
                .disabled(disabled)
                .into_any_element();
        }
        Select::new(&self.minecraft)
            .accessibility_label(tr(cx, "instances.create.minecraft.label"))
            .placeholder(
                self.minecraft_status
                    .placeholder(cx, tr(cx, "instances.create.minecraft.choose")),
            )
            .search_placeholder(tr(cx, "instances.create.minecraft.search"))
            .menu_width(rems(20.))
            .disabled(disabled || !matches!(self.minecraft_status, CatalogStatus::Ready))
            .into_any_element()
    }

    fn build_input(&self, disabled: bool, cx: &App) -> AnyElement {
        if self.manual {
            return Input::new(&self.manual_build)
                .id("new-profile-loader-manual")
                .aria_label(tr(cx, "instances.create.loader.version"))
                .disabled(disabled)
                .into_any_element();
        }
        Select::new(&self.loader_version)
            .accessibility_label(tr(cx, "instances.create.loader.version"))
            .placeholder(self.loader_catalog.placeholder(cx))
            .search_placeholder(tr(cx, "instances.create.loader.search"))
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

fn retry_row(label_id: &'static str, message: String, button: Button, cx: &App) -> AnyElement {
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
