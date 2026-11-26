// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Profile discovery, selection and desktop actions.

use std::path::{Path, PathBuf};

use axlauncher_core::desktop::launching::LaunchJob;
use axlauncher_core::state::profile::{DiscoveredProfile, ProfileDiscovery, discover};
use axlauncher_core::storage::DataDir;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, Context, ElementId, GroupBox, Input, IntoElement, Label, h_flex,
    v_flex,
};

use crate::instance_row;

use super::{RootView, launch};

pub(super) struct InstancesState {
    discovered: ProfileDiscovery,
    selected: Option<PathBuf>,
}

impl InstancesState {
    pub(super) fn new(discovered: ProfileDiscovery) -> Self {
        let selected = discovered
            .instances
            .first()
            .map(|entry| entry.directory.clone());
        Self {
            discovered,
            selected,
        }
    }

    pub(super) fn replace(&mut self, discovered: ProfileDiscovery, preserve_selection: bool) {
        let previous = self.selected().filter(|_| preserve_selection);
        let selected = previous.and_then(|entry| find_previous(&discovered.instances, entry));
        self.selected = selected
            .or_else(|| discovered.instances.first())
            .map(|entry| entry.directory.clone());
        self.discovered = discovered;
    }

    pub(super) fn refresh_after_install(
        &mut self,
        current: Option<&DataDir>,
        job: &DataDir,
    ) -> bool {
        let instances = job.instances();
        if current.map(DataDir::instances).as_ref() != Some(&instances) {
            return false;
        }
        self.replace(discover(&instances), true);
        true
    }

    pub(super) fn selected(&self) -> Option<&DiscoveredProfile> {
        self.find(self.selected.as_deref()?)
    }

    pub(super) fn select(&mut self, directory: &Path) {
        if self.find(directory).is_some() {
            self.selected = Some(directory.to_path_buf());
        }
    }

    pub(super) fn find(&self, directory: &Path) -> Option<&DiscoveredProfile> {
        self.discovered
            .instances
            .iter()
            .find(|entry| entry.directory == directory)
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/frontend/src/panel/instances.rs"]
mod tests;

fn find_previous<'a>(
    entries: &'a [DiscoveredProfile],
    previous: &DiscoveredProfile,
) -> Option<&'a DiscoveredProfile> {
    let mut same_id = entries
        .iter()
        .filter(|entry| entry.profile.id() == previous.profile.id());
    let first = same_id.next()?;
    if first.directory == previous.directory || same_id.clone().next().is_none() {
        return Some(first);
    }
    same_id.find(|entry| entry.directory == previous.directory)
}

impl RootView {
    pub(crate) fn select_instance(&mut self, directory: PathBuf, cx: &mut Context<'_, Self>) {
        if self.instances.selected.as_ref() == Some(&directory) {
            return;
        }
        self.instances.selected = Some(directory);
        self.inspection = launch::Inspection::default();
        cx.notify();
    }

    pub(super) fn launch_job(
        &self,
        entry: &DiscoveredProfile,
        player_name: String,
    ) -> Option<LaunchJob> {
        LaunchJob::for_profile(
            self.paths.as_ref()?,
            entry,
            player_name,
            self.settings.as_ref(),
        )
    }

    pub(super) fn render_instances(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let danger = cx.theme().danger;
        let profiles = &self.instances.discovered;
        let installations = self
            .paths
            .as_ref()
            .and_then(|data| self.installations.get(data.game().root()));

        GroupBox::new()
            .id("instances")
            .title(Label::new("Instances").text_sm().text_color(muted))
            .child(
                v_flex()
                    .gap_3()
                    .child(self.modpack.clone())
                    .children(profiles.issues.iter().enumerate().map(|(index, issue)| {
                        Label::new(issue.to_string())
                            .id(ElementId::named_usize("profile-discovery-issue", index))
                            .text_xs()
                            .text_color(danger)
                    }))
                    .when(
                        profiles.instances.is_empty() && profiles.issues.is_empty(),
                        |column| {
                            column
                                .child(Label::new("No profiles found.").text_sm().text_color(muted))
                        },
                    )
                    .child(self.render_profile_creation(cx))
                    .when(
                        !profiles.instances.is_empty() && self.accounts.selected().is_none(),
                        |column| column.child(self.offline_player()),
                    )
                    .children(profiles.instances.iter().map(|entry| {
                        let selected = self.instances.selected.as_ref() == Some(&entry.directory);
                        let row = instance_row::row(
                            entry,
                            selected,
                            installations,
                            self.launches.get(&entry.directory),
                            self.account_selection_pending()
                                || self.java_write_pending(entry.profile.id(), cx),
                            muted,
                            cx,
                        );
                        v_flex()
                            .gap_2()
                            .child(row)
                            .when(selected, |row| row.children(self.selected_java_picker()))
                    })),
            )
            .into_any_element()
    }

    fn offline_player(&self) -> impl IntoElement + use<> {
        h_flex()
            .gap_3()
            .child(Label::new("Offline player name").text_sm())
            .child(
                Input::new(&self.player_input)
                    .id("offline-player-name")
                    .aria_label("Offline player name")
                    .w_48(),
            )
    }
}
