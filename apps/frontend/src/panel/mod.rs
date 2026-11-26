// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The desktop workspace and state shared across its pages.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use axlauncher_core::state::{SettingsData, SettingsOrigin, ThemeChoice};
use axlauncher_core::storage::DataDir;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{Context, Entity, InputEvent, InputState, Window};

use crate::installations::Installation;
use crate::launches::Launch;
use crate::modpacks::{ImportEvent, ModpackImport};
use crate::{Loaded, Startup, load_at, palette_for};

pub(crate) mod accounts;
mod actions;
mod activity;
mod download_debug;
mod environment;
mod instances;
mod java;
mod launch;
mod profile_creation;
mod shell;

const DEFAULT_PLAYER: &str = "Player";

pub(crate) struct RootView {
    page: shell::Page,
    sidebar_collapsed: bool,
    paths: Option<DataDir>,
    notice: Option<String>,
    settings: Option<SettingsData>,
    origin: Option<SettingsOrigin>,
    theme: ThemeChoice,
    root_input: Entity<InputState>,
    player_input: Entity<InputState>,
    accounts: accounts::AccountsPanel,
    profile_creation: profile_creation::ProfileCreation,
    instances: instances::InstancesState,
    java: Entity<java::JavaPanel>,
    modpack: Entity<ModpackImport>,
    download_debug: download_debug::DownloadDebug,
    inspection: launch::Inspection,
    installations: HashMap<PathBuf, HashMap<String, Installation>>,
    launches: HashMap<PathBuf, (String, Launch)>,
    environment: environment::EnvironmentState,
    activity: activity::ActivityLog,
}

impl RootView {
    pub(crate) fn new(startup: Startup, window: &mut Window, cx: &mut Context<'_, Self>) -> Self {
        let Startup { palette: _, loaded } = startup;
        let root_text = loaded
            .paths
            .as_ref()
            .map(DataDir::root)
            .map(|root| root.display().to_string())
            .unwrap_or_default();
        let root_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Data directory")
                .default_value(root_text)
        });
        let player_input = cx.new(|cx| InputState::new(window, cx).placeholder(DEFAULT_PLAYER));
        let profile_creation = profile_creation::ProfileCreation::new(window, cx);
        cx.subscribe(&player_input, Self::player_changed).detach();
        let theme = loaded
            .settings
            .as_ref()
            .map(|settings| settings.theme)
            .unwrap_or_default();
        let modpack = Self::new_modpack_import(loaded.paths.is_some(), window, cx);
        let download_debug =
            download_debug::DownloadDebug::new(loaded.settings.as_ref(), window, cx);
        let accounts = accounts::AccountsPanel::new(loaded.paths.as_ref(), window, cx);
        let game_language = loaded
            .settings
            .as_ref()
            .map_or("", |settings| settings.default_game_language.as_str());
        let environment = environment::EnvironmentState::new(game_language, window, cx);
        let java = Self::new_java_panel(loaded.paths.clone(), cx);
        let mut view = Self {
            page: shell::Page::Home,
            sidebar_collapsed: false,
            paths: loaded.paths,
            notice: loaded.notice,
            settings: loaded.settings,
            origin: loaded.origin,
            theme,
            root_input,
            player_input,
            accounts,
            profile_creation,
            instances: instances::InstancesState::new(loaded.profiles),
            java,
            modpack,
            download_debug,
            inspection: launch::Inspection::default(),
            installations: HashMap::new(),
            launches: HashMap::new(),
            environment,
            activity: activity::ActivityLog::new(),
        };

        view.record_data_root();
        view.refresh_accounts(cx);
        view.load_minecraft_versions(window, cx);
        view
    }

    fn new_modpack_import(
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Entity<ModpackImport> {
        let modpack = cx.new(ModpackImport::new);
        modpack.update(cx, |view, cx| view.set_enabled(enabled, cx));
        cx.subscribe_in(&modpack, window, Self::modpack_event)
            .detach();
        cx.observe(&modpack, Self::modpack_download_changed)
            .detach();
        modpack
    }

    fn new_java_panel(
        paths: Option<DataDir>,
        cx: &mut Context<'_, Self>,
    ) -> Entity<java::JavaPanel> {
        let panel = cx.new(|cx| java::JavaPanel::new(paths, cx));
        cx.subscribe(&panel, Self::java_changed).detach();
        cx.observe(&panel, |_, _, cx| cx.notify()).detach();
        panel
    }

    fn java_changed(
        &mut self,
        _: Entity<java::JavaPanel>,
        event: &java::JavaEvent,
        cx: &mut Context<'_, Self>,
    ) {
        let selected = self.instances.selected().map(|entry| entry.profile.id());
        match event {
            java::JavaEvent::RefreshStarted => self.inspection.mark_java_refresh(),
            java::JavaEvent::CatalogChanged => self.inspection.invalidate_refreshed_catalog(),
            java::JavaEvent::SelectionChanged(profile)
                if profile
                    .as_deref()
                    .is_none_or(|owner| Some(owner) == selected) =>
            {
                self.inspection = launch::Inspection::default();
            }
            java::JavaEvent::SelectionChanged(_) => return,
        }
        cx.notify();
    }

    fn java_write_pending(&self, profile: &str, cx: &Context<'_, Self>) -> bool {
        self.java.read(cx).write_pending(profile)
    }

    fn refresh_java_after_install(&mut self, paths: &DataDir, cx: &mut Context<'_, Self>) {
        if self.paths.as_ref().map(DataDir::root) == Some(paths.root()) {
            self.java.update(cx, java::JavaPanel::refresh);
        }
    }

    fn player_changed(
        &mut self,
        _: Entity<InputState>,
        event: &InputEvent,
        cx: &mut Context<'_, Self>,
    ) {
        if matches!(event, InputEvent::Change) {
            self.inspection = launch::Inspection::default();
            cx.notify();
        }
    }

    fn player_name(&self, cx: &Context<'_, Self>) -> String {
        let value = self.player_input.read(cx).value();
        let name = value.trim();
        if name.is_empty() {
            DEFAULT_PLAYER.to_owned()
        } else {
            name.to_owned()
        }
    }

    pub(super) fn reload(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let Some(paths) = self.paths.clone() else {
            return;
        };
        let loaded = load_at(paths);
        self.apply_loaded(loaded, window, cx);
        self.activity.info("Reloaded settings and instances");
        cx.notify();
    }

    fn apply_loaded(&mut self, loaded: Loaded, window: &mut Window, cx: &mut Context<'_, Self>) {
        let previous = self.paths.as_ref();
        let current = loaded.paths.as_ref();
        let same_data = previous.map(DataDir::root) == current.map(DataDir::root);
        let same_instances = previous.map(DataDir::instances) == current.map(DataDir::instances);
        self.paths = loaded.paths;
        self.modpack
            .update(cx, |view, cx| view.set_enabled(self.paths.is_some(), cx));
        if !same_data {
            self.accounts.reset(self.paths.as_ref(), window, cx);
        } else {
            self.accounts.update_paths(self.paths.as_ref());
        }
        self.refresh_accounts(cx);
        self.settings = loaded.settings;
        self.origin = loaded.origin;
        self.download_debug
            .reload_sources(self.settings.as_ref(), window, cx);
        self.notice = loaded.notice;
        self.theme = self
            .settings
            .as_ref()
            .map(|settings| settings.theme)
            .unwrap_or_default();
        self.environment.clear_feedback();
        self.java
            .update(cx, |panel, cx| panel.reload(self.paths.clone(), cx));
        let game_language = self
            .settings
            .as_ref()
            .map(|settings| settings.default_game_language.clone())
            .unwrap_or_default();
        self.environment
            .set_game_language(game_language, window, cx);
        self.instances.replace(loaded.profiles, same_instances);
        self.inspection = launch::Inspection::default();

        let root_text = self
            .paths
            .as_ref()
            .map(DataDir::root)
            .map(|root| root.display().to_string())
            .unwrap_or_default();
        self.root_input
            .update(cx, |state, cx| state.set_value(root_text, window, cx));
        palette_for(self.theme).apply(cx);
    }

    fn modpack_event(
        &mut self,
        view: &Entity<ModpackImport>,
        event: &ImportEvent,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        match event {
            ImportEvent::InstallRequested => self.install_modpack(view, window, cx),
            ImportEvent::Installed {
                paths,
                directory,
                name,
            } => self.record_modpack(paths, directory, name, cx),
        }
    }

    fn install_modpack(
        &mut self,
        view: &Entity<ModpackImport>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(paths) = self.paths.clone() else {
            return;
        };
        let options = axlauncher_core::desktop::sources::DownloadOptions::from_settings(
            self.settings.as_ref(),
        );
        view.update(cx, |view, cx| view.install(paths, options, window, cx));
    }

    fn record_modpack(
        &mut self,
        paths: &DataDir,
        directory: &Path,
        name: &str,
        cx: &mut Context<'_, Self>,
    ) {
        self.activity.success(format!("Installed {name}"));
        self.refresh_java_after_install(paths, cx);
        if self
            .instances
            .refresh_after_install(self.paths.as_ref(), paths)
        {
            self.select_instance(directory.to_path_buf(), cx);
        }
        cx.notify();
    }

    fn record_data_root(&mut self) {
        match &self.paths {
            Some(paths) => self
                .activity
                .info(format!("Data root: {}", paths.root().display())),
            None => self.activity.error("Data root unknown"),
        }
    }

    fn settings_readonly(&self) -> bool {
        self.settings.is_none()
            || self.paths.is_none()
            || matches!(
                self.origin,
                Some(SettingsOrigin::ReadOnlyOtherSchema { .. })
            )
    }

    fn fallen_proxy(&self) -> Option<axlauncher_core::spi::ServiceHost> {
        axlauncher_core::desktop::sources::fallen_proxy(self.settings.as_ref())
    }
}
