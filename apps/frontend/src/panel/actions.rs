// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Install and launch actions for profiles shown in the panel.

use std::path::Path;

use axlauncher_core::desktop::{
    installation::{InstallTarget, install_key},
    launching::{LaunchJob, Player},
    sources::DownloadOptions,
};
use axlauncher_core::install::prepare::Progress;
use axlauncher_core::state::profile::Game;
use axlauncher_core::storage::DataDir;
use axlauncher_ui::{Context, Entity};

use crate::installations::{self, InstallEvent, Installation};
use crate::launches::{self, Launch, LaunchEvent};
use crate::modpacks::ModpackImport;

use super::{RootView, download_debug::TransferProgress};

const MAX_VISIBLE_INSTALLATIONS: usize = 8;

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
        let options = DownloadOptions::from_settings(self.settings.as_ref());
        self.activity
            .info(format!("Installing {version} in {}", game_root.display()));
        self.set_installation(
            game_root,
            &key,
            Installation::Running(Progress::Version),
            cx,
        );
        let directory = directory.to_path_buf();
        let installed_paths = paths.clone();
        let installed_directory = directory.clone();
        let recorded_profile = target.is_loader();
        installations::start(
            paths,
            directory,
            target,
            options,
            cx,
            move |view, event, cx| {
                view.installation_event(
                    &installed_paths,
                    &installed_directory,
                    &key,
                    recorded_profile,
                    event,
                    cx,
                );
            },
        );
    }

    fn installation_event(
        &mut self,
        paths: &DataDir,
        directory: &Path,
        key: &str,
        recorded_profile: bool,
        event: InstallEvent,
        cx: &mut Context<'_, Self>,
    ) {
        match event {
            InstallEvent::Progress(progress) => self.set_installation(
                paths.game().root(),
                key,
                Installation::Running(progress),
                cx,
            ),
            InstallEvent::Finished(outcome) => {
                self.finish_installation(paths, directory, key, recorded_profile, outcome, cx)
            }
        }
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

    fn finish_installation(
        &mut self,
        paths: &DataDir,
        directory: &Path,
        key: &str,
        recorded_profile: bool,
        outcome: Result<Option<String>, String>,
        cx: &mut Context<'_, Self>,
    ) {
        let installation = match outcome {
            Ok(Some(version)) => {
                self.activity
                    .success(format!("{} installed as {version}", directory.display()));
                Installation::Complete
            }
            Ok(None) => Installation::Complete,
            Err(error) => Installation::Failed(error),
        };
        if recorded_profile
            && self
                .instances
                .refresh_after_install(self.paths.as_ref(), paths)
        {
            self.invalidate_launch_preview();
        }
        self.set_installation(paths.game().root(), key, installation, cx);
        self.refresh_java_after_install(paths, cx);
    }

    fn installation(&self, root: &Path, version: &str) -> Option<&Installation> {
        self.installations.get(root)?.get(version)
    }

    fn set_installation(
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
        self.publish_transfer_progress(cx);
        cx.notify();
    }

    pub(super) fn modpack_download_changed(
        &mut self,
        _: Entity<ModpackImport>,
        cx: &mut Context<'_, Self>,
    ) {
        self.publish_transfer_progress(cx);
        cx.notify();
    }

    pub(super) fn publish_transfer_progress(&self, cx: &mut Context<'_, Self>) {
        let modpack = self.modpack.read(cx).install_progress();
        let active = modpack.is_some()
            || self
                .installations
                .values()
                .flat_map(|versions| versions.values())
                .any(|state| matches!(state, Installation::Running(_)));
        let mut transfers = modpack
            .map(|(name, progress)| {
                TransferProgress::new(
                    "download-progress:modpack".into(),
                    format!("{name} · {}", progress.message()),
                )
            })
            .into_iter()
            .collect::<Vec<_>>();
        let game = self.paths.as_ref().map(DataDir::game);
        if let Some(versions) = game
            .as_ref()
            .and_then(|game| self.installations.get(game.root()))
        {
            let mut running: Vec<_> = versions
                .iter()
                .filter(|(_, state)| matches!(state, Installation::Running(_)))
                .collect();
            running.sort_unstable_by_key(|(version, _)| version.as_str());
            transfers.extend(
                running
                    .into_iter()
                    .take(MAX_VISIBLE_INSTALLATIONS)
                    .map(|(version, state)| installation_progress(version, state)),
            );
        }
        self.download_debug
            .update(cx, |debug, cx| debug.set_transfers(transfers, active, cx));
    }

    pub(crate) fn launch(&mut self, directory: &Path, cx: &mut Context<'_, Self>) {
        if self.account_selection_pending(cx) {
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
        let player = match self.accounts.read(cx).selected_online() {
            Some((service, id)) => Player::Online {
                service,
                id,
                fallen_proxy: self.fallen_proxy(),
            },
            None => Player::Offline(self.preview_session(cx)),
        };
        self.start_launch(job, player, cx);
    }

    fn start_launch(&mut self, job: LaunchJob, player: Player, cx: &mut Context<'_, Self>) {
        let root = job.game_root().to_path_buf();
        let directory = job.instance_directory().to_path_buf();
        let version = job.version_id().to_owned();
        self.set_launch(&directory, &version, Launch::Starting, cx);
        let options = DownloadOptions::from_settings(self.settings.as_ref());
        launches::start(job, player, options, cx, move |view, event, cx| {
            view.launch_event(&root, &directory, &version, event, cx);
        });
    }

    fn launch_event(
        &mut self,
        root: &Path,
        directory: &Path,
        version: &str,
        event: LaunchEvent,
        cx: &mut Context<'_, Self>,
    ) {
        match event {
            LaunchEvent::AccountResolved(service) => self.accounts.update(cx, |accounts, cx| {
                accounts.sync_launch_accounts(&service, cx)
            }),
            LaunchEvent::Status(status) => {
                self.update_launch_status(root, directory, version, status, cx)
            }
        }
    }

    fn update_launch_status(
        &mut self,
        root: &Path,
        directory: &Path,
        version: &str,
        status: Launch,
        cx: &mut Context<'_, Self>,
    ) {
        if matches!(status, Launch::NeedsInstall(_)) {
            self.clear_completed_installation(root, directory);
        }
        self.set_launch(directory, version, status, cx);
    }

    fn set_launch(
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
    fn clear_completed_installation(&mut self, root: &Path, directory: &Path) {
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

fn installation_progress(version: &str, state: &Installation) -> TransferProgress {
    TransferProgress::new(
        format!("download-progress:{version}"),
        format!("{version} · {}", state.message()),
    )
}
