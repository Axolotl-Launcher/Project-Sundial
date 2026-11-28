// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Install and launch actions for profiles shown in the panel.

use std::path::Path;
use std::sync::Arc;

use axlauncher_core::desktop::{
    accounts::DesktopAccounts,
    installation::{InstallTarget, install_key},
    launching::LaunchFailure,
    sources::DownloadOptions,
};
use axlauncher_core::install::prepare::Progress;
use axlauncher_core::state::profile::Game;
use axlauncher_core::storage::DataDir;
use axlauncher_ui::{AsyncApp, Context, WeakEntity};

use crate::installations::{self, Installation};
use crate::launches::{self, Launch};

use super::super::RootView;

impl RootView {
    /// A vanilla profile installs its Minecraft version; a loader profile
    /// acquires that version first, resolves the loader's profile, and
    /// prepares the files the loader's document names.
    pub(crate) fn install_game(
        &mut self,
        directory: &Path,
        game: &Game,
        cx: &mut Context<'_, Self>,
    ) {
        if self.instances.installing.contains(directory) || self.instance_folder_is_busy(directory)
        {
            return;
        }
        let Some(paths) = self.paths.clone() else {
            return;
        };
        let game_paths = paths.game();
        let game_root = game_paths.root();
        let version = game.minecraft.clone();
        let Some(target) = InstallTarget::for_game(game) else {
            self.activity.error(format!(
                "Loader {} is not available in this build yet.",
                game.loader
                    .as_ref()
                    .map_or("", |loader| loader.kind.as_str())
            ));
            cx.notify();
            return;
        };
        let key = target.key();
        if self.install_in_progress(game_root, &key, game) {
            return;
        }
        self.instances.installing.insert(directory.to_owned());
        let options = DownloadOptions::from_settings(self.settings.as_ref());
        self.activity
            .info(format!("Installing {version} in {}", game_root.display()));
        self.set_installation(
            game_root,
            &key,
            Installation::Running(Progress::Version),
            cx,
        );
        let job = installations::start(paths, directory.to_path_buf(), target, options, cx);
        cx.spawn(async move |view, cx| {
            let _ = Self::watch_installation(job, view, cx).await;
        })
        .detach();
    }

    /// Follows a started install until it settles, reporting as the row expects.
    async fn watch_installation(
        job: installations::InstallationJob,
        view: WeakEntity<Self>,
        cx: &mut AsyncApp,
    ) -> Option<()> {
        let installations::InstallationJob {
            key,
            directory,
            is_loader,
            paths,
            game_root,
            mut updates,
            task,
        } = job;
        while updates.changed().await.is_ok() {
            let progress = *updates.borrow_and_update();
            Self::show_installation_progress(&view, &game_root, &key, progress, cx)?;
        }
        let outcome = task.await;
        view.update(cx, |view, cx| {
            view.finish_installation(&paths, &directory, &key, is_loader, outcome, cx)
        })
        .ok()
    }

    fn show_installation_progress(
        view: &WeakEntity<Self>,
        game_root: &Path,
        key: &str,
        progress: Progress,
        cx: &mut AsyncApp,
    ) -> Option<()> {
        view.update(cx, |view, cx| {
            view.set_installation(game_root, key, Installation::Running(progress), cx)
        })
        .ok()
    }

    /// Whether this profile already has an install the button must not repeat.
    ///
    /// Complete describes the shared slot, not this profile: a second profile
    /// that names the same game and loader and has written no version id of its
    /// own still needs to run. It is the only action that would produce one.
    fn install_in_progress(&self, game_root: &Path, key: &str, game: &Game) -> bool {
        match self.installation(game_root, key) {
            Some(Installation::Running(_)) => true,
            Some(Installation::Complete) => game.version_id.is_some(),
            Some(Installation::Failed(_)) | None => false,
        }
    }

    pub(crate) fn finish_installation(
        &mut self,
        paths: &DataDir,
        directory: &Path,
        key: &str,
        recorded_profile: bool,
        outcome: Result<Option<String>, String>,
        cx: &mut Context<'_, Self>,
    ) {
        self.instances.installing.remove(directory);
        let installation = match outcome {
            Ok(Some(version)) => {
                self.activity
                    .success(format!("{} installed as {version}", directory.display()));
                Installation::Complete
            }
            Ok(None) => Installation::Complete,
            Err(error) => Installation::Failed(error),
        };
        if recorded_profile && self.instances.refresh_for(self.paths.as_ref(), paths) {
            self.invalidate_inspection();
        }
        self.set_installation(paths.game().root(), key, installation, cx);
        self.refresh_java_after_install(paths, cx);
    }

    fn installation(&self, root: &Path, version: &str) -> Option<&Installation> {
        self.installations.get(root)?.get(version)
    }

    pub(crate) fn set_installation(
        &mut self,
        root: &Path,
        version: &str,
        installation: Installation,
        cx: &mut Context<'_, Self>,
    ) {
        match &installation {
            Installation::Complete => self
                .activity
                .success(format!("{version} ready in {}", root.display())),
            Installation::Failed(error) => self.activity.error(format!(
                "Installing {version} in {} failed: {error}",
                root.display()
            )),
            Installation::Running(_) => (),
        }
        self.installations
            .entry(root.to_path_buf())
            .or_default()
            .insert(version.to_owned(), installation);
        self.update_download_activity(cx);
        cx.notify();
    }

    pub(crate) fn launch(&mut self, directory: &Path, cx: &mut Context<'_, Self>) {
        if self.instances.installing.contains(directory)
            || self.instance_folder_is_busy(directory)
            || self.account_selection_pending(cx)
        {
            return;
        }
        let Some(entry) = self.instances.find(directory) else {
            return;
        };
        if self.java_write_pending(entry.profile.id(), cx) {
            return;
        }
        let Some(job) = self.launch_job(entry, self.player_name(cx)) else {
            return;
        };
        // The install is tracked under the key the row uses, not under the
        // version id a launch asks for: for a loader profile those differ, and
        // launching while its install rewrites the game files must be blocked.
        let installing = entry
            .profile
            .game()
            .and_then(install_key)
            .is_some_and(|key| {
                matches!(
                    self.installation(job.game_root(), &key),
                    Some(Installation::Running(_))
                )
            });
        if self
            .launches
            .get(directory)
            .is_some_and(|(_, launch)| launch.is_active())
            || installing
        {
            return;
        }
        let player = self.accounts.read(cx).player(cx);
        self.set_launch(directory, job.version_id(), Launch::Starting, cx);
        let options = DownloadOptions::from_settings(self.settings.as_ref());
        let launch = launches::start(job, player, options, cx);
        cx.spawn(async move |view, cx| Self::watch_launch(launch, view, cx).await)
            .detach();
    }

    /// Follows a started launch until it settles, reporting as the row expects.
    async fn watch_launch(launch: launches::LaunchTask, view: WeakEntity<Self>, cx: &mut AsyncApp) {
        let launches::LaunchTask {
            game_root,
            directory,
            version,
            mut started,
            account,
            task,
        } = launch;
        if let Ok(service) = account.await {
            let _ = view.update(cx, |view, cx| view.sync_launch_accounts(&service, cx));
        }
        let show_running = |view: &mut Self, cx: &mut Context<'_, Self>| {
            view.set_launch(&directory, &version, Launch::Running, cx);
        };
        if started.changed().await.is_ok() {
            let _ = view.update(cx, show_running);
        }
        let outcome = task.await;
        let _ = view.update(cx, |view, cx| {
            view.finish_launch(&game_root, &directory, &version, outcome, cx)
        });
    }

    fn sync_launch_accounts(&mut self, service: &Arc<DesktopAccounts>, cx: &mut Context<'_, Self>) {
        self.accounts.update(cx, |accounts, cx| {
            accounts.sync_launch_accounts(service, cx)
        });
    }

    fn finish_launch(
        &mut self,
        root: &Path,
        directory: &Path,
        version: &str,
        outcome: Result<(), LaunchFailure>,
        cx: &mut Context<'_, Self>,
    ) {
        let launch = match outcome {
            Ok(()) => Launch::Exited,
            Err(LaunchFailure::NeedsInstall(reason)) => {
                self.clear_completed_installation(root, directory);
                Launch::NeedsInstall(reason)
            }
            Err(LaunchFailure::Failed(reason)) => Launch::Failed(reason),
        };
        self.set_launch(directory, version, launch, cx);
    }

    pub(crate) fn set_launch(
        &mut self,
        directory: &Path,
        version: &str,
        launch: Launch,
        cx: &mut Context<'_, Self>,
    ) {
        let message = format!("{} ({version}): {}", directory.display(), launch.message());
        match launch {
            Launch::Starting | Launch::Running => self.activity.info(message),
            Launch::Exited => self.activity.success(message),
            Launch::NeedsInstall(_) | Launch::Failed(_) => self.activity.error(message),
        }
        self.launches
            .insert(directory.to_path_buf(), (version.to_owned(), launch));
        cx.notify();
    }

    /// Forgets a completed install for the profile at `directory`, so a launch
    /// that found files missing offers Install again.
    pub(crate) fn clear_completed_installation(&mut self, root: &Path, directory: &Path) {
        let Some(key) = self
            .instances
            .find(directory)
            .and_then(|entry| entry.profile.game())
            .and_then(install_key)
        else {
            return;
        };
        if let Some(states) = self.installations.get_mut(root)
            && matches!(states.get(&key), Some(Installation::Complete))
        {
            states.remove(&key);
        }
    }
}
