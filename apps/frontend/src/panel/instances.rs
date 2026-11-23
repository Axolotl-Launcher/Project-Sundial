// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Profile discovery, selection and desktop actions.

use std::path::{Path, PathBuf};

use axlauncher_core::launch::LaunchRequest;
use axlauncher_core::state::profile::{DiscoveredProfile, Game, ProfileDiscovery};
use axlauncher_core::storage::DataDir;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, Button, ButtonVariants, Context, ElementId, GroupBox, Input,
    IntoElement, Label, h_flex, v_flex,
};

use crate::installations::vanilla_version;
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

pub(crate) fn profile_version(game: &Game) -> Option<&str> {
    game.version_id.as_deref().or_else(|| vanilla_version(game))
}

/// The key an install is tracked under.
///
/// A loader profile has no version id until its install produces one, so the
/// install is tracked under what the profile asked for: the Minecraft version,
/// the loader kind and the loader version it named. Two profiles with different
/// loaders, or different loader versions, need different installs, and a
/// vanilla install of a version must not make a loader profile look installed.
/// A loader this build cannot install has no key, which keeps an action that
/// would fail at the end of a download off the row.
pub(crate) fn install_key(game: &Game) -> Option<String> {
    let Some(loader) = game.loader.as_ref() else {
        return Some(game.minecraft.clone());
    };
    if crate::installations::is_vanilla(&loader.kind) {
        return Some(game.minecraft.clone());
    }
    if !crate::installations::supported_loader(&loader.kind) {
        return None;
    }
    // Two markers that name the same loader in different case must share one
    // slot, or installing one leaves the other looking uninstalled forever.
    let kind = axlauncher_core::spi::LoaderKind::parse(&loader.kind)?.as_str();
    Some(match loader.version.as_deref() {
        Some(version) if !version.trim().is_empty() => {
            format!("{}:{}:{}", game.minecraft, kind, version.trim())
        }
        _ => format!("{}:{}", game.minecraft, kind),
    })
}

fn find_previous<'a>(
    entries: &'a [DiscoveredProfile],
    previous: &DiscoveredProfile,
) -> Option<&'a DiscoveredProfile> {
    let mut same_id = entries
        .iter()
        .filter(|entry| entry.profile.id == previous.profile.id);
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

    pub(super) fn launch_request(
        &self,
        entry: &DiscoveredProfile,
        player_name: String,
    ) -> Option<LaunchRequest> {
        let version = profile_version(entry.profile.game.as_ref()?)?;
        Some(LaunchRequest {
            game_root: DataDir::new(self.root.as_ref()?).meta(),
            instance_directory: entry.directory.clone(),
            version_id: version.to_owned(),
            java_preference: None,
            player_name,
        })
    }

    fn creation_form(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        h_flex()
            .gap_2()
            .items_end()
            .child(
                Input::new(&self.new_name)
                    .id("new-profile-name")
                    .aria_label("New profile name")
                    .w_40(),
            )
            .child(
                Input::new(&self.new_minecraft)
                    .id("new-profile-minecraft")
                    .aria_label("Minecraft version")
                    .w_32(),
            )
            .child(
                Input::new(&self.new_loader)
                    .id("new-profile-loader")
                    .aria_label("Loader kind: fabric, quilt, or empty")
                    .w_32(),
            )
            .child(
                Input::new(&self.new_loader_version)
                    .id("new-profile-loader-version")
                    .aria_label("Loader version, empty for the recommended one")
                    .w_32(),
            )
            .child(
                Button::new("new-profile-create")
                    .label("Create")
                    .primary()
                    .on_click(cx.listener(|view, _, _, cx| view.create_profile(cx))),
            )
            .into_any_element()
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
                    .child(self.render_create_instance(data.as_ref(), cx))
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
                    .child(self.creation_form(cx))
                    .when(!profiles.instances.is_empty(), |column| {
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
                    })
                    .children(profiles.instances.iter().map(|entry| {
                        instance_row::row(
                            entry,
                            self.instances.selected.as_ref() == Some(&entry.directory),
                            installations,
                            self.launches.get(&entry.directory),
                            muted,
                            cx,
                        )
                    })),
            )
            .into_any_element()
    }
}
