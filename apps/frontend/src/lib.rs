// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Axolotl Launcher frontend: application state, layout and window wiring.
//!
//! This crate owns the root view platform shells open, and is the only crate
//! that combines `axlauncher_core` (domain) with `axlauncher_ui` (kit
//! vocabulary + palette). State lives here in GPUI entities; components stay
//! stateless recipes.

use std::collections::HashMap;
use std::path::PathBuf;

use axlauncher_core::BRAND_NAME;
use axlauncher_core::state::profile::{DiscoveredProfile, discover};
use axlauncher_core::state::{SettingsOrigin, ThemeChoice, load_settings};
use axlauncher_core::storage::DataDir;
use axlauncher_ui::{
    ActiveTheme, Button, ButtonVariants, GroupBox, GroupBoxVariants, Hsla, Label, Palette,
    StyledExt,
};
use axlauncher_ui::{
    Bounds, Context, IntoElement, Render, Window, WindowBounds, WindowOptions, div, px, size,
};
use axlauncher_ui::{kit, prelude::*};

mod installations;
mod instance_row;
use installations::Installation;

const WINDOW_WIDTH: f32 = 720.;
const WINDOW_HEIGHT: f32 = 480.;

/// Overrides the data directory; the platform data directory is used otherwise.
const DATA_DIR_ENV: &str = "AXLAUNCHER_DATA_DIR";

/// Directory name under the platform data directory.
const APP_DIR: &str = "AXLauncher";

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
    instances: Instances,
    game_root: PathBuf,
    concurrency: usize,
}

impl Startup {
    fn load() -> Self {
        let Some(root) = data_root() else {
            return Self {
                palette: Palette::Dark,
                notice: Some(format!(
                    "Cannot determine the data directory. Set {DATA_DIR_ENV} to choose one."
                )),
                instances: Instances::unavailable(),
                game_root: PathBuf::new(),
                concurrency: 0,
            };
        };

        let (palette, notice, concurrency) = match load_settings(&root) {
            Ok(loaded) => (
                palette_for(loaded.data.theme),
                notice_for(&loaded.origin),
                loaded.data.max_concurrent_downloads as usize,
            ),
            Err(error) => (Palette::Dark, Some(error.to_string()), 0),
        };

        let data = DataDir::new(root);
        Self {
            palette,
            notice,
            instances: Instances::load(data.instances()),
            game_root: data.meta(),
            concurrency,
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

struct RootView {
    notice: Option<String>,
    instances: Instances,
    game_root: PathBuf,
    concurrency: usize,
    installations: HashMap<String, Installation>,
}

impl RootView {
    fn new(startup: Startup) -> Self {
        Self {
            notice: startup.notice,
            instances: startup.instances,
            game_root: startup.game_root,
            concurrency: startup.concurrency,
            installations: HashMap::new(),
        }
    }

    fn refresh_instances(&mut self, cx: &mut Context<'_, Self>) {
        self.instances = Instances::load(self.instances.root.clone());
        cx.notify();
    }

    fn install(&mut self, version: &str, cx: &mut Context<'_, Self>) {
        if matches!(
            self.installations.get(version),
            Some(Installation::Running(_) | Installation::Complete)
        ) {
            return;
        }
        self.set_installation(
            version,
            Installation::Running(axlauncher_core::install::prepare::Progress::Version),
            cx,
        );
        installations::start(
            self.game_root.clone(),
            version.to_owned(),
            self.concurrency,
            cx,
        );
    }

    fn set_installation(
        &mut self,
        version: &str,
        installation: Installation,
        cx: &mut Context<'_, Self>,
    ) {
        self.installations.insert(version.to_owned(), installation);
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
            .child(self.instances.panel(&self.installations, muted, cx))
    }
}

struct Instances {
    root: PathBuf,
    items: Vec<DiscoveredProfile>,
    error: Option<String>,
}

impl Instances {
    fn load(directory: PathBuf) -> Self {
        let discovered = discover(&directory);
        Self {
            root: directory,
            items: discovered.instances,
            error: (!discovered.issues.is_empty()).then(|| {
                discovered
                    .issues
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n")
            }),
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
    fn panel(
        &self,
        installations: &HashMap<String, Installation>,
        muted: Hsla,
        cx: &mut Context<'_, RootView>,
    ) -> impl IntoElement {
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
                    .when(!self.root.as_os_str().is_empty(), |panel| {
                        panel.child(
                            div().h_flex().gap_3().child(
                                Button::new("refresh-instances")
                                    .primary()
                                    .label("Refresh")
                                    .on_click(
                                        cx.listener(|view, _, _, cx| view.refresh_instances(cx)),
                                    ),
                            ),
                        )
                    })
                    .when_some(self.error.clone(), |panel, error| {
                        panel.child(Label::new(error).text_sm().text_color(muted))
                    })
                    .when(self.items.is_empty() && self.error.is_none(), |panel| {
                        panel.child(Label::new("No instances yet.").text_sm().text_color(muted))
                    })
                    .children(
                        self.items
                            .iter()
                            .map(|instance| instance_row::row(instance, installations, muted, cx)),
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
