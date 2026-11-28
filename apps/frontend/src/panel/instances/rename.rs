// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Coordinates instance rename jobs with the rest of the panel.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axlauncher_core::desktop::instance_settings::{self, EditCancellation, InstanceTarget};
use axlauncher_core::state::profile::{DiscoveredProfile, ProfileFolderName};
use axlauncher_core::storage::{DataDir, DataPath};
use axlauncher_ui::{
    App, AppContext, AsyncApp, Button, ButtonVariants, Context, Disableable, ElementId, Sizable,
    Task, WeakEntity, Window,
};

mod dialog;
use super::RootView;

impl RootView {
    pub(super) fn rename_instance_button(
        &self,
        entry: &DiscoveredProfile,
        cx: &Context<'_, Self>,
    ) -> Button {
        let directory = entry.directory.clone();
        let id = ElementId::Name(format!("rename:{}", directory.display()).into());
        Button::new(id)
            .small()
            .ghost()
            .label("Rename…")
            .accessibility_label(format!("Rename {}", entry.profile.instance.name))
            .disabled(self.instance_files_busy(&directory))
            .on_click(cx.listener(move |view, _, window, cx| {
                view.open_rename(&directory, window, cx);
            }))
    }

    fn open_rename(&self, directory: &Path, window: &mut Window, cx: &mut Context<'_, Self>) {
        let Some((entry, paths)) = self.instances.find(directory).zip(self.paths.clone()) else {
            return;
        };
        let target = Arc::new(InstanceTarget::new(entry, &paths));
        let root = cx.weak_entity();
        let name = entry.profile.instance.name.clone();
        dialog::open(name, window, cx, move |name, folder, cx| {
            Self::request_rename(&root, paths.clone(), target.clone(), name, folder, cx)
        });
    }

    fn request_rename(
        root: &WeakEntity<Self>,
        paths: DataDir,
        target: Arc<InstanceTarget>,
        name: String,
        folder: Option<ProfileFolderName>,
        cx: &mut App,
    ) -> Result<Task<Result<(), String>>, String> {
        root.update(cx, |view, cx| {
            view.start_rename(paths, target, name, folder, cx)
        })
        .map_err(|error| error.to_string())?
    }

    fn start_rename(
        &mut self,
        paths: DataDir,
        target: Arc<InstanceTarget>,
        name: String,
        folder: Option<ProfileFolderName>,
        cx: &mut Context<'_, Self>,
    ) -> Result<Task<Result<(), String>>, String> {
        let directory = target.directory();
        let same_paths = self.paths.as_ref().is_some_and(|current| {
            current.path(DataPath::Root) == paths.path(DataPath::Root)
                && current.path(DataPath::Profiles) == paths.path(DataPath::Profiles)
        });
        if !same_paths
            || self
                .instances
                .find(directory)
                .is_none_or(|entry| entry.profile.id() != target.id())
        {
            return Err("This instance changed. Reopen the dialog to continue.".into());
        }
        if self.instance_files_busy(directory) {
            return Err(
                "Close Minecraft or let the current instance operation finish before renaming."
                    .into(),
            );
        }
        self.instances.renaming.insert(directory.to_owned());
        let request_paths = paths.clone();
        let request_target = target.clone();
        let task = cx.background_spawn(async move {
            save_rename(&request_paths, &request_target, &name, folder)
        });
        cx.notify();
        Ok(cx.spawn(async move |view, cx| {
            Self::apply_rename(view, &paths, target.directory(), task.await, cx)
        }))
    }

    fn apply_rename(
        view: WeakEntity<Self>,
        paths: &DataDir,
        original: &Path,
        result: Result<PathBuf, String>,
        cx: &mut AsyncApp,
    ) -> Result<(), String> {
        view.update(cx, |view, cx| {
            view.finish_rename(paths, original, result.as_deref().ok(), cx)
        })
        .map_err(|error| error.to_string())?;
        result.map(|_| ())
    }

    fn finish_rename(
        &mut self,
        paths: &DataDir,
        original: &Path,
        destination: Option<&Path>,
        cx: &mut Context<'_, Self>,
    ) {
        self.instances.renaming.remove(original);
        if let Some(destination) = destination.filter(|path| *path != original)
            && let Some(launch) = self.launches.remove(original)
        {
            self.launches.insert(destination.to_owned(), launch);
        }
        let was_selected = self.instances.selected.as_deref() == Some(original);
        cx.notify();
        if !self.instances.refresh_for(self.paths.as_ref(), paths) {
            return;
        }
        if let Some(destination) = destination.filter(|_| was_selected) {
            self.instances.select(destination);
        }
        self.invalidate_inspection();
    }
}

fn save_rename(
    paths: &DataDir,
    target: &InstanceTarget,
    name: &str,
    folder: Option<ProfileFolderName>,
) -> Result<PathBuf, String> {
    let cancel = EditCancellation::default();
    let renamed = match folder {
        Some(folder) => {
            instance_settings::rename_with_folder(paths, target, name, &folder, &cancel)
        }
        None => instance_settings::rename(paths, target, name, &cancel)
            .map(|_| target.directory().to_owned()),
    };
    renamed.map_err(|error| error.to_string())
}
