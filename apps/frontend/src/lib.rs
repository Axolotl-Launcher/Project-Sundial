// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Axolotl Launcher frontend: application state, layout and window wiring.
//!
//! This crate owns the root view platform shells open, and is the only crate
//! that combines `axlauncher_core` (domain) with `axlauncher_ui` (kit
//! vocabulary + palette). State lives here in GPUI entities; components stay
//! stateless recipes.

use std::path::{Path, PathBuf};

use axlauncher_core::BRAND_NAME;
use axlauncher_core::state::{
    Instance, SettingsOrigin, ThemeChoice, create, discover, load_settings,
};
use axlauncher_core::storage::DataDir;
use axlauncher_ui::{
    ActiveTheme, Button, ButtonVariants, ElementId, GroupBox, GroupBoxVariants, Hsla, Label,
    Palette, StyledExt,
};
use axlauncher_ui::{
    Bounds, Context, IntoElement, Render, Window, WindowBounds, WindowOptions, div, px, size,
};
use axlauncher_ui::{kit, prelude::*};

const WINDOW_WIDTH: f32 = 720.;
const WINDOW_HEIGHT: f32 = 480.;

/// Overrides the data directory; the platform data directory is used otherwise.
const DATA_DIR_ENV: &str = "AXLAUNCHER_DATA_DIR";

/// Directory name under the platform data directory.
const APP_DIR: &str = "AXLauncher";

/// How many numbered names to try before giving up on creating an instance.
const MAX_INSTANCE_ATTEMPTS: u32 = 100;

/// Opens the Axolotl Launcher window and runs the GPUI event loop.
///
/// A platform shell calls this once from `main`; it never returns.
pub fn run() {
    let startup = Startup::load();

    kit::application()
        .with_assets(kit::assets::Assets)
        .run(move |cx| {
            startup.palette.install(cx);
            open_main_window(cx, startup);
        });
}

fn open_main_window(cx: &mut kit::App, startup: Startup) {
    let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);

    kit::open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        cx,
        |_, cx| cx.new(|_| RootView::new(startup)),
    )
    .expect("failed to open the Axolotl Launcher window");
}

/// What the window needs, read once before the event loop starts.
struct Startup {
    palette: Palette,
    notice: Option<String>,
    root: Option<PathBuf>,
    instances: Instances,
}

impl Startup {
    fn load() -> Self {
        let Some(root) = data_root() else {
            return Self {
                palette: Palette::Dark,
                notice: Some(format!(
                    "Cannot determine the data directory. Set {DATA_DIR_ENV} to choose one."
                )),
                root: None,
                instances: Instances::unavailable(),
            };
        };

        let (palette, notice) = match load_settings(&root) {
            Ok(loaded) => (palette_for(loaded.data.theme), notice_for(&loaded.origin)),
            Err(error) => (Palette::Dark, Some(error.to_string())),
        };

        Self {
            palette,
            notice,
            instances: Instances::load(&root),
            root: Some(root),
        }
    }
}

/// The launcher's data root: the override when set, else the platform data
/// directory plus the application name.
fn data_root() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(DATA_DIR_ENV) {
        return Some(PathBuf::from(path));
    }
    dirs::data_dir().map(|base| base.join(APP_DIR))
}

fn palette_for(theme: ThemeChoice) -> Palette {
    match theme {
        ThemeChoice::Dark => Palette::Dark,
        ThemeChoice::Light => Palette::Light,
    }
}

/// Text for the one settings outcome a user needs to know about.
fn notice_for(origin: &SettingsOrigin) -> Option<String> {
    match origin {
        SettingsOrigin::Created | SettingsOrigin::Stored => None,
        SettingsOrigin::Quarantined { path } => Some(format!(
            "settings.json was unreadable; moved to {} and defaults applied.",
            path.display()
        )),
        SettingsOrigin::ReadOnlyOtherSchema { found } => Some(format!(
            "settings.json uses schema {found}; this build writes schema 1, so defaults are in \
             use and the file was left untouched."
        )),
    }
}

/// The first free `instance-N` id and its display name.
fn next_instance(root: &Path) -> Option<(String, String)> {
    for number in 1..=MAX_INSTANCE_ATTEMPTS {
        let id = format!("instance-{number}");
        if !root.join(&id).exists() {
            return Some((id, format!("Instance {number}")));
        }
    }
    None
}

struct RootView {
    root: Option<PathBuf>,
    notice: Option<String>,
    instances: Instances,
}

impl RootView {
    fn new(startup: Startup) -> Self {
        Self {
            root: startup.root,
            notice: startup.notice,
            instances: startup.instances,
        }
    }

    fn create_instance(&mut self, cx: &mut Context<'_, Self>) {
        let Some(root) = self.root.clone() else {
            return;
        };
        let Some((id, name)) = next_instance(&root) else {
            self.notice = Some("Cannot find a free instance name.".to_string());
            cx.notify();
            return;
        };

        let instance = Instance {
            id,
            name,
            game_version: None,
            loader: None,
        };

        match create(&root, &instance) {
            Ok(()) => {
                self.instances = Instances::load(&root);
                self.notice = None;
            }
            Err(error) => self.notice = Some(error.to_string()),
        }
        cx.notify();
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
            .when_some(self.notice.clone(), |view, notice| {
                view.child(Label::new(notice).text_sm().text_color(muted))
            })
            .child(self.instances.panel(muted, cx))
    }
}

struct Instances {
    root: PathBuf,
    items: Vec<Instance>,
    error: Option<String>,
}

impl Instances {
    fn load(root: &Path) -> Self {
        let directory = DataDir::new(root).instances();

        match discover(&directory) {
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

    fn unavailable() -> Self {
        Self {
            root: PathBuf::new(),
            items: Vec::new(),
            error: None,
        }
    }
}

impl Instances {
    fn panel(&self, muted: Hsla, cx: &mut Context<'_, RootView>) -> impl IntoElement {
        let heading = match self.items.len() {
            0 => "Instances".to_string(),
            count => format!("Instances · {count}"),
        };

        GroupBox::new()
            .outline()
            .title(Label::new(heading).text_sm().text_color(muted))
            .child(
                div()
                    .v_flex()
                    .gap_3()
                    .child(Label::new(self.root_label()).text_xs().text_color(muted))
                    .child(
                        div().h_flex().gap_3().child(
                            Button::new("create-instance")
                                .primary()
                                .label("New instance")
                                .on_click(cx.listener(|view, _, _, cx| view.create_instance(cx))),
                        ),
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

    fn root_label(&self) -> String {
        if self.root.as_os_str().is_empty() {
            "data directory unknown".to_string()
        } else {
            self.root.display().to_string()
        }
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
            Label::new("axlauncher_core + gpui-kit")
                .text_sm()
                .text_color(muted),
        )
}
