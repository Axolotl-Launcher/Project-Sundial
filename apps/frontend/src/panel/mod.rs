// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The desktop workspace and state shared across its pages.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use axlauncher_core::desktop::sources::{DownloadOptions, fallen_proxy};
use axlauncher_core::launch::plan::Session;
use axlauncher_core::network_spi::ServiceHost;
use axlauncher_core::state::{SettingsData, SettingsOrigin, ThemeChoice};
use axlauncher_core::storage::{DataDir, DataPath};
use axlauncher_ui::prelude::*;
use axlauncher_ui::{Context, Entity, InputEvent, InputState, Window};

use crate::installations::Installation;
use crate::launches::Launch;
use crate::modpacks::{ImportEvent, ModpackImport};
use crate::{Loaded, Startup, load_at, palette_for};

pub(crate) mod accounts;
mod activity;
mod download_debug;
mod environment;
mod instances;
mod java;
mod markdown_debug;
mod profile_creation;
mod service_sources;
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
    accounts: Entity<accounts::AccountsPanel>,
    profile_creation: Entity<profile_creation::ProfileCreation>,
    instances: instances::InstancesState,
    java: Entity<java::JavaPanel>,
    modpack: Entity<ModpackImport>,
    download_debug: Entity<download_debug::DownloadDebug>,
    markdown_debug: Entity<markdown_debug::MarkdownDebug>,
    download_sources: service_sources::DownloadSources,
    inspection: instances::launch::Inspection,
    installations: HashMap<PathBuf, HashMap<String, Installation>>,
    launches: HashMap<PathBuf, (String, Launch)>,
    environment: environment::EnvironmentState,
    activity: activity::ActivityLog,
}

impl RootView {
    pub(crate) fn new(startup: Startup, window: &mut Window, cx: &mut Context<'_, Self>) -> Self {
        let Startup { palette: _, loaded } = startup;
        let root_text = Self::data_root_text(loaded.paths.as_ref());
        let root_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Data directory")
                .default_value(root_text)
        });
        let player_input = cx.new(|cx| InputState::new(window, cx).placeholder(DEFAULT_PLAYER));
        let profile_creation = Self::new_profile_creation(
            loaded.paths.is_some(),
            loaded.settings.as_ref(),
            window,
            cx,
        );
        cx.subscribe(&player_input, Self::player_changed).detach();
        let theme = loaded
            .settings
            .as_ref()
            .map(|settings| settings.theme)
            .unwrap_or_default();
        let modpack = Self::new_modpack_import(loaded.paths.is_some(), window, cx);
        let download_debug =
            Self::new_download_debug(loaded.paths.clone(), loaded.settings.as_ref(), window, cx);
        let markdown_debug = cx.new(|cx| markdown_debug::MarkdownDebug::new(window, cx));
        let download_sources = Self::new_download_sources(loaded.settings.as_ref(), window, cx);
        let accounts = Self::new_accounts(&loaded, player_input.clone(), window, cx);
        let game_language = loaded
            .settings
            .as_ref()
            .map_or("", |settings| settings.default_game_language.as_str());
        let environment = Self::new_environment(game_language, window, cx);
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
            markdown_debug,
            download_sources,
            inspection: instances::launch::Inspection::default(),
            installations: HashMap::new(),
            launches: HashMap::new(),
            environment,
            activity: activity::ActivityLog::new(),
        };

        view.record_data_root();
        view
    }

    fn data_root_text(paths: Option<&DataDir>) -> String {
        paths.map_or_else(String::new, |paths| {
            paths.path(DataPath::Root).display().to_string()
        })
    }

    fn new_accounts(
        loaded: &Loaded,
        player_input: Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Entity<accounts::AccountsPanel> {
        let accounts = cx.new(|cx| {
            accounts::AccountsPanel::new(
                loaded.paths.as_ref(),
                player_input,
                loaded.settings.as_ref(),
                window,
                cx,
            )
        });
        cx.observe(&accounts, |_, _, cx| cx.notify()).detach();
        cx.subscribe(
            &accounts,
            |view, _, _: &accounts::AccountSelectionChanged, _| {
                view.invalidate_inspection();
            },
        )
        .detach();
        accounts
    }

    fn new_profile_creation(
        enabled: bool,
        settings: Option<&SettingsData>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Entity<profile_creation::ProfileCreation> {
        let options = DownloadOptions::from_settings(settings);
        let form =
            cx.new(|cx| profile_creation::ProfileCreation::new(enabled, options, window, cx));
        cx.subscribe(&form, Self::create_profile).detach();
        form
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

    fn new_download_debug(
        paths: Option<DataDir>,
        settings: Option<&SettingsData>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Entity<download_debug::DownloadDebug> {
        let options = DownloadOptions::from_settings(settings);
        cx.new(|cx| download_debug::DownloadDebug::new(paths, options, window, cx))
    }

    fn modpack_download_changed(&mut self, _: Entity<ModpackImport>, cx: &mut Context<'_, Self>) {
        self.update_download_activity(cx);
        cx.notify();
    }

    fn update_download_activity(&self, cx: &mut Context<'_, Self>) {
        let active = self.modpack.read(cx).install_progress().is_some()
            || self
                .installations
                .values()
                .flat_map(|versions| versions.values())
                .any(|state| matches!(state, Installation::Running(_)));
        self.download_debug
            .update(cx, |debug, cx| debug.set_external_activity(active, cx));
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
                self.invalidate_inspection();
            }
            java::JavaEvent::SelectionChanged(_) => return,
        }
        cx.notify();
    }

    fn java_write_pending(&self, profile: &str, cx: &Context<'_, Self>) -> bool {
        self.java.read(cx).write_pending(profile)
    }

    fn refresh_java_after_install(&mut self, paths: &DataDir, cx: &mut Context<'_, Self>) {
        if self.paths.as_ref().map(|paths| paths.path(DataPath::Root))
            == Some(paths.path(DataPath::Root))
        {
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
            self.invalidate_inspection();
            cx.notify();
        }
    }

    fn player_name(&self, cx: &Context<'_, Self>) -> String {
        self.accounts.read(cx).player_name(cx)
    }

    fn preview_session(&self, cx: &Context<'_, Self>) -> Session {
        self.accounts.read(cx).preview_session(cx)
    }

    fn account_selection_pending(&self, cx: &Context<'_, Self>) -> bool {
        self.accounts.read(cx).account_selection_pending()
    }

    /// Drops the diagnostics preview, because what it described has changed.
    fn invalidate_inspection(&mut self) {
        self.inspection = instances::launch::Inspection::default();
    }

    /// The profile the creation form describes, once its marker is on disk.
    fn create_profile(
        &mut self,
        _: Entity<profile_creation::ProfileCreation>,
        profile_creation::ProfileRequested(name, game): &profile_creation::ProfileRequested,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(paths) = self.paths.as_ref() else {
            return;
        };

        let instances = paths.path(DataPath::Profiles);
        match axlauncher_core::state::profile::create(instances, name, game.clone()) {
            Ok(directory) => {
                self.activity
                    .success(format!("Created {name} at {}", directory.display()));
                self.instances
                    .replace(axlauncher_core::state::profile::discover(instances), true);
                self.instances.select(&directory);
                self.invalidate_inspection();
            }
            Err(error) => self.activity.error(error.to_string()),
        }
        cx.notify();
    }

    fn set_settings(&mut self, settings: Option<SettingsData>, cx: &mut Context<'_, Self>) {
        let options = DownloadOptions::from_settings(settings.as_ref());
        self.profile_creation.update(cx, |form, cx| {
            form.configure(self.paths.is_some(), options.clone(), cx)
        });
        self.download_debug.update(cx, |debug, cx| {
            debug.configure(self.paths.clone(), options, cx)
        });
        self.accounts.update(cx, |accounts, _| {
            accounts.configure_sign_in(settings.as_ref())
        });
        self.settings = settings;
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
        let same_data = previous.map(|paths| paths.path(DataPath::Root))
            == current.map(|paths| paths.path(DataPath::Root));
        let same_instances = previous.map(|paths| paths.path(DataPath::Profiles))
            == current.map(|paths| paths.path(DataPath::Profiles));
        self.paths = loaded.paths;
        self.modpack
            .update(cx, |view, cx| view.set_enabled(self.paths.is_some(), cx));
        self.set_settings(loaded.settings, cx);
        self.accounts.update(cx, |accounts, cx| {
            accounts.reload(self.paths.as_ref(), same_data, window, cx)
        });
        self.origin = loaded.origin;
        self.reload_sources(window, cx);
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
        self.invalidate_inspection();

        let root_text = Self::data_root_text(self.paths.as_ref());
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
            Some(paths) => self.activity.info(format!(
                "Data root: {}",
                paths.path(DataPath::Root).display()
            )),
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

    fn fallen_proxy(&self) -> Option<ServiceHost> {
        fallen_proxy(self.settings.as_ref())
    }
}
