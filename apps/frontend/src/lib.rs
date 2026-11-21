//! Superlogical frontend: application state, layout and window wiring.
//!
//! This crate owns the [`RootView`] that platform shells open, and is the only
//! crate that combines `superlogical_core` (domain) with `superlogical_ui`
//! (components). Components stay stateless; state lives here in gpui entities.

use gpui::{
    App, Application, Bounds, Context, IntoElement, Render, SharedString, TitlebarOptions, Window,
    WindowBounds, WindowOptions, div, prelude::*, px, size,
};
use superlogical_core::{BRAND_NAME, Counter};
use superlogical_ui::{Button, ButtonVariant, Card, Divider, Label, TextSize, Theme};

/// Opens the Superlogical window and runs the gpui event loop.
///
/// A platform shell calls this once from `main`; it never returns.
pub fn run() {
    Application::new().run(|cx: &mut App| {
        Theme::dark().install(cx);

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
            |_, cx| cx.new(RootView::new),
        )
        .expect("failed to open the Superlogical window");

        cx.activate(true);
    });
}

/// The application root: owns state and lays components out.
struct RootView {
    counter: Counter,
}

impl RootView {
    fn new(_cx: &mut Context<Self>) -> Self {
        Self {
            counter: Counter::default(),
        }
    }
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = *Theme::global(cx);
        let value = self.counter.value();

        div()
            .flex()
            .flex_col()
            .size_full()
            .gap_6()
            .p_8()
            .bg(theme.canvas)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        Label::new(BRAND_NAME)
                            .size(TextSize::Title)
                            .weight(gpui::FontWeight::SEMIBOLD),
                    )
                    .child(
                        Label::new("superlogical_core + superlogical_ui")
                            .size(TextSize::Small)
                            .color(theme.text_muted),
                    ),
            )
            .child(
                Card::new().title("Counter").child(
                    div()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap_4()
                        .child(
                            Label::new(value.to_string())
                                .size(TextSize::Display)
                                .weight(gpui::FontWeight::SEMIBOLD),
                        )
                        .child(Divider)
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap_3()
                                .child(
                                    Button::new("decrement", "Decrement")
                                        .variant(ButtonVariant::Secondary)
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.counter.decrement();
                                            cx.notify();
                                        })),
                                )
                                .child(Button::new("increment", "Increment").on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.counter.increment();
                                        cx.notify();
                                    },
                                )))
                                .child(
                                    Button::new("reset", "Reset")
                                        .variant(ButtonVariant::Ghost)
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
                Label::new("State lives in superlogical_core; superlogical_ui only draws.")
                    .size(TextSize::Small)
                    .color(theme.text_muted),
            )
    }
}
