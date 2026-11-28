// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The Debug page's Markdown editor and preview.

use axlauncher_core::desktop::modrinth::project_image;
use axlauncher_markdown::Markdown;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, Button, Context, Entity, GroupBox, IntoElement, Label, Render, TextViewState,
    Textarea, TextareaState, Window, div, h_flex, rems, v_flex,
};

const PANE_BASIS_REMS: f32 = 16.;

pub(super) struct MarkdownDebug {
    source: Entity<TextareaState>,
    preview: Option<Entity<TextViewState>>,
    markdown: Markdown,
}

impl MarkdownDebug {
    pub(super) fn new(window: &mut Window, cx: &mut Context<'_, Self>) -> Self {
        let source = cx.new(|cx| {
            TextareaState::new(window, cx).placeholder("Type or paste a Markdown document")
        });
        Self {
            source,
            preview: None,
            markdown: Markdown::new(|uri| project_image(uri).ok()),
        }
    }

    fn render_document(&mut self, cx: &mut Context<'_, Self>) {
        let source = self.source.read(cx).value();
        self.preview = (!source.trim().is_empty()).then(|| self.markdown.parse(&source, cx));
        cx.notify();
    }

    fn render_source(&self, cx: &Context<'_, Self>) -> impl IntoElement + use<> {
        v_flex()
            .min_w_0()
            .flex_basis(rems(PANE_BASIS_REMS))
            .flex_grow_1()
            .gap_2()
            .child(Label::new("Markdown source").text_sm())
            .child(
                Textarea::new(&self.source)
                    .accessibility_id("markdown-source")
                    .aria_label("Markdown source")
                    .w_full()
                    .h_72(),
            )
            .child(
                h_flex().child(
                    Button::new("render-markdown")
                        .label("Render")
                        .on_click(cx.listener(|view, _, _, cx| view.render_document(cx))),
                ),
            )
    }

    fn render_preview(&self, cx: &Context<'_, Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        v_flex()
            .min_w_0()
            .flex_basis(rems(PANE_BASIS_REMS))
            .flex_grow_1()
            .gap_2()
            .child(Label::new("Preview").text_sm())
            .child(
                div()
                    .id("markdown-preview")
                    .h_72()
                    .overflow_hidden()
                    .border_1()
                    .border_color(theme.border)
                    .rounded(theme.radius)
                    .map(|panel| match &self.preview {
                        Some(preview) => {
                            panel.child(self.markdown.text_view(preview).scrollable(true).p_3())
                        }
                        None => panel.p_3().child(
                            Label::new("No Markdown rendered")
                                .text_sm()
                                .text_color(theme.muted_foreground),
                        ),
                    }),
            )
    }
}

impl Render for MarkdownDebug {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        GroupBox::new()
            .id("markdown-debug")
            .flex_shrink_0()
            .title(Label::new("Markdown debug").text_sm())
            .child(
                h_flex()
                    .items_start()
                    .flex_wrap()
                    .gap_4()
                    .child(self.render_source(cx))
                    .child(self.render_preview(cx)),
            )
    }
}
