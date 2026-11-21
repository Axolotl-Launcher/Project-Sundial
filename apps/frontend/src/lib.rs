// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! Superlogical frontend: application state, layout and window wiring.
//!
//! This crate owns the root view platform shells open, and is the only crate
//! that combines `superlogical_core` (domain) with `superlogical_ui` (kit
//! vocabulary + palette). State lives here in GPUI entities; components stay
//! stateless recipes.

use std::path::PathBuf;

use superlogical_core::BRAND_NAME;
use superlogical_core::state::{Instance, discover_instances};
use superlogical_core::storage::DataDir;
use superlogical_ui::{ActiveTheme, ElementId, GroupBox, GroupBoxVariants, Hsla, Label, StyledExt};
use superlogical_ui::{
    Bounds, Context, IntoElement, Render, Window, WindowBounds, WindowOptions, div, px, size,
};
use superlogical_ui::{kit, prelude::*};

const WINDOW_WIDTH: f32 = 720.;
const WINDOW_HEIGHT: f32 = 480.;
const PALETTE: superlogical_ui::Palette = superlogical_ui::Palette::Dark;

/// Overrides the data directory; the platform data directory is used otherwise.
const DATA_DIR_ENV: &str = "SUPERLOGICAL_DATA_DIR";

/// Directory name under the platform data directory.
const APP_DIR: &str = "Superlogical";

/// Opens the Superlogical window and runs the GPUI event loop.
///
/// A platform shell calls this once from `main`; it never returns.
pub fn run() {
    kit::application()
        .with_assets(kit::assets::Assets)
        .run(|cx| {
            PALETTE.install(cx);
            open_main_window(cx);
        });
}

fn open_main_window(cx: &mut kit::App) {
    let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);

    kit::open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        cx,
        |_, cx| cx.new(RootView::new),
    )
    .expect("failed to open the Superlogical window");
}

/// The launcher's data root: the override when set, else the platform data
/// directory plus the application name.
fn data_root() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(DATA_DIR_ENV) {
        return Some(PathBuf::from(path));
    }
    dirs::data_dir().map(|base| base.join(APP_DIR))
}

struct RootView {
    instances: Instances,
}

struct Instances {
    root: PathBuf,
    items: Vec<Instance>,
    error: Option<String>,
}

impl Instances {
    fn load() -> Self {
        let Some(root) = data_root() else {
            return Self {
                root: PathBuf::new(),
                items: Vec::new(),
                error: Some(format!(
                    "Cannot determine the data directory. Set {DATA_DIR_ENV} to choose one."
                )),
            };
        };
        let directory = DataDir::new(&root).instances();

        match discover_instances(&directory) {
            Ok(items) => Self {
                root: directory,
                items,
                error: None,
            },
            Err(error) => Self {
                root: directory,
                items: Vec::new(),
                error: Some(error.to_string()),
            },
        }
    }
}

impl RootView {
    fn new(_cx: &mut Context<'_, Self>) -> Self {
        Self {
            instances: Instances::load(),
        }
    }
}

impl Render for RootView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let theme = cx.theme();
        let background = theme.background;
        let muted = theme.muted_foreground;

        div()
            .v_flex()
            .size_full()
            .gap_6()
            .p_8()
            .bg(background)
            .child(header(muted))
            .child(self.instances.panel(muted))
    }
}

impl Instances {
    fn panel(&self, muted: Hsla) -> impl IntoElement {
        let count = self.items.len();
        let heading = match count {
            0 => "Instances".to_string(),
            1 => "Instances · 1".to_string(),
            other => format!("Instances · {other}"),
        };

        GroupBox::new()
            .outline()
            .title(Label::new(heading).text_sm().text_color(muted))
            .child(
                div()
                    .v_flex()
                    .gap_3()
                    .child(
                        Label::new(self.root.display().to_string())
                            .text_xs()
                            .text_color(muted),
                    )
                    .when_some(self.error.clone(), |panel, error| {
                        panel.child(Label::new(error).text_sm().text_color(muted))
                    })
                    .when(self.items.is_empty() && self.error.is_none(), |panel| {
                        panel.child(
                            Label::new(
                                "No instances yet. Each one is a folder with instance.json.",
                            )
                            .text_sm()
                            .text_color(muted),
                        )
                    })
                    .children(
                        self.items
                            .iter()
                            .enumerate()
                            .map(|(index, instance)| row(index, instance, muted)),
                    ),
            )
    }
}

/// One instance row.
///
/// The labels take explicit ids: `Label::new` defaults its id to the call site,
/// so labels built in a loop would share one id and gpui's accessibility tree
/// panics on the duplicate in debug builds.
fn row(index: usize, instance: &Instance, muted: Hsla) -> impl IntoElement {
    let details = match (&instance.game_version, &instance.loader) {
        (Some(version), Some(loader)) => format!("{version} · {loader}"),
        (Some(version), None) => version.clone(),
        (None, Some(loader)) => loader.clone(),
        (None, None) => "unknown version".to_string(),
    };

    div()
        .v_flex()
        .gap_1()
        .child(
            Label::new(instance.name.clone())
                .id(ElementId::named_usize("instance-name", index))
                .text_base(),
        )
        .child(
            Label::new(format!("{} · {details}", instance.id))
                .id(ElementId::named_usize("instance-detail", index))
                .text_xs()
                .text_color(muted),
        )
}

fn header(muted: Hsla) -> impl IntoElement {
    div()
        .v_flex()
        .gap_1()
        .child(Label::new(BRAND_NAME).text_xl())
        .child(
            Label::new("superlogical_core + gpui-kit")
                .text_sm()
                .text_color(muted),
        )
}
