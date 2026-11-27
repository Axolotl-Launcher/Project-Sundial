// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The desktop workspace and state shared across its pages.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use axlauncher_core::desktop::sources::DownloadOptions;
use axlauncher_core::state::{SettingsData, SettingsOrigin, ThemeChoice};
use axlauncher_core::storage::DataDir;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{App, Context, Entity, InputEvent, InputState, Window};

use crate::installations::Installation;
use crate::instance_row::InstanceAction;
use crate::launches::Launch;
use crate::modpacks::{ImportEvent, ModpackImport};
use crate::{Loaded, Startup, load_at, palette_for};

mod accounts;
mod actions;
mod activity;
mod download_debug;
mod environment;
mod instances;
mod java;
mod launch;
mod profile_creation;
mod shell;

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
    inspection: launch::Inspection,
    installations: HashMap<PathBuf, HashMap<String, Installation>>,
    launches: HashMap<PathBuf, (String, Launch)>,
    environment: environment::EnvironmentState,
    activity: activity::ActivityLog,
}

impl RootView {
    pub(crate) fn new(startup: Startup, window: &mut Window, cx: &mut Context<'_, Self>) -> Self {
        let Startup { palette: _, loaded } = startup;
        let root_text = data_root_text(loaded.paths.as_ref());
        let root_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Data directory")
                .default_value(root_text)
        });
        let player_input =
            cx.new(|cx| InputState::new(window, cx).placeholder(accounts::DEFAULT_PLAYER));
        let profile_creation = Self::new_profile_form(&loaded, window, cx);
        cx.subscribe(&player_input, Self::player_changed).detach();
        let theme = loaded
            .settings
            .as_ref()
            .map(|settings| settings.theme)
            .unwrap_or_default();
        let modpack = Self::new_modpack_import(loaded.paths.is_some(), window, cx);
        let download_debug = cx.new(|cx| {
            download_debug::DownloadDebug::new(
                loaded.paths.clone(),
                DownloadOptions::from_settings(loaded.settings.as_ref()),
                window,
                cx,
            )
        });
        let accounts = Self::new_accounts(&loaded, player_input.clone(), window, cx);
        let game_language = loaded
            .settings
            .as_ref()
            .map_or("", |settings| settings.default_game_language.as_str());
        let environment =
            environment::EnvironmentState::new(game_language, loaded.settings.as_ref(), window, cx);
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

        view.initialize(window, cx);
        view
    }

    fn initialize(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.record_data_root();
        self.subscribe_environment(window, cx);
        self.accounts
            .update(cx, accounts::AccountsPanel::refresh_accounts);
        self.profile_creation
            .update(cx, |form, cx| form.load_minecraft_versions(window, cx));
    }

    fn new_accounts(
        loaded: &Loaded,
        player_input: Entity<InputState>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Entity<accounts::AccountsPanel> {
        let proxy = axlauncher_core::desktop::sources::fallen_proxy(loaded.settings.as_ref());
        let accounts = cx.new(|cx| {
            accounts::AccountsPanel::new(loaded.paths.as_ref(), player_input, proxy, window, cx)
        });
        cx.observe(&accounts, |_, _, cx| cx.notify()).detach();
        cx.subscribe(&accounts, |view, _, _: &accounts::SnapshotChanged, cx| {
            view.invalidate_launch_preview();
            cx.notify();
        })
        .detach();
        accounts
    }

    fn new_profile_form(
        loaded: &Loaded,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Entity<profile_creation::ProfileCreation> {
        let options = DownloadOptions::from_settings(loaded.settings.as_ref());
        let form = cx.new(|cx| {
            profile_creation::ProfileCreation::new(loaded.paths.is_some(), options, window, cx)
        });
        cx.subscribe(&form, |view, _, event, cx| view.create_profile(event, cx))
            .detach();
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
        self.accounts.read(cx).player_name(cx)
    }

    fn preview_session(&self, cx: &Context<'_, Self>) -> axlauncher_core::launch::Session {
        self.accounts.read(cx).preview_session(cx)
    }

    fn account_selection_pending(&self, cx: &Context<'_, Self>) -> bool {
        self.accounts.read(cx).account_selection_pending()
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
        self.accounts.update(cx, |accounts, cx| {
            accounts.reload(self.paths.as_ref(), same_data, window, cx)
        });
        self.replace_settings(loaded.settings, cx);
        self.origin = loaded.origin;
        self.environment
            .reload_sources(self.settings.as_ref(), window, cx);
        self.publish_transfer_progress(cx);
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

        let root_text = data_root_text(self.paths.as_ref());
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

    fn invalidate_launch_preview(&mut self) {
        self.inspection = launch::Inspection::default();
    }

    fn instance_action(
        directory: PathBuf,
        cx: &Context<'_, Self>,
    ) -> impl Fn(InstanceAction, &mut App) + Clone + 'static + use<> {
        let view = cx.weak_entity();
        move |action, cx| {
            let _ = view.update(cx, |view, cx| match action {
                InstanceAction::Select => view.select_instance(directory.clone(), cx),
                InstanceAction::Install(game) => view.install_game(&directory, &game, cx),
                InstanceAction::Launch => view.launch(&directory, cx),
            });
        }
    }

    fn create_profile(
        &mut self,
        profile_creation::CreateProfile(name, game): &profile_creation::CreateProfile,
        cx: &mut Context<'_, Self>,
    ) {
        use axlauncher_core::state::profile;

        let Some(paths) = self.paths.as_ref() else {
            return;
        };
        let instances = paths.instances();
        match profile::create(&instances, name, game.clone()) {
            Ok(directory) => {
                self.activity
                    .success(format!("Created {name} at {}", directory.display()));
                self.instances.replace(profile::discover(&instances), true);
                self.instances.select(&directory);
                self.invalidate_launch_preview();
            }
            Err(error) => self.activity.error(error.to_string()),
        }
        cx.notify();
    }

    fn replace_settings(&mut self, settings: Option<SettingsData>, cx: &mut Context<'_, Self>) {
        self.settings = settings;
        let proxy = self.fallen_proxy();
        self.accounts
            .update(cx, |accounts, cx| accounts.set_fallen_proxy(proxy, cx));
        let options = DownloadOptions::from_settings(self.settings.as_ref());
        self.profile_creation.update(cx, |form, cx| {
            form.configure(self.paths.is_some(), options, cx)
        });
        self.download_debug.update(cx, |debug, cx| {
            debug.configure(
                self.paths.clone(),
                DownloadOptions::from_settings(self.settings.as_ref()),
                cx,
            );
        });
    }

    fn fallen_proxy(&self) -> Option<axlauncher_core::spi::ServiceHost> {
        axlauncher_core::desktop::sources::fallen_proxy(self.settings.as_ref())
    }
}

fn data_root_text(paths: Option<&DataDir>) -> String {
    paths.map_or_else(String::new, |paths| paths.root().display().to_string())
}
