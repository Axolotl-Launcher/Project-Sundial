// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Profile discovery, selection and desktop actions.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axlauncher_core::desktop::{
    instance_settings::{self, EditCancellation, InstanceHeap, InstanceTarget},
    launching::LaunchJob,
};
use axlauncher_core::domain::memory::MaximumHeap;
use axlauncher_core::state::profile::{DiscoveredProfile, ProfileDiscovery, discover};
use axlauncher_core::storage::{DataDir, DataPath};
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, App, Button, Context, Disableable, ElementId, GroupBox, Hsla, Input,
    IntoElement, Label, Sizable, Task, WeakEntity, Window, h_flex, v_flex,
};

use crate::installations::Installation;

use super::{
    RootView,
    java::{InstancePicker, memory::open_editor},
    memory_edits::MemoryScope,
};

mod actions;
mod folders;
mod instance_row;
pub(super) mod launch;

use instance_row::RowAction;

#[derive(Clone)]
struct InstanceMemoryEdit {
    paths: DataDir,
    scope: MemoryScope,
    target: Arc<InstanceTarget>,
    expected: Option<MaximumHeap>,
}

pub(super) struct InstancesState {
    discovered: ProfileDiscovery,
    selected: Option<PathBuf>,
    installing: HashSet<PathBuf>,
    deleting: HashSet<PathBuf>,
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
            installing: HashSet::new(),
            deleting: HashSet::new(),
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

    pub(super) fn refresh_for(&mut self, current: Option<&DataDir>, job: &DataDir) -> bool {
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

    fn refresh_heap(
        &mut self,
        target: &InstanceTarget,
        expected: Option<MaximumHeap>,
        memory: Option<MaximumHeap>,
    ) -> Result<(), String> {
        let entry = self
            .discovered
            .instances
            .iter_mut()
            .find(|entry| {
                entry.directory == target.directory() && entry.profile.id() == target.id()
            })
            .ok_or("This instance changed. Reopen the editor to continue.")?;
        if entry.profile.memory() == expected {
            entry.profile.apply_heap_snapshot(memory);
        }
        Ok(())
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
    ) -> Option<Button> {
        entry.profile.game()?;
        let directory = entry.directory.clone();
        let label = if entry.profile.memory().is_some() {
            "Memory · Manual"
        } else {
            "Memory · Global"
        };
        let button =
            Button::new(ElementId::Name(
                format!("instance-memory:{}", directory.display()).into(),
            ))
            .label(label)
            .accessibility_label(format!(
                "Configure memory for {}",
                entry.profile.instance.name
            ))
            .small()
            .disabled(self.instances.deleting.contains(&directory))
            .on_click(cx.listener(move |view, _, window, cx| {
                view.open_instance_memory(&directory, window, cx)
            }));
        Some(button)
    }

    fn open_instance_memory(
        &mut self,
        directory: &Path,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some((entry, paths)) = self.instances.find(directory).zip(self.paths.clone()) else {
            return;
        };
        let target = std::sync::Arc::new(InstanceTarget::new(entry, &paths));
        let scope = self.memory_edits.scope();
        let expected = entry.profile.memory();
        let title = format!("Memory for {}", entry.profile.instance.name);
        let suggested = self
            .settings
            .as_ref()
            .and_then(|settings| settings.memory().manual_heap())
            .unwrap_or_default();
        let view = cx.weak_entity();
        let edit = InstanceMemoryEdit {
            paths,
            scope,
            target,
            expected,
        };
        open_editor(
            title,
            "Follow global settings",
            expected,
            suggested,
            window,
            cx,
            move |heap, cancel, cx| {
                Self::start_instance_memory_edit(view.clone(), edit.clone(), heap, cancel, cx)
            },
        );
    }

    fn commit_instance_memory_edit(
        &mut self,
        edit: InstanceMemoryEdit,
        heap: Option<MaximumHeap>,
        cancel: Arc<EditCancellation>,
        cx: &mut Context<'_, Self>,
    ) -> Result<Task<Result<(), String>>, String> {
        let InstanceMemoryEdit {
            paths,
            scope,
            target,
            expected,
        } = edit;
        if self.instances.deleting.contains(target.directory())
            || !scope.matches(&self.memory_edits.scope())
            || self
                .instances
                .find(target.directory())
                .is_none_or(|entry| entry.profile.id() != target.id())
        {
            return Err("This instance changed. Reopen the editor to continue.".into());
        }
        let choice = heap.map_or(InstanceHeap::Inherit, |heap| {
            InstanceHeap::ManualMib(heap.mib())
        });
        let selected = target.clone();
        let edited = target;
        Ok(self.save_memory(
            scope,
            cancel,
            cx,
            move |cancel| {
                instance_settings::set_instance_heap(&paths, &selected, choice, cancel)
                    .map_err(|error| error.to_string())
            },
            move |view, memory, cx| view.apply_instance_memory(&edited, expected, memory, cx),
        ))
    }

    fn apply_instance_memory(
        &mut self,
        target: &InstanceTarget,
        expected: Option<MaximumHeap>,
        memory: Option<MaximumHeap>,
        cx: &mut Context<'_, Self>,
    ) -> Result<(), String> {
        self.instances.refresh_heap(target, expected, memory)?;
        self.invalidate_inspection();
        cx.notify();
        Ok(())
    }

    fn start_instance_memory_edit(
        view: WeakEntity<Self>,
        edit: InstanceMemoryEdit,
        heap: Option<MaximumHeap>,
        cancel: Arc<EditCancellation>,
        cx: &mut App,
    ) -> Result<Task<Result<(), String>>, String> {
        view.update(cx, |view, cx| {
            view.commit_instance_memory_edit(edit, heap, cancel, cx)
        })
        .map_err(|error| error.to_string())?
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
                    .child(self.profile_creation.clone())
                    .when(
                        !profiles.instances.is_empty()
                            && self.accounts.read(cx).selected().is_none(),
                        |column| column.child(self.offline_player()),
                    )
                    .children(
                        profiles
                            .instances
                            .iter()
                            .map(|entry| self.render_instance(entry, installations, muted, cx)),
                    ),
            )
            .into_any_element()
    }

    fn render_instance(
        &self,
        entry: &DiscoveredProfile,
        installations: Option<&HashMap<String, Installation>>,
        muted: Hsla,
        cx: &mut Context<'_, Self>,
    ) -> impl IntoElement + use<> {
        let selected = self.instances.selected.as_ref() == Some(&entry.directory);
        let row = instance_row::row(
            entry,
            selected,
            installations,
            self.launches.get(&entry.directory),
            instance_row::Availability {
                deleting: self.instances.deleting.contains(&entry.directory),
                launch_pending: self.account_selection_pending(cx)
                    || self.java_write_pending(entry.profile.id(), cx),
            },
            muted,
            Self::row_listener(entry.directory.clone(), cx),
        );
        v_flex()
            .gap_2()
            .child(row)
            .child(InstancePicker::new(self.java.clone(), entry))
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .children(self.instance_memory_button(entry, cx))
                    .children(self.instance_folder_buttons(entry, cx)),
            )
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
