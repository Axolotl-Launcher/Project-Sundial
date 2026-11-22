// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Axolotl Launcher frontend: the development panel and its window wiring.
//!
//! This crate owns the root view platform shells open, and is the only crate
//! that combines `axlauncher_core` with `axlauncher_ui`. State lives here in
//! GPUI entities; components stay stateless recipes.

use std::path::PathBuf;

use axlauncher_core::state::{
    Instance, SettingsData, SettingsOrigin, ThemeChoice, discover, load_settings,
};
use axlauncher_core::storage::DataDir;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{Bounds, Palette, WindowBounds, WindowOptions, kit, px, size};

mod panel;

const WINDOW_WIDTH: f32 = 1080.;
const WINDOW_HEIGHT: f32 = 820.;
const WINDOW_MIN_WIDTH: f32 = 720.;
const WINDOW_MIN_HEIGHT: f32 = 560.;

/// Overrides the data directory; the platform data directory is used otherwise.
const DATA_DIR_ENV: &str = "AXLAUNCHER_DATA_DIR";

/// Directory name under the platform data directory.
const APP_DIR: &str = "AXLauncher";

/// What the panel reads once at startup and again on every reload.
pub(crate) struct Loaded {
    pub(crate) root: Option<PathBuf>,
    pub(crate) settings: Option<SettingsData>,
    pub(crate) origin: Option<SettingsOrigin>,
    pub(crate) instances: Vec<Instance>,
    pub(crate) instances_error: Option<String>,
    pub(crate) notice: Option<String>,
}

pub(crate) struct Startup {
    pub(crate) palette: Palette,
    pub(crate) loaded: Loaded,
}

impl Startup {
    pub(crate) fn load() -> Self {
        let loaded = load_at(data_root());
        let palette = loaded
            .settings
            .as_ref()
            .map(|settings| palette_for(settings.theme))
            .unwrap_or(Palette::Dark);
        Self { palette, loaded }
    }
}

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
            window_min_size: Some(size(px(WINDOW_MIN_WIDTH), px(WINDOW_MIN_HEIGHT))),
            ..Default::default()
        },
        cx,
        move |window, cx| cx.new(|cx| panel::RootView::new(startup, window, cx)),
    )
    .expect("failed to open the Axolotl Launcher window");
}

/// The launcher's data root: the override when set, else the platform data
/// directory plus the application name.
fn data_root() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os(DATA_DIR_ENV)
        && !path.is_empty()
    {
        return Some(PathBuf::from(path));
    }
    dirs::data_dir().map(|base| base.join(APP_DIR))
}

/// Reads settings and instances for `root`; `None` means the platform could
/// not name a data directory.
pub(crate) fn load_at(root: Option<PathBuf>) -> Loaded {
    let Some(root) = root else {
        return Loaded {
            root: None,
            settings: None,
            origin: None,
            instances: Vec::new(),
            instances_error: None,
            notice: Some(format!(
                "Cannot determine the data directory. Set {DATA_DIR_ENV}, or choose one below."
            )),
        };
    };

    let (settings, origin, notice) = match load_settings(&root) {
        Ok(loaded) => {
            let notice = settings_notice(&loaded.origin);
            (Some(loaded.data), Some(loaded.origin), notice)
        }
        Err(error) => (None, None, Some(error.to_string())),
    };

    let (instances, instances_error) = match discover(&DataDir::new(&root).instances()) {
        Ok(instances) => (instances, None),
        Err(error) => (Vec::new(), Some(error.to_string())),
    };

    Loaded {
        root: Some(root),
        settings,
        origin,
        instances,
        instances_error,
        notice,
    }
}

pub(crate) fn palette_for(theme: ThemeChoice) -> Palette {
    match theme {
        ThemeChoice::Dark => Palette::Dark,
        ThemeChoice::Light => Palette::Light,
    }
}

/// Text for the one settings outcome a user needs to know about at startup.
fn settings_notice(origin: &SettingsOrigin) -> Option<String> {
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

/// A short description of where the loaded settings came from.
pub(crate) fn describe_origin(origin: &SettingsOrigin) -> String {
    match origin {
        SettingsOrigin::Created => "created with defaults".to_string(),
        SettingsOrigin::Stored => "loaded from disk".to_string(),
        SettingsOrigin::Quarantined { path } => {
            format!("unreadable copy moved to {}", path.display())
        }
        SettingsOrigin::ReadOnlyOtherSchema { found } => {
            format!("schema {found} left untouched")
        }
    }
}
