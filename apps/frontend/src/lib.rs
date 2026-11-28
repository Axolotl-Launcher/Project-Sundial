// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Axolotl Launcher desktop views and window wiring.
//!
//! This crate owns the root view platform shells open, and is the only crate
//! that combines `axlauncher_core` with `axlauncher_ui`. State lives here in
//! GPUI entities; components stay stateless recipes.

use std::cell::Cell;
use std::rc::Rc;
use std::thread::JoinHandle;

use axlauncher_core::BRAND_NAME;
use axlauncher_core::state::profile::{ProfileDiscovery, discover};
use axlauncher_core::state::{SettingsData, SettingsOrigin, ThemeChoice, load_settings};
use axlauncher_core::storage::{DataDir, DataPath};
use axlauncher_ui::prelude::*;
use axlauncher_ui::{Bounds, Palette, TitlebarOptions, WindowBounds, WindowOptions, kit, px, size};

use crate::i18n::tr;

mod file_picker;
mod i18n;
mod installation_progress;
mod installations;
mod launches;
mod modpacks;
mod panel;

use panel::RootView;

// Cargo Shear scans imports inside the crate but misses the mirrored tests.
#[cfg(test)]
use tempfile as test_tempfile;

const WINDOW_WIDTH: f32 = 1080.;
const WINDOW_HEIGHT: f32 = 820.;
const WINDOW_MIN_WIDTH: f32 = 720.;
const WINDOW_MIN_HEIGHT: f32 = 560.;
const APP_ID: &str = "com.axolotl.axlauncher";

/// What the panel reads once at startup and again on every reload.
pub(crate) struct Loaded {
    pub(crate) paths: Option<DataDir>,
    pub(crate) settings: Option<SettingsData>,
    pub(crate) origin: Option<SettingsOrigin>,
    pub(crate) profiles: ProfileDiscovery,
    pub(crate) notice: Option<String>,
}

pub(crate) struct Startup {
    pub(crate) palette: Palette,
    pub(crate) loaded: Loaded,
}

pub use axlauncher_core::WorkerImages;

impl Startup {
    pub(crate) fn load() -> Self {
        let loaded = match DataDir::from_environment() {
            Ok(paths) => load_at(paths),
            Err(error) => Loaded {
                paths: None,
                settings: None,
                origin: None,
                profiles: ProfileDiscovery::default(),
                notice: Some(error.to_string()),
            },
        };
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
/// A platform shell calls this after ruling out worker mode.
pub fn run(images: WorkerImages) {
    images.configure();
    let startup = Startup::load();
    let shutdown = Rc::new(Cell::new(None));
    let on_quit = Rc::clone(&shutdown);

    kit::application()
        .with_assets(kit::assets::Assets)
        .run(move |cx| {
            cx.on_app_quit(move |cx| stop_workers(cx, &on_quit))
                .detach();
            startup.palette.install(cx);
            i18n::set_language(
                i18n::configured_language(startup.loaded.settings.as_ref()),
                cx,
            );
            open_main_window(cx, startup);
        });
    wait_for_workers(shutdown.take().unwrap_or_else(WorkerImages::begin_shutdown));
}

/// AppKit may exit without returning from the event loop. Hide the UI before its final wait.
fn stop_workers(
    cx: &kit::App,
    shutdown: &Cell<Option<JoinHandle<()>>>,
) -> impl Future<Output = ()> + use<> {
    let reaper = WorkerImages::begin_shutdown();
    let native_quit = if cfg!(target_os = "macos") {
        cx.hide();
        Some(reaper)
    } else {
        shutdown.set(Some(reaper));
        None
    };
    async move {
        if let Some(reaper) = native_quit {
            wait_for_workers(reaper);
        }
    }
}

fn wait_for_workers(reaper: JoinHandle<()>) {
    reaper.join().expect("finish worker shutdown");
}

fn open_main_window(cx: &mut kit::App, startup: Startup) {
    let bounds = Bounds::centered(None, size(px(WINDOW_WIDTH), px(WINDOW_HEIGHT)), cx);

    kit::open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(WINDOW_MIN_WIDTH), px(WINDOW_MIN_HEIGHT))),
            titlebar: Some(TitlebarOptions {
                title: Some(BRAND_NAME.into()),
                ..Default::default()
            }),
            app_id: Some(APP_ID.to_owned()),
            #[cfg(target_os = "linux")]
            icon: Some(axlauncher_ui::application_icon()),
            ..Default::default()
        },
        cx,
        move |window, cx| cx.new(|cx| RootView::new(startup, window, cx)),
    )
    .expect("failed to open the Axolotl Launcher window");
}

/// A coherent settings and instance view for the selected directories.
pub(crate) fn load_at(paths: DataDir) -> Loaded {
    let (settings, origin, notice) = match load_settings(paths.path(DataPath::Root)) {
        Ok(loaded) => (Some(loaded.data), Some(loaded.origin), None),
        Err(error) => (None, None, Some(error.to_string())),
    };

    let profiles = discover(paths.path(DataPath::Profiles));

    Loaded {
        paths: Some(paths),
        settings,
        origin,
        profiles,
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
pub(crate) fn settings_notice(origin: &SettingsOrigin, cx: &kit::App) -> Option<String> {
    match origin {
        SettingsOrigin::Created | SettingsOrigin::Stored => None,
        SettingsOrigin::Quarantined { path } => Some(i18n::interpolate(
            tr(cx, "settings.notice.quarantined"),
            &[("path", &path.display().to_string())],
        )),
        SettingsOrigin::ReadOnlyOtherSchema { found } => Some(i18n::interpolate(
            tr(cx, "settings.notice.read-only"),
            &[
                ("found", &found.to_string()),
                (
                    "supported",
                    &axlauncher_core::state::SETTINGS_SCHEMA_VERSION.to_string(),
                ),
            ],
        )),
    }
}

/// A short description of where the loaded settings came from.
pub(crate) fn describe_origin(origin: &SettingsOrigin, cx: &kit::App) -> String {
    match origin {
        SettingsOrigin::Created => tr(cx, "settings.origin.created").to_string(),
        SettingsOrigin::Stored => tr(cx, "settings.origin.stored").to_string(),
        SettingsOrigin::Quarantined { path } => i18n::interpolate(
            tr(cx, "settings.origin.quarantined"),
            &[("path", &path.display().to_string())],
        ),
        SettingsOrigin::ReadOnlyOtherSchema { found } => i18n::interpolate(
            tr(cx, "settings.origin.read-only"),
            &[("schema", &found.to_string())],
        ),
    }
}
