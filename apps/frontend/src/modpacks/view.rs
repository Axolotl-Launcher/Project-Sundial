// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use axlauncher_core::storage::DataDir;
use axlauncher_modpack::{ModpackPlugin, OptionId, OptionalFile, Pack, PackSource, PlanOptions};
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, App, AsyncWindowContext, BackgroundExecutor, Button, ButtonVariants,
    Checkbox, Context, Disableable, ElementId, Entity, EventEmitter, FocusHandle, IntoElement,
    KeyDownEvent, Label, Pagination, Render, SharedString, WeakEntity, Window, h_flex, v_flex,
};
use tokio::sync::watch;

use super::{InstallFailure, Progress, Selection, install};
use crate::game_install::InstallOptions;

mod controls;
mod manual;
use controls::{choose_file, preview_title, progress_label, restricted_download_option};
use manual::ManualDownloads;

pub(crate) enum ImportEvent {
    InstallRequested,
    Installed {
        root: PathBuf,
        directory: PathBuf,
        name: String,
    },
}

impl EventEmitter<ImportEvent> for ModpackImport {}

const OPTIONAL_PAGE_SIZE: usize = 10;

pub(crate) struct ModpackImport {
    plugin: ModpackPlugin,
    trigger: FocusHandle,
    preview_focus: FocusHandle,
    state: State,
    error: Option<String>,
    enabled: bool,
}

enum State {
    Idle,
    Inspecting,
    Preview(Preview),
    Installing(Preview, Progress),
}

struct Preview {
    pack: Arc<Pack>,
    selected: BTreeSet<OptionId>,
    policy: PlanOptions,
    manual: Entity<ManualDownloads>,
    page: usize,
}

impl Preview {
    fn new(pack: Pack, cx: &mut App) -> Self {
        Self {
            pack: Arc::new(pack),
            selected: BTreeSet::new(),
            policy: PlanOptions::default(),
            manual: cx.new(|_| ManualDownloads::default()),
            page: 1,
        }
    }
}

impl ModpackImport {
    pub(crate) fn new(cx: &mut Context<'_, Self>) -> Self {
        Self {
            plugin: ModpackPlugin::builtin(),
            trigger: cx.focus_handle(),
            preview_focus: cx.focus_handle(),
            state: State::Idle,
            error: None,
            enabled: false,
        }
    }
}

impl Render for ModpackImport {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let state = &self.state;
        let danger = cx.theme().danger;
        v_flex()
            .gap_2()
            .child(
                h_flex().child(
                    Button::new("choose-modpack")
                        .label("Import modpack…")
                        .track_focus(&self.trigger)
                        .loading(matches!(state, State::Inspecting))
                        .disabled(!self.enabled || !matches!(state, State::Idle))
                        .on_click(
                            cx.listener(|view, _, window, cx| view.choose_modpack(window, cx)),
                        ),
                ),
            )
            .when_some(
                match state {
                    State::Preview(preview) => Some((preview, None)),
                    State::Installing(preview, progress) => Some((preview, Some(*progress))),
                    _ => None,
                },
                |column, (preview, progress)| {
                    column.child(self.render_modpack_preview(preview, progress, cx))
                },
            )
            .when_some(self.error.clone(), |column, error| {
                column.child(Label::new(error).text_sm().text_color(danger))
            })
            .into_any_element()
    }
}

impl ModpackImport {
    fn render_modpack_preview(
        &self,
        preview: &Preview,
        progress: Option<Progress>,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let pack = &preview.pack;
        let loader = pack.game().loader().map_or_else(
            || "Vanilla".to_owned(),
            |(kind, version)| format!("{kind} {version}"),
        );
        let muted = cx.theme().muted_foreground;
        v_flex()
            .gap_2()
            .on_key_down(cx.listener(Self::modpack_key))
            .child(Label::new(preview_title(pack)).text_sm())
            .child(
                Label::new(format!(
                    "{} · Minecraft {} · {loader}",
                    pack.source().label(),
                    pack.game().minecraft()
                ))
                .text_sm()
                .text_color(muted),
            )
            .child(
                Label::new(format!(
                    "{} required downloads · {} bundled files",
                    pack.required_count(),
                    pack.bundled_count()
                ))
                .text_xs()
                .text_color(muted),
            )
            .when(!pack.optional_files().is_empty(), |column| {
                column.child(self.render_modpack_options(preview, progress.is_some(), cx))
            })
            .when(pack.source() == PackSource::CurseForge, |column| {
                column.child(restricted_download_option(
                    preview.policy.auto_download_restricted(),
                    progress.is_some(),
                    cx.listener(Self::set_automatic),
                ))
            })
            .when(
                progress.is_none() && preview.manual.read(cx).has_pending(),
                |column| column.child(preview.manual.clone()),
            )
            .child(self.render_modpack_actions(progress, preview.manual.read(cx).picking(), cx))
            .into_any_element()
    }

    fn render_modpack_options(
        &self,
        preview: &Preview,
        busy: bool,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let offset = (preview.page - 1) * OPTIONAL_PAGE_SIZE;
        let optional = preview.pack.optional_files();
        let pages = optional.len().div_ceil(OPTIONAL_PAGE_SIZE);
        v_flex()
            .gap_1()
            .child(
                Label::new(format!(
                    "Optional files · {} of {} selected",
                    preview.selected.len(),
                    optional.len()
                ))
                .text_sm(),
            )
            .children(
                optional
                    .iter()
                    .skip(offset)
                    .take(OPTIONAL_PAGE_SIZE)
                    .map(|file| {
                        option_checkbox(file, preview.selected.contains(file.id()), busy, cx)
                    }),
            )
            .when(pages > 1 && !busy, |column| {
                column.child(
                    Pagination::new("modpack-option-pages")
                        .current_page(preview.page)
                        .total_pages(pages)
                        .on_click(cx.listener(Self::select_modpack_page)),
                )
            })
            .into_any_element()
    }

    fn render_modpack_actions(
        &self,
        progress: Option<Progress>,
        picking: bool,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let status = progress.map(progress_label);
        h_flex()
            .gap_2()
            .child(
                Button::new("cancel-modpack")
                    .label("Cancel")
                    .ghost()
                    .disabled(progress.is_some() || picking)
                    .on_click(cx.listener(|view, _, window, cx| view.close_modpack(window, cx))),
            )
            .child(
                Button::new("install-modpack")
                    .label("Install pack")
                    .primary()
                    .track_focus(&self.preview_focus)
                    .loading(progress.is_some())
                    .disabled(!self.enabled || picking)
                    .on_click(cx.listener(|view, _, window, cx| view.request_install(window, cx))),
            )
            .when_some(status, |row, status| {
                row.child(Label::new(status).text_sm())
            })
            .into_any_element()
    }

    fn choose_modpack(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if !matches!(self.state, State::Idle) || !self.enabled {
            return;
        }
        self.state = State::Inspecting;
        self.error = None;
        let selection = choose_file("Choose a modpack archive", cx);
        let executor = cx.background_executor().clone();
        let plugin = self.plugin;
        cx.spawn_in(window, async move |view, cx| {
            let result = inspect_path(selection.await, executor, plugin).await;
            complete_preview(view, cx, result);
        })
        .detach();
        cx.notify();
    }

    fn finish_modpack_preview(
        &mut self,
        result: Result<Option<Pack>, String>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.state = match result {
            Ok(Some(pack)) => State::Preview(Preview::new(pack, cx)),
            Ok(None) => State::Idle,
            Err(error) => {
                self.error = Some(error);
                State::Idle
            }
        };
        self.focus_modpack(window, cx);
    }

    fn select_modpack_option(&mut self, id: &OptionId, checked: bool, cx: &mut Context<'_, Self>) {
        let State::Preview(preview) = &mut self.state else {
            return;
        };
        if checked {
            preview.selected.insert(id.clone());
        } else {
            preview.selected.remove(id);
        }
        cx.notify();
    }

    fn select_modpack_page(&mut self, page: &usize, _: &mut Window, cx: &mut Context<'_, Self>) {
        if let State::Preview(preview) = &mut self.state {
            preview.page = *page;
            cx.notify();
        }
    }

    fn modpack_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if event.keystroke.key == "escape" && matches!(self.state, State::Preview(_)) {
            cx.stop_propagation();
            self.close_modpack(window, cx);
        }
    }

    fn close_modpack(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if matches!(&self.state, State::Preview(preview) if preview.manual.read(cx).picking()) {
            return;
        }
        self.state = State::Idle;
        self.error = None;
        self.focus_modpack(window, cx);
    }

    fn focus_modpack(&self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let focus = if matches!(self.state, State::Preview(_)) {
            &self.preview_focus
        } else {
            &self.trigger
        };
        focus.focus(window, cx);
        cx.notify();
    }

    fn request_install(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) {
        if self.enabled
            && matches!(&self.state, State::Preview(preview) if !preview.manual.read(cx).picking())
        {
            cx.emit(ImportEvent::InstallRequested);
        }
    }

    pub(crate) fn install(
        &mut self,
        root: PathBuf,
        options: InstallOptions,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if !matches!(self.state, State::Preview(_)) {
            return;
        }
        let State::Preview(preview) = std::mem::replace(&mut self.state, State::Idle) else {
            return;
        };
        let (progress, updates) = watch::channel(Progress::Planning);
        let pack = Arc::clone(&preview.pack);
        let selected = preview.selected.clone();
        let local = preview.manual.read(cx).selected_paths();
        let policy = preview.policy;
        let data = DataDir::new(&root);
        self.state = State::Installing(preview, *updates.borrow());
        self.error = None;
        let task = cx.background_spawn(async move {
            let selection = Selection {
                options: &selected,
                local: &local,
                policy,
            };
            install(&data, &pack, selection, options, progress)
        });
        cx.spawn_in(window, async move |view, cx| {
            show_progress(&view, cx, updates).await;
            let outcome = task.await;
            complete_install(view, cx, root, outcome);
        })
        .detach();
        cx.notify();
    }

    fn finish_modpack(
        &mut self,
        root: PathBuf,
        result: Result<PathBuf, InstallFailure>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let State::Installing(preview, _) = std::mem::replace(&mut self.state, State::Idle) else {
            return;
        };
        match result {
            Ok(directory) => {
                cx.emit(ImportEvent::Installed {
                    root,
                    directory,
                    name: preview.pack.name().to_owned(),
                });
            }
            Err(InstallFailure::Failed(error)) => {
                self.error = Some(error);
                self.state = State::Preview(preview);
            }
            Err(InstallFailure::Manual(files)) => {
                ManualDownloads::replace_pending(&preview.manual, files, cx);
                self.state = State::Preview(preview);
            }
        }
        self.focus_modpack(window, cx);
    }

    pub(crate) fn set_enabled(&mut self, enabled: bool, cx: &mut Context<'_, Self>) {
        self.enabled = enabled;
        cx.notify();
    }

    fn update_modpack_progress(&mut self, progress: Progress, cx: &mut Context<'_, Self>) {
        if let State::Installing(_, previous) = &mut self.state {
            *previous = progress;
            cx.notify();
        }
    }

    fn set_automatic(&mut self, enabled: &bool, _: &mut Window, cx: &mut Context<'_, Self>) {
        if let State::Preview(preview) = &mut self.state {
            preview.policy = preview.policy.with_auto_download_restricted(*enabled);
            cx.notify();
        }
    }
}

fn option_checkbox(
    file: &OptionalFile,
    checked: bool,
    busy: bool,
    cx: &mut Context<'_, ModpackImport>,
) -> Checkbox {
    let id = file.id().clone();
    let label = SharedString::from(file.label().to_owned());
    Checkbox::new(ElementId::Name(SharedString::from(id.key().to_owned())))
        .label(label.clone())
        .tooltip(label)
        .checked(checked)
        .disabled(busy)
        .on_click(cx.listener(move |view, checked: &bool, _, cx| {
            view.select_modpack_option(&id, *checked, cx);
        }))
}

async fn inspect_path(
    path: Result<Option<PathBuf>, String>,
    executor: BackgroundExecutor,
    plugin: ModpackPlugin,
) -> Result<Option<Pack>, String> {
    let Some(path) = path? else { return Ok(None) };
    executor
        .spawn(async move {
            plugin
                .inspect(&path)
                .map(Some)
                .map_err(|error| error.to_string())
        })
        .await
}

fn complete_preview(
    view: WeakEntity<ModpackImport>,
    cx: &mut AsyncWindowContext,
    result: Result<Option<Pack>, String>,
) {
    let _ = view.update_in(cx, |view, window, cx| {
        view.finish_modpack_preview(result, window, cx)
    });
}

async fn show_progress(
    view: &WeakEntity<ModpackImport>,
    cx: &mut AsyncWindowContext,
    mut updates: watch::Receiver<Progress>,
) {
    while updates.changed().await.is_ok() {
        let progress = *updates.borrow_and_update();
        let _ = view.update_in(cx, |view, _, cx| view.update_modpack_progress(progress, cx));
    }
}

fn complete_install(
    view: WeakEntity<ModpackImport>,
    cx: &mut AsyncWindowContext,
    root: PathBuf,
    result: Result<PathBuf, InstallFailure>,
) {
    let _ = view.update_in(cx, |view, window, cx| {
        view.finish_modpack(root, result, window, cx)
    });
}
