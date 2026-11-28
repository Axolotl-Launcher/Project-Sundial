// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Profile discovery, selection and desktop actions.

use std::path::{Path, PathBuf};

use axlauncher_core::desktop::launching::LaunchJob;
use axlauncher_core::desktop::memory;
use axlauncher_core::domain::memory::MaximumHeap;
use axlauncher_core::state::profile::{DiscoveredProfile, Profile, ProfileDiscovery, discover};
use axlauncher_core::storage::{DataDir, DataPath};
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, App, Button, Context, ElementId, GroupBox, Input, IntoElement, Label,
    Sizable, WeakEntity, Window, h_flex, v_flex,
};

use crate::i18n::{interpolate, tr};

use super::{
    RootView,
    java::{
        InstancePicker,
        memory::{MemoryScope, open_editor},
    },
};

mod actions;
mod instance_row;
pub(super) mod launch;

use instance_row::RowAction;

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
        let instances = job.path(DataPath::Profiles);
        if current.map(|paths| paths.path(DataPath::Profiles)) != Some(instances) {
            return false;
        }
        self.replace(discover(instances), true);
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

    pub(super) fn update_profile(&mut self, directory: &Path, profile: Profile) {
        if let Some(entry) = self
            .discovered
            .instances
            .iter_mut()
            .find(|entry| entry.directory == directory)
        {
            entry.profile = profile;
        }
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
        self.invalidate_inspection();
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

    fn instance_memory_button(
        &self,
        entry: &DiscoveredProfile,
        cx: &mut Context<'_, Self>,
    ) -> Option<AnyElement> {
        entry.profile.game()?;
        let directory = entry.directory.clone();
        let label = if entry.profile.memory().is_some() {
            tr(cx, "instances.memory.manual")
        } else {
            tr(cx, "instances.memory.global")
        };
        let button =
            Button::new(ElementId::Name(
                format!("instance-memory:{}", directory.display()).into(),
            ))
            .label(label)
            .accessibility_label(interpolate(
                tr(cx, "instances.memory.configure-name"),
                &[("name", &entry.profile.instance.name)],
            ))
            .small()
            .on_click(cx.listener(move |view, _, window, cx| {
                view.open_instance_memory(&directory, window, cx)
            }));
        Some(h_flex().child(button).into_any_element())
    }

    fn open_instance_memory(
        &mut self,
        directory: &Path,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(entry) = self.instances.find(directory) else {
            return;
        };
        let directory = entry.directory.clone();
        let id = entry.profile.id().to_owned();
        let scope = MemoryScope::Instance(entry.profile.instance.name.clone());
        let suggested = self
            .settings
            .as_ref()
            .and_then(|settings| settings.memory().manual_heap())
            .unwrap_or_default();
        let view = cx.weak_entity();
        open_editor(
            scope,
            entry.profile.memory(),
            suggested,
            window,
            cx,
            move |heap, cx| {
                view.update(cx, |view, cx| view.save_heap(&directory, &id, heap, cx))
                    .map_err(|error| error.to_string())?
            },
        );
    }

    fn save_heap(
        &mut self,
        directory: &Path,
        id: &str,
        heap: Option<MaximumHeap>,
        cx: &mut Context<'_, Self>,
    ) -> Result<(), String> {
        if self
            .instances
            .find(directory)
            .is_none_or(|entry| entry.profile.id() != id)
        {
            return Err(tr(cx, "instances.memory.instance-changed").into());
        }
        let updated =
            memory::set_instance(directory, id, heap).map_err(|error| error.to_string())?;
        self.instances.update_profile(directory, updated);
        self.invalidate_inspection();
        cx.notify();
        Ok(())
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
            .title(
                Label::new(tr(cx, "instances.title"))
                    .text_sm()
                    .text_color(muted),
            )
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
                            column.child(
                                Label::new(tr(cx, "instances.empty"))
                                    .text_sm()
                                    .text_color(muted),
                            )
                        },
                    )
                    .child(self.profile_creation.clone())
                    .when(
                        !profiles.instances.is_empty()
                            && self.accounts.read(cx).selected().is_none(),
                        |column| column.child(self.offline_player(cx)),
                    )
                    .children(profiles.instances.iter().map(|entry| {
                        let selected = self.instances.selected.as_ref() == Some(&entry.directory);
                        let row = instance_row::row(
                            entry,
                            selected,
                            installations,
                            self.launches.get(&entry.directory),
                            self.account_selection_pending(cx)
                                || self.java_write_pending(entry.profile.id(), cx),
                            cx,
                            Self::row_listener(entry.directory.clone(), cx),
                        );
                        v_flex()
                            .gap_2()
                            .child(row)
                            .child(InstancePicker::new(self.java.clone(), entry))
                            .children(self.instance_memory_button(entry, cx))
                    })),
            )
            .into_any_element()
    }

    fn row_listener(
        directory: PathBuf,
        cx: &Context<'_, Self>,
    ) -> impl Fn(RowAction, &mut App) + Clone + 'static {
        let view = cx.weak_entity();
        move |action, cx| Self::dispatch_row_action(&view, &directory, action, cx)
    }

    fn dispatch_row_action(
        view: &WeakEntity<Self>,
        directory: &Path,
        action: RowAction,
        cx: &mut App,
    ) {
        let _ = view.update(cx, |view, cx| view.handle_row_action(directory, action, cx));
    }

    fn handle_row_action(
        &mut self,
        directory: &Path,
        action: RowAction,
        cx: &mut Context<'_, Self>,
    ) {
        match action {
            RowAction::Select => self.select_instance(directory.to_path_buf(), cx),
            RowAction::Launch => self.launch(directory, cx),
            RowAction::Install => self.install_profile(directory, cx),
        }
    }

    fn install_profile(&mut self, directory: &Path, cx: &mut Context<'_, Self>) {
        let Some(game) = self
            .instances
            .find(directory)
            .and_then(|entry| entry.profile.game())
        else {
            return;
        };
        let game = game.clone();
        self.install_game(directory, &game, cx);
    }

    fn offline_player(&self, cx: &axlauncher_ui::App) -> impl IntoElement + use<> {
        h_flex()
            .gap_3()
            .child(Label::new(tr(cx, "instances.offline-player")).text_sm())
            .child(
                Input::new(&self.player_input)
                    .id("offline-player-name")
                    .aria_label(tr(cx, "instances.offline-player"))
                    .w_48(),
            )
    }
}
