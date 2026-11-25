// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Profile discovery, selection and desktop actions.

use std::path::{Path, PathBuf};

use axlauncher_core::desktop::launching::LaunchJob;
use axlauncher_core::state::profile::{DiscoveredProfile, ProfileDiscovery};
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

    pub(super) fn replace(&mut self, discovered: ProfileDiscovery, same_root: bool) {
        let previous = self.selected().filter(|_| same_root);
        let selected = previous.and_then(|entry| find_previous(&discovered.instances, entry));
        self.selected = selected
            .or_else(|| discovered.instances.first())
            .map(|entry| entry.directory.clone());
        self.discovered = discovered;
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
            self.root.as_deref()?,
            entry,
            player_name,
            self.settings.as_ref(),
        )
    }

    pub(super) fn render_instances(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let danger = cx.theme().danger;
        let profiles = &self.instances.discovered;
        let data = self.root.as_ref().map(DataDir::new);
        let installations = data
            .as_ref()
            .and_then(|data| self.installations.get(&data.meta()));

        GroupBox::new()
            .id("instances")
            .title(Label::new("Instances").text_sm().text_color(muted))
            .child(
                v_flex()
                    .gap_3()
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
                        |column| {
                            column.child(
                                h_flex()
                                    .gap_3()
                                    .child(Label::new("Offline player name").text_sm())
                                    .child(
                                        Input::new(&self.player_input)
                                            .id("offline-player-name")
                                            .aria_label("Offline player name")
                                            .w_48(),
                                    ),
                            )
                        },
                    )
                    .children(profiles.instances.iter().map(|entry| {
                        instance_row::row(
                            entry,
                            self.instances.selected.as_ref() == Some(&entry.directory),
                            installations,
                            self.launches.get(&entry.directory),
                            self.account_selection_pending(),
                            muted,
                            cx,
                        )
                    })),
            )
            .into_any_element()
    }
}
