// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Profile folder commands and their confirmation flow.

use std::io;
use std::path::{Path, PathBuf};

use axlauncher_core::state::profile::{self, DiscoveredProfile};
use axlauncher_core::storage::{DataDir, DataPath, FolderRemoval};
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    App, AsyncWindowContext, Button, ButtonVariants, Context, DialogFooter, Disableable, ElementId,
    Notification, Sizable, WeakEntity, Window, WindowExt,
};

use super::RootView;

#[derive(Clone)]
struct InstanceFolder {
    paths: DataDir,
    directory: PathBuf,
    id: String,
    name: String,
}

impl RootView {
    pub(super) fn instance_folder_buttons(
        &self,
        entry: &DiscoveredProfile,
        cx: &Context<'_, Self>,
    ) -> [Button; 2] {
        let directory = &entry.directory;
        let name = &entry.profile.instance.name;
        let deleting = self.instances.deleting.contains(directory);
        let open_directory = directory.clone();
        let delete_directory = directory.clone();
        [
            Button::new(ElementId::Name(
                format!("open-folder:{}", directory.display()).into(),
            ))
            .small()
            .ghost()
            .label("Open Folder")
            .accessibility_label(format!("Open Folder for {name}"))
            .disabled(deleting)
            .on_click(cx.listener(move |view, _, window, cx| {
                view.open_instance_folder(&open_directory, window, cx);
            })),
            Button::new(ElementId::Name(
                format!("delete:{}", directory.display()).into(),
            ))
            .small()
            .ghost()
            .label(if deleting { "Deleting…" } else { "Delete…" })
            .accessibility_label(format!("Delete {name}"))
            .disabled(self.instance_files_busy(directory))
            .on_click(cx.listener(move |view, _, window, cx| {
                view.confirm_instance_deletion(&delete_directory, window, cx);
            })),
        ]
    }

    fn instance_files_busy(&self, directory: &Path) -> bool {
        self.instances.installing.contains(directory)
            || self.instances.deleting.contains(directory)
            || self
                .launches
                .get(directory)
                .is_some_and(|(_, launch)| launch.is_active())
    }

    fn instance_folder(&self, directory: &Path) -> Option<InstanceFolder> {
        let entry = self.instances.find(directory)?;
        Some(InstanceFolder {
            paths: self.paths.clone()?,
            directory: entry.directory.clone(),
            id: entry.profile.id().to_owned(),
            name: entry.profile.instance.name.clone(),
        })
    }

    fn open_instance_folder(
        &self,
        directory: &Path,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(folder) = self.instance_folder(directory) else {
            return;
        };
        let task = cx.background_spawn(async move {
            profile::open_folder(&folder.paths, &folder.directory, &folder.id)
        });
        cx.spawn_in(window, async move |_, cx| {
            Self::report_open_folder(task.await, cx);
        })
        .detach();
    }

    fn report_open_folder(result: io::Result<()>, cx: &mut AsyncWindowContext) {
        let Err(error) = result else { return };
        let notification =
            Notification::error(error.to_string()).title("Could not open the instance folder");
        let _ = cx.update(|window, cx| window.push_notification(notification, cx));
    }

    fn confirm_instance_deletion(
        &self,
        directory: &Path,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(folder) = self.instance_folder(directory) else {
            return;
        };
        let view = cx.weak_entity();
        window.open_alert_dialog(cx, move |dialog, _, _| {
            let target = folder.clone();
            let view = view.clone();
            let cancel = Button::new("cancel-delete-instance")
                .label("Cancel")
                .on_click(|_, window, cx| window.close_dialog(cx));
            let delete = Button::new("confirm-delete-instance")
                .danger()
                .label("Delete")
                .on_click(move |_, window, cx| Self::submit_deletion(&view, &target, window, cx));
            dialog
                .title(format!("Delete “{}”?", folder.name))
                .description(concat!(
                    "The instance folder and all its files will go to the recycle bin, ",
                    "or be permanently deleted if the recycle bin is unavailable."
                ))
                // Enter on the dialog itself must not commit a destructive action.
                .on_ok(|_, _, _| false)
                .footer(DialogFooter::new().child(cancel).child(delete))
        });
    }

    fn submit_deletion(
        view: &WeakEntity<Self>,
        target: &InstanceFolder,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.close_dialog(cx);
        let _ = view.update(cx, |view, cx| {
            view.delete_instance(target.clone(), window, cx)
        });
    }

    fn delete_instance(
        &mut self,
        folder: InstanceFolder,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let current_root = self
            .paths
            .as_ref()
            .map(|paths| paths.path(DataPath::Profiles));
        let current_id = self
            .instances
            .find(&folder.directory)
            .map(|entry| entry.profile.id());
        if current_root != Some(folder.paths.path(DataPath::Profiles))
            || current_id != Some(&folder.id)
        {
            window.push_notification(
                Notification::error("This instance changed. Reopen the confirmation to continue."),
                cx,
            );
            return;
        }
        if self.instance_files_busy(&folder.directory) {
            window.push_notification(
                Notification::error(
                    "Close Minecraft or let the installation finish before deleting this instance.",
                ),
                cx,
            );
            return;
        }
        self.instances.deleting.insert(folder.directory.clone());
        let target = folder.clone();
        let deletion = cx.background_spawn(async move {
            profile::delete_folder(&target.paths, &target.directory, &target.id)
        });
        cx.spawn_in(window, async move |view, cx| {
            Self::apply_deletion(view, &folder, deletion.await, cx);
        })
        .detach();
        cx.notify();
    }

    fn apply_deletion(
        view: WeakEntity<Self>,
        folder: &InstanceFolder,
        result: io::Result<FolderRemoval>,
        cx: &mut AsyncWindowContext,
    ) {
        let _ = view.update_in(cx, |view, window, cx| {
            view.finish_deletion(folder, result, window, cx)
        });
    }

    fn finish_deletion(
        &mut self,
        folder: &InstanceFolder,
        result: io::Result<FolderRemoval>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.instances.deleting.remove(&folder.directory);
        let name = &folder.name;
        cx.notify();
        let removal = match result {
            Ok(removal) => removal,
            Err(error) => {
                let notification = Notification::error(error.to_string())
                    .title(format!("Could not delete {name}"));
                return window.push_notification(notification, cx);
            }
        };
        self.launches.remove(&folder.directory);
        if self
            .instances
            .refresh_for(self.paths.as_ref(), &folder.paths)
        {
            self.invalidate_inspection();
        }
        let message = match removal {
            FolderRemoval::Recycled => format!("Moved {name} to the recycle bin"),
            FolderRemoval::PermanentlyDeleted => format!("Permanently deleted {name}"),
        };
        self.activity.success(message);
    }
}
