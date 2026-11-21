// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! Superlogical frontend: application state, layout and window wiring.
//!
//! This crate owns the root view platform shells open, and is the only crate
//! that combines `superlogical_core` (domain) with `superlogical_ui` (kit
//! vocabulary + palette). State lives here in GPUI entities; components stay
//! stateless recipes.

use superlogical_core::{BRAND_NAME, Counter};
use superlogical_ui::{
    ActiveTheme, Button, ButtonVariants, Disableable, GroupBox, GroupBoxVariants, Label, Separator,
    StyledExt,
};
use superlogical_ui::{
    Bounds, Context, IntoElement, Render, Window, WindowBounds, WindowOptions, div, px, size,
};
use superlogical_ui::{kit, prelude::*};

/// Opens the Superlogical window and runs the GPUI event loop.
///
/// A platform shell calls this once from `main`; it never returns.
pub fn run() {
    kit::application()
        .with_assets(kit::assets::Assets)
        .run(|cx| {
            superlogical_ui::Palette::Dark.install(cx);

            kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(720.), px(480.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                cx,
                |_, cx| cx.new(RootView::new),
            )
            .expect("failed to open the Superlogical window");
        });
}

/// The application root: owns state and lays components out.
struct RootView {
    counter: Counter,
}

impl RootView {
    fn new(_cx: &mut Context<'_, Self>) -> Self {
        Self {
            counter: Counter::default(),
        }
    }
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let theme = cx.theme();
        let background = theme.background;
        let muted = theme.muted_foreground;
        let value = self.counter.value();

        div()
            .v_flex()
            .size_full()
            .gap_6()
            .p_8()
            .bg(background)
            .child(
                div()
                    .v_flex()
                    .gap_1()
                    .child(Label::new(BRAND_NAME).text_xl())
                    .child(Label::new("superlogical_core + gpui-kit").text_sm()),
            )
            .child(
                GroupBox::new()
                    .outline()
                    .title(Label::new("Counter").text_sm())
                    .child(
                        div()
                            .v_flex()
                            .items_center()
                            .gap_4()
                            .child(Label::new(value.to_string()).text_3xl())
                            .child(Separator::horizontal())
                            .child(
                                div()
                                    .h_flex()
                                    .gap_3()
                                    .child(
                                        Button::new("decrement")
                                            .secondary()
                                            .label("Decrement")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.counter.decrement();
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Button::new("increment")
                                            .primary()
                                            .label("Increment")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.counter.increment();
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        Button::new("reset")
                                            .ghost()
                                            .label("Reset")
                                            .disabled(value == 0)
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.counter.reset();
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    ),
            )
            .child(
                div().text_color(muted).child(
                    Label::new("State lives in superlogical_core; rendering comes from gpui-kit.")
                        .text_xs(),
                ),
            )
    }
}
