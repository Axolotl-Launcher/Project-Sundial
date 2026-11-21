//! Superlogical UI, built on gpui (Zed's GPU-accelerated UI framework).
//!
//! This crate only exposes the window and its view. Platform shells in
//! `platforms/` own the executable entry point.

use gpui::{
    App, Application, Bounds, Context, FontWeight, Render, SharedString, TitlebarOptions, Window,
    WindowBounds, WindowOptions, div, prelude::*, px, rgb, size,
};
use superlogical_core::{BRAND_NAME, Counter};

/// Opens the Superlogical window and runs the gpui event loop.
pub fn run() {
    Application::new().run(|cx: &mut App| {
        cx.open_window(
            WindowOptions {
                titlebar: Some(TitlebarOptions {
                    title: Some(SharedString::from(BRAND_NAME)),
                    appears_transparent: false,
                    ..Default::default()
                }),
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(720.), px(480.)),
                    cx,
                ))),
                ..Default::default()
            },
            |_, cx| cx.new(|_| RootView::new()),
        )
        .expect("failed to open the Superlogical window");

        cx.activate(true);
    });
}

/// Root view of the application.
struct RootView {
    counter: Counter,
}

impl RootView {
    fn new() -> Self {
        Self {
            counter: Counter::default(),
        }
    }
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let value = self.counter.value();

        let button = |id: &'static str, label: &'static str, background: u32| {
            div()
                .id(id)
                .flex()
                .items_center()
                .justify_center()
                .h_10()
                .px_4()
                .rounded_md()
                .bg(rgb(background))
                .text_color(rgb(0xffffff))
                .cursor_pointer()
                .child(label)
        };

        div()
            .flex()
            .flex_col()
            .size_full()
            .gap_6()
            .p_8()
            .bg(rgb(0x0f0f12))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(0xffffff))
                            .child(BRAND_NAME),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x9a9aa5))
                            .child("gpui 0.2.2 · core + ui"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_4()
                    .p_6()
                    .rounded_lg()
                    .border_1()
                    .border_color(rgb(0x2a2a31))
                    .bg(rgb(0x1b1b1f))
                    .shadow_md()
                    .child(
                        div()
                            .text_sm()
                            .text_color(rgb(0x9a9aa5))
                            .child("Counter value"),
                    )
                    .child(
                        div()
                            .text_3xl()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(0xffffff))
                            .child(value.to_string()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap_3()
                            .child(button("decrement", "Decrement", 0x2a2a31).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.counter.decrement();
                                    cx.notify();
                                }),
                            ))
                            .child(button("increment", "Increment", 0x2f6feb).on_click(
                                cx.listener(|this, _, _, cx| {
                                    this.counter.increment();
                                    cx.notify();
                                }),
                            ))
                            .child(button("reset", "Reset", 0x2a2a31).on_click(cx.listener(
                                |this, _, _, cx| {
                                    this.counter.reset();
                                    cx.notify();
                                },
                            ))),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(0x6f6f7a))
                    .child("State lives in superlogical_core; the shell only renders it."),
            )
    }
}
