// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::BTreeMap;
use std::path::PathBuf;

use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, App, AsyncWindowContext, Button, ButtonVariants, Context, Disableable,
    Entity, IntoElement, Label, Link, Render, SharedString, WeakEntity, Window, h_flex, v_flex,
};

use crate::file_picker::choose_file;
use crate::modpacks::service::ManualFile;

#[derive(Default)]
pub(super) struct ManualDownloads {
    pending: Vec<ManualFile>,
    chosen: BTreeMap<String, PathBuf>,
    picking: bool,
    error: Option<String>,
}

impl ManualDownloads {
    pub(super) fn replace_pending(view: &Entity<Self>, mut files: Vec<ManualFile>, app: &mut App) {
        files.sort_unstable_by(|left, right| left.path().cmp(right.path()));
        view.update(app, |manual, cx| {
            manual
                .chosen
                .retain(|path, _| files.iter().any(|file| file.path() == path));
            manual.pending = files;
            cx.notify();
        });
    }

    pub(super) fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }
    pub(super) fn picking(&self) -> bool {
        self.picking
    }
    pub(super) fn selected_paths(&self) -> BTreeMap<String, PathBuf> {
        self.chosen.clone()
    }

    fn row(&self, file: &ManualFile, cx: &mut Context<'_, Self>) -> AnyElement {
        let path = file.path().to_owned();
        let muted = cx.theme().muted_foreground;
        let chosen = self
            .chosen
            .get(&path)
            .and_then(|path| path.file_name())
            .map(|value| value.to_string_lossy().into_owned());
        v_flex()
            .gap_1()
            .child(Label::new(path.clone()).text_sm())
            .when_some(file.reason().map(str::to_owned), |column, reason| {
                column.child(Label::new(reason).text_xs().text_color(muted))
            })
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        Link::new(SharedString::from(format!("download-{path}")))
                            .href(file.url().to_owned())
                            .child("Open download page"),
                    )
                    .child(
                        Button::new(SharedString::from(format!("select-{path}")))
                            .label(if chosen.is_some() {
                                "Replace file…"
                            } else {
                                "Choose file…"
                            })
                            .ghost()
                            .disabled(self.picking)
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.choose_local_file(path.clone(), window, cx)
                            })),
                    )
                    .when_some(chosen, |row, name| {
                        row.child(Label::new(name).text_xs().text_color(muted))
                    }),
            )
            .into_any_element()
    }
}

impl ManualDownloads {
    fn choose_local_file(
        &mut self,
        target: String,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if self.picking {
            return;
        }
        self.picking = true;
        self.error = None;
        let selection = choose_file("Choose the downloaded file", cx);
        cx.spawn_in(window, async move |view, cx| {
            complete_selection(view, cx, target, selection.await);
        })
        .detach();
        cx.notify();
    }

    fn finish_local_file(
        &mut self,
        target: String,
        result: Result<Option<PathBuf>, String>,
        cx: &mut Context<'_, Self>,
    ) {
        self.picking = false;
        match result {
            Ok(Some(path)) => {
                self.chosen.insert(target, path);
            }
            Ok(None) => {}
            Err(error) => self.error = Some(error),
        }
        cx.notify();
    }
}

impl Render for ManualDownloads {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let danger = cx.theme().danger;
        v_flex()
            .gap_2()
            .child(Label::new("Files to download manually").text_sm())
            .children(self.pending.iter().map(|file| self.row(file, cx)))
            .when_some(self.error.clone(), |column, error| {
                column.child(Label::new(error).text_sm().text_color(danger))
            })
    }
}

fn complete_selection(
    view: WeakEntity<ManualDownloads>,
    cx: &mut AsyncWindowContext,
    target: String,
    result: Result<Option<PathBuf>, String>,
) {
    let _ = view.update_in(cx, |view, _, cx| view.finish_local_file(target, result, cx));
}
