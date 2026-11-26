// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use axlauncher_core::storage::DataDir;
use std::sync::Arc;

use axlauncher_ui::{AppContext, AsyncWindowContext, Context, Task, WeakEntity, Window};
use tokio::sync::watch;

use super::{ImportEvent, ManualDownloads, ModpackImport, Progress, State};
use crate::modpacks::service::{self, Outcome, ProgressUpdate};
use axlauncher_core::desktop::sources::DownloadOptions;

impl ModpackImport {
    pub(crate) fn install_progress(&self) -> Option<(&str, Progress)> {
        match &self.state {
            State::Installing(preview, progress) => Some((preview.pack.name(), *progress)),
            _ => None,
        }
    }

    pub(super) fn request_install(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) {
        if self.enabled && matches!(self.state, State::Preview(_)) && !self.manual_picker_open(cx) {
            cx.emit(ImportEvent::InstallRequested);
        }
    }

    pub(crate) fn install(
        &mut self,
        paths: DataDir,
        options: DownloadOptions,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let previous = std::mem::replace(&mut self.state, State::Idle);
        let State::Preview(mut preview) = previous else {
            self.state = previous;
            return;
        };
        let (progress, updates) = watch::channel(Progress::default());
        let pack = Arc::clone(&preview.pack);
        let selected = preview.selected.clone();
        let local = preview.manual.read(cx).selected_paths();
        let policy = preview.policy;
        let pending = preview.pending.take();
        self.state = State::Installing(preview, *updates.borrow());
        self.error = None;
        let task = cx.background_spawn(async move {
            let report = |change| report_progress(&progress, change);
            match pending {
                Some(install) => install.resume(&local, options, report),
                None => service::install(paths, &pack, &selected, policy, options, report),
            }
        });
        cx.spawn_in(window, async move |view, cx| {
            Self::observe_install(view, cx, updates, task).await;
        })
        .detach();
        cx.notify();
    }

    async fn observe_install(
        view: WeakEntity<Self>,
        cx: &mut AsyncWindowContext,
        mut updates: watch::Receiver<Progress>,
        task: Task<Result<Outcome, String>>,
    ) {
        while updates.changed().await.is_ok() {
            let progress = *updates.borrow_and_update();
            let _ = view.update_in(cx, |view, _, cx| view.update_modpack_progress(progress, cx));
        }
        let outcome = task.await;
        let _ = view.update_in(cx, |view, window, cx| {
            view.finish_modpack(outcome, window, cx)
        });
    }

    fn finish_modpack(
        &mut self,
        result: Result<Outcome, String>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let State::Installing(mut preview, _) = std::mem::replace(&mut self.state, State::Idle)
        else {
            return;
        };
        match result {
            Ok(Outcome::Installed {
                paths,
                directory,
                root_data_packs,
            }) => {
                self.root_data_packs = root_data_packs;
                cx.emit(ImportEvent::Installed {
                    paths,
                    directory,
                    name: preview.pack.name().to_owned(),
                });
            }
            Err(error) => {
                ManualDownloads::replace_pending(&preview.manual, Vec::new(), cx);
                self.error = Some(error);
                self.state = State::Preview(preview);
            }
            Ok(Outcome::Pending {
                install,
                files,
                error,
            }) => {
                ManualDownloads::replace_pending(&preview.manual, files, cx);
                self.error = error;
                preview.pending = Some(install);
                self.state = State::Preview(preview);
            }
        }
        self.focus_modpack(window, cx);
    }

    fn update_modpack_progress(&mut self, progress: Progress, cx: &mut Context<'_, Self>) {
        if let State::Installing(_, previous) = &mut self.state {
            *previous = progress;
            cx.notify();
        }
    }
}

fn report_progress(progress: &watch::Sender<Progress>, change: ProgressUpdate) {
    progress.send_if_modified(|state| state.update(change));
}
