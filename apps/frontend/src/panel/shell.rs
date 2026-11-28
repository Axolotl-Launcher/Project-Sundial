// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The navigation sidebar and scrollable page layout.

use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, Button, ButtonVariants, Context, Disableable, ElementId, GroupBox,
    Icon, IconName, IntoElement, Label, Render, ScrollableElement, Sidebar, SidebarMenu, Sizable,
    StyledExt, Window, div, h_flex, v_flex,
};

use crate::i18n::tr;

use crate::installations::Installation;

use super::RootView;

const MAX_VISIBLE_INSTALLATIONS: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Page {
    Home,
    Instances,
    Settings,
    Debug,
}

impl Page {
    fn label(self, cx: &axlauncher_ui::App) -> &'static str {
        match self {
            Self::Home => tr(cx, "navigation.home"),
            Self::Instances => tr(cx, "navigation.instances"),
            Self::Settings => tr(cx, "navigation.settings"),
            Self::Debug => tr(cx, "navigation.debug"),
        }
    }

    fn icon(self) -> IconName {
        match self {
            Self::Home => IconName::LayoutDashboard,
            Self::Instances => IconName::Folder,
            Self::Settings => IconName::Settings,
            Self::Debug => IconName::SquareTerminal,
        }
    }
}

impl RootView {
    fn render_sidebar(&self, cx: &Context<'_, Self>) -> Sidebar<SidebarMenu> {
        let collapsed = self.sidebar_collapsed;
        let toggle_label = if collapsed {
            tr(cx, "navigation.sidebar.expand")
        } else {
            tr(cx, "navigation.sidebar.collapse")
        };
        let toggle = sidebar_button(
            "toggle-sidebar",
            IconName::Menu,
            toggle_label,
            (!collapsed).then_some("Axolotl"),
        )
        .tooltip(toggle_label)
        .on_click(cx.listener(|view, _, _, cx| {
            view.sidebar_collapsed = !view.sidebar_collapsed;
            cx.notify();
        }));

        // The Button controls keep both groups keyboard-operable.
        Sidebar::new("main-sidebar")
            .w_40()
            .collapsed(collapsed)
            .header(
                v_flex()
                    .w_full()
                    .gap_1()
                    .child(toggle)
                    .child(self.navigation_button(Page::Home, cx))
                    .child(self.navigation_button(Page::Instances, cx)),
            )
            .footer(
                v_flex()
                    .w_full()
                    .gap_1()
                    .child(self.navigation_button(Page::Settings, cx))
                    .child(self.navigation_button(Page::Debug, cx)),
            )
    }

    fn navigation_button(&self, page: Page, cx: &Context<'_, Self>) -> Button {
        let label = page.label(cx);
        sidebar_button(
            ElementId::named_usize("navigate", page as usize),
            page.icon(),
            label,
            (!self.sidebar_collapsed).then_some(label),
        )
        .toggled(self.page == page)
        .on_click(cx.listener(move |view, _, _, cx| {
            view.page = page;
            cx.notify();
        }))
    }

    fn render_header(&self, cx: &Context<'_, Self>) -> impl IntoElement + use<> {
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .justify_between()
                    .child(Label::new(self.page.label(cx)).text_xl())
                    .child(
                        Button::new("reload-all")
                            .label(tr(cx, "navigation.reload"))
                            .ghost()
                            .disabled(self.paths.is_none())
                            .on_click(cx.listener(|view, _, window, cx| view.reload(window, cx))),
                    ),
            )
            .when_some(
                self.notice.clone().or_else(|| {
                    self.origin
                        .as_ref()
                        .and_then(|origin| crate::settings_notice(origin, cx))
                }),
                |header, notice| {
                    header.child(
                        Label::new(notice)
                            .text_xs()
                            .text_color(cx.theme().muted_foreground),
                    )
                },
            )
    }

    fn page_sections(&mut self, cx: &mut Context<'_, Self>) -> Vec<AnyElement> {
        match self.page {
            Page::Home => vec![self.accounts.clone().into_any_element()],
            Page::Instances => vec![self.render_instances(cx)],
            Page::Settings => vec![
                self.render_environment(self.render_language_picker(cx), cx),
                self.render_memory(cx),
                self.java.clone().into_any_element(),
                self.render_service_sources(cx),
            ],
            Page::Debug => vec![
                self.render_launch(cx),
                self.render_download_debug(cx),
                self.render_activity(cx),
            ],
        }
    }
    fn render_download_debug(&self, cx: &Context<'_, Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        GroupBox::new()
            .id("download-debug")
            .title(
                Label::new(tr(cx, "navigation.download-debug"))
                    .text_sm()
                    .text_color(muted),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(self.download_debug.clone())
                    .children(self.render_modpack_progress(cx))
                    .children(self.render_installation_progress(cx)),
            )
            .into_any_element()
    }

    fn render_modpack_progress(&self, cx: &Context<'_, Self>) -> Option<Label> {
        let (name, progress) = self.modpack.read(cx).install_progress()?;
        Some(
            Label::new(format!("{name} · {}", progress.message(cx)))
                .id(ElementId::Name("download-progress:modpack".into()))
                .text_sm()
                .text_color(cx.theme().muted_foreground),
        )
    }

    fn render_installation_progress(&self, cx: &Context<'_, Self>) -> Vec<AnyElement> {
        let Some(root) = self.paths.as_ref().map(|paths| paths.game()) else {
            return Vec::new();
        };
        let Some(versions) = self.installations.get(root.root()) else {
            return Vec::new();
        };
        let mut active: Vec<_> = versions
            .iter()
            .filter(|(_, state)| matches!(state, Installation::Running(_)))
            .collect();
        active.sort_unstable_by_key(|(version, _)| version.as_str());
        active
            .into_iter()
            .take(MAX_VISIBLE_INSTALLATIONS)
            .map(|(version, state)| {
                Label::new(format!("{version} · {}", state.message(cx)))
                    .id(ElementId::Name(
                        format!("download-progress:{version}").into(),
                    ))
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .into_any_element()
            })
            .collect()
    }
}

impl Render for RootView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        h_flex()
            .size_full()
            .bg(cx.theme().background)
            .child(self.render_sidebar(cx))
            .child(
                v_flex()
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .gap_4()
                    .child(div().px_6().pt_6().child(self.render_header(cx)))
                    .child(
                        div()
                            .v_flex()
                            .flex_1()
                            .min_h_0()
                            .px_6()
                            .pb_6()
                            .gap_5()
                            .overflow_y_scrollbar()
                            .id(ElementId::named_usize("page-scroll", self.page as usize))
                            .children(self.page_sections(cx)),
                    ),
            )
    }
}

fn sidebar_button(
    id: impl Into<ElementId>,
    icon: IconName,
    label: &'static str,
    text: Option<&'static str>,
) -> Button {
    let button = Button::new(id)
        .small()
        .w_full()
        .ghost()
        .accessibility_label(label);
    match text {
        Some(text) => button.child(
            h_flex()
                .w_full()
                .gap_2()
                .child(Icon::new(icon).size_4().flex_shrink_0())
                .child(text),
        ),
        None => button.icon(icon).tooltip(label),
    }
}
