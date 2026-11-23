// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Install and launch actions for profiles shown in the panel.

use std::path::Path;

use axlauncher_core::install::prepare::Progress;
use axlauncher_core::storage::DataDir;
use axlauncher_ui::Context;

use crate::installations::{self, Installation, vanilla_version};
use crate::launches::{self, Launch};

use super::RootView;

impl RootView {
    pub(crate) fn install(&mut self, version: &str, cx: &mut Context<'_, Self>) {
        let Some(root) = self.root.as_ref().map(|root| DataDir::new(root).meta()) else {
            return;
        };
        if matches!(
            self.installation(&root, version),
            Some(Installation::Running(_) | Installation::Complete)
        ) {
            return;
        }
        let concurrency = self
            .settings
            .as_ref()
            .map_or(0, |settings| settings.max_concurrent_downloads as usize);
        self.activity
            .info(format!("Installing {version} in {}", root.display()));
        self.set_installation(&root, version, Installation::Running(Progress::Version), cx);
        self.monitor_downloads(cx);
        installations::start(root, version.to_owned(), concurrency, cx);
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
        cx.notify();
    }

    pub(crate) fn launch(&mut self, directory: &Path, cx: &mut Context<'_, Self>) {
        if self.account_selection_pending() {
            return;
        }
        let Some(entry) = self.instances.find(directory) else {
            return;
        };
        let Some(request) = self.launch_request(entry, self.player_name(cx)) else {
            return;
        };
        let vanilla = entry
            .profile
            .game
            .as_ref()
            .and_then(vanilla_version)
            .is_some();
        if self
            .launches
            .get(directory)
            .is_some_and(|(_, launch)| launch.is_active())
            || matches!(
                self.installation(&request.game_root, &request.version_id),
                Some(Installation::Running(_))
            )
        {
            return;
        }
        let player = match self.accounts.selected_microsoft() {
            Some((service, id)) => launches::Player::Microsoft(service, id),
            None => launches::Player::Offline(self.preview_session(cx)),
        };
        self.set_launch(directory, &request.version_id, Launch::Starting, cx);
        launches::start(request, player, vanilla, cx);
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

    pub(crate) fn clear_completed_installation(&mut self, root: &Path, version: &str) {
        if let Some(states) = self.installations.get_mut(root)
            && matches!(states.get(version), Some(Installation::Complete))
        {
            states.remove(version);
        }
    }
}
