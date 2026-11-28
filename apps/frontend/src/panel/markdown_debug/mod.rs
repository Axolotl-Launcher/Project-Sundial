// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The Debug page's Markdown editor and preview.

use axlauncher_core::desktop::modrinth::project_image;
use axlauncher_markdown::Markdown;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, Axis, Button, Context, DragMoveEvent, Entity, FocusHandle, GroupBox, HandleEdge,
    IntoElement, KeyDownEvent, Label, MouseButton, MouseDownEvent, Pixels, Render, Sizable,
    TextViewState, Textarea, TextareaState, Window, div, h_flex, relative, rems, resize_handle,
    v_flex,
};

const PANE_BASIS_REMS: f32 = 16.;
const DEFAULT_HEIGHT: f32 = 21.;
const MIN_HEIGHT: f32 = 16.;
const MAX_HEIGHT: f32 = 48.;
const KEYBOARD_STEP: f32 = 2.;

pub(super) struct MarkdownDebug {
    source: Entity<TextareaState>,
    preview: Option<Entity<TextViewState>>,
    markdown: Markdown,
    height: f32,
    resize_focus: FocusHandle,
    resize_start: Option<ResizeStart>,
}

struct ResizeStart {
    height: f32,
    pointer_y: Pixels,
}

struct ResizeMarkdown;

impl Render for ResizeMarkdown {
    fn render(&mut self, _: &mut Window, _: &mut Context<'_, Self>) -> impl IntoElement {
        div()
    }
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
            height: DEFAULT_HEIGHT,
            resize_focus: cx.focus_handle().tab_stop(true),
            resize_start: None,
        }
    }

    fn render_document(&mut self, cx: &mut Context<'_, Self>) {
        let source = self.source.read(cx).value();
        self.preview = (!source.trim().is_empty()).then(|| self.markdown.parse(&source, cx));
        cx.notify();
    }

    fn render_source(&self, cx: &Context<'_, Self>) -> impl IntoElement + use<> {
        let header = h_flex()
            .w_full()
            .justify_between()
            .gap_2()
            .child(Label::new("Markdown source").text_sm())
            .child(
                Button::new("render-markdown")
                    .small()
                    .label("Render")
                    .on_click(cx.listener(|view, _, _, cx| view.render_document(cx))),
            );
        let editor = Textarea::new(&self.source)
            .accessibility_id("markdown-source")
            .aria_label("Markdown source")
            .w_full()
            .h(relative(1.));
        pane(header, editor)
    }

    fn render_preview(&self, cx: &Context<'_, Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let preview = div()
            .id("markdown-preview")
            .size_full()
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
            });
        pane(Label::new("Preview").text_sm(), preview)
    }

    fn resize_handle(&self, cx: &Context<'_, Self>) -> impl IntoElement + use<> {
        let ring = cx.theme().ring;
        div().relative().h_3().child(
            resize_handle("markdown-height", Axis::Vertical)
                .inside(HandleEdge::Trailing)
                .track_focus(&self.resize_focus)
                .role(axlauncher_ui::Role::Splitter)
                .aria_label("Markdown area height")
                .focus_visible(move |style| style.bg(ring))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|view, event: &MouseDownEvent, _, _| {
                        view.resize_start = Some(ResizeStart {
                            height: view.height,
                            pointer_y: event.position.y,
                        });
                    }),
                )
                .on_drag(ResizeMarkdown, |_, _, _, cx| cx.new(|_| ResizeMarkdown))
                .on_drag_move(cx.listener(Self::resize_drag))
                .on_key_down(cx.listener(Self::resize_key)),
        )
    }

    fn resize_drag(
        &mut self,
        event: &DragMoveEvent<ResizeMarkdown>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if let Some(start) = &self.resize_start {
            let delta = (event.event.position.y - start.pointer_y) / window.rem_size();
            self.set_height(start.height + delta, cx);
        }
    }

    fn resize_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<'_, Self>) {
        let height = match event.keystroke.key.as_str() {
            "up" => self.height - KEYBOARD_STEP,
            "down" => self.height + KEYBOARD_STEP,
            "home" => MIN_HEIGHT,
            "end" => MAX_HEIGHT,
            _ => return,
        };
        self.set_height(height, cx);
        cx.stop_propagation();
    }

    fn set_height(&mut self, height: f32, cx: &mut Context<'_, Self>) {
        let height = height.clamp(MIN_HEIGHT, MAX_HEIGHT);
        if self.height != height {
            self.height = height;
            cx.notify();
        }
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
                    .h(rems(self.height))
                    .items_stretch()
                    .content_stretch()
                    .flex_wrap()
                    .gap_4()
                    .child(self.render_source(cx))
                    .child(self.render_preview(cx)),
            )
            .child(self.resize_handle(cx))
    }
}

fn pane(header: impl IntoElement, body: impl IntoElement) -> impl IntoElement {
    // Keep the flex lines equal when the panes wrap, regardless of their contents.
    div()
        .relative()
        .min_w_0()
        .min_h_0()
        .flex_basis(rems(PANE_BASIS_REMS))
        .flex_grow_1()
        .child(
            v_flex()
                .absolute()
                .inset_0()
                .gap_2()
                .child(h_flex().h_8().flex_shrink_0().child(header))
                .child(div().flex_1().min_h_0().child(body)),
        )
}
