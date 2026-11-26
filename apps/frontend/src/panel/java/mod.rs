// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Java catalog state and background operations scoped to the data directory.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axlauncher_core::desktop::java::JavaCatalog;
use axlauncher_core::launch::JavaRuntime;
use axlauncher_core::storage::DataDir;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, App, Button, ButtonVariants, Context, Disableable, ElementId,
    EventEmitter, GroupBox, Hsla, IntoElement, Label, Render, Sizable, Tooltip, Window, div,
    h_flex, v_flex,
};

use crate::file_picker::choose_file;

mod instance;
mod operation;
pub(super) use instance::InstancePicker;
use operation::{Activity, Operation, OperationError};

#[derive(Default)]
pub(super) struct JavaPanel {
    paths: Option<DataDir>,
    catalog: Option<Arc<JavaCatalog>>,
    error: Option<(Option<String>, String)>,
    in_flight: BTreeMap<PathBuf, (u64, Activity)>,
    scope: u64,
    refresh_queued: bool,
}

pub(super) enum JavaEvent {
    RefreshStarted,
    CatalogChanged,
    SelectionChanged(Option<String>),
}

impl EventEmitter<JavaEvent> for JavaPanel {}

impl JavaPanel {
    pub(super) fn new(paths: Option<DataDir>, cx: &mut Context<'_, Self>) -> Self {
        let mut panel = Self {
            paths,
            ..Self::default()
        };
        panel.run(Operation::Startup, cx);
        panel
    }

    pub(super) fn reload(&mut self, paths: Option<DataDir>, cx: &mut Context<'_, Self>) {
        self.change_paths(paths);
        self.refresh(cx);
        cx.notify();
    }

    fn change_paths(&mut self, paths: Option<DataDir>) {
        if self.root() != paths.as_ref().map(DataDir::root) {
            self.scope += 1;
            self.catalog = None;
            self.error = None;
            self.refresh_queued = false;
        }
        self.paths = paths;
    }

    fn root(&self) -> Option<&Path> {
        self.paths.as_ref().map(DataDir::root)
    }

    fn current(&self) -> Option<&(u64, Activity)> {
        self.in_flight.get(self.root()?)
    }

    fn busy(&self) -> bool {
        self.current().is_some()
    }

    fn active(&self) -> Option<&Activity> {
        let (scope, activity) = self.current()?;
        (*scope == self.scope).then_some(activity)
    }

    pub(super) fn write_pending(&self, profile: &str) -> bool {
        match self.current().map(|(_, activity)| activity) {
            Some(Activity::Starting | Activity::Discovering | Activity::SavingCatalog) => true,
            Some(Activity::SavingProfile(current)) => profile == current,
            _ => false,
        }
    }

    fn snapshot(&self) -> Option<Arc<JavaCatalog>> {
        self.catalog.clone()
    }

    fn feedback(&self, profile: Option<&str>) -> Option<(&str, bool)> {
        if let Some(activity) = self.active() {
            return (profile.is_none() || activity.profile() == profile)
                .then(|| (activity.label(), false));
        }
        if self.busy() && profile.is_none() {
            return Some(("Waiting to refresh the Java list…", false));
        }
        self.error
            .as_ref()
            .filter(|(owner, _)| owner.as_deref() == profile)
            .map(|(_, error)| (error.as_str(), true))
    }

    fn begin(&mut self, activity: Activity) -> Option<DataDir> {
        let paths = self.paths.clone().filter(|_| !self.busy())?;
        self.error = None;
        self.in_flight
            .insert(paths.root().to_owned(), (self.scope, activity));
        Some(paths)
    }

    fn take_completed(&mut self, root: &Path) -> Option<Activity> {
        let (scope, activity) = self.in_flight.remove(root)?;
        (self.root() == Some(root) && scope == self.scope).then_some(activity)
    }

    pub(super) fn refresh(&mut self, cx: &mut Context<'_, Self>) {
        if self.busy() {
            self.refresh_queued = true;
            return;
        }
        self.run(Operation::Refresh, cx);
    }

    fn run(&mut self, operation: Operation, cx: &mut Context<'_, Self>) {
        let refreshing = matches!(operation, Operation::Refresh);
        let Some(paths) = self.begin(operation.activity()) else {
            return;
        };
        if refreshing {
            cx.emit(JavaEvent::RefreshStarted);
        }
        let root = paths.root().to_owned();
        let task = cx.background_spawn(async move { operation.run(&paths) });
        cx.spawn(async move |view, cx| {
            let outcome = task.await;
            let _ = view.update(cx, |view, cx| view.finish(&root, outcome, cx));
        })
        .detach();
        cx.notify();
    }

    fn finish(
        &mut self,
        root: &Path,
        outcome: Result<JavaCatalog, OperationError>,
        cx: &mut Context<'_, Self>,
    ) {
        if let Some(activity) = self.take_completed(root) {
            self.apply_outcome(activity, outcome, cx);
        }
        self.finish_feedback(root, cx);
    }

    fn apply_outcome(
        &mut self,
        activity: Activity,
        outcome: Result<JavaCatalog, OperationError>,
        cx: &mut Context<'_, Self>,
    ) {
        let profile = activity.profile().map(str::to_owned);
        if activity.writes() && !matches!(outcome, Err(OperationError::LoadOrSave(_))) {
            cx.emit(JavaEvent::SelectionChanged(profile.clone()));
        }
        if matches!(activity, Activity::Loading)
            && let (Some(previous), Ok(next)) = (self.catalog.as_deref(), &outcome)
            && previous != next
        {
            cx.emit(JavaEvent::CatalogChanged);
        }
        match outcome {
            Ok(catalog) => self.catalog = Some(Arc::new(catalog)),
            Err(OperationError::LoadOrSave(error) | OperationError::RefreshAfterSave(error)) => {
                self.error = Some((profile, error))
            }
        }
    }

    fn add_java(&mut self, cx: &mut Context<'_, Self>) {
        let Some(paths) = self.begin(Activity::ChoosingFile) else {
            return;
        };
        let root = paths.root().to_owned();
        let selection = choose_file("Choose a Java executable", cx);
        cx.spawn(async move |view, cx| {
            let outcome = selection.await;
            let _ = view.update(cx, |view, cx| view.finish_picker(&root, outcome, cx));
        })
        .detach();
        cx.notify();
    }

    fn finish_picker(
        &mut self,
        root: &Path,
        outcome: Result<Option<PathBuf>, String>,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(Activity::ChoosingFile) = self.take_completed(root) else {
            self.finish_feedback(root, cx);
            return;
        };
        match outcome {
            Ok(Some(path)) => {
                self.run(Operation::Add(path), cx);
                return;
            }
            Ok(None) => (),
            Err(error) => self.error = Some((None, error)),
        }
        self.finish_feedback(root, cx);
    }

    fn finish_feedback(&mut self, root: &Path, cx: &mut Context<'_, Self>) {
        if self.root() != Some(root) {
            return;
        }
        if std::mem::take(&mut self.refresh_queued) {
            let error = self.error.take();
            self.refresh(cx);
            self.error = error;
        }
        cx.notify();
    }
}

impl Render for JavaPanel {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let available = self.root().is_some();
        let disabled = !available || self.busy();
        let snapshot = self.snapshot();
        let rows = snapshot.as_deref().map(catalog_rows);
        let status = match &rows {
            _ if !available => Some("Choose a data directory to manage Java."),
            Some(rows) if rows.is_empty() => Some("No Java installations found."),
            _ => None,
        };
        GroupBox::new()
            .id("java")
            .title(
                Label::new("Java")
                    .text_sm()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(java_toolbar(disabled, cx))
                    .children(java_feedback(self, None, cx))
                    .when_some(status, |column, status| {
                        column.child(
                            Label::new(status)
                                .text_sm()
                                .text_color(cx.theme().muted_foreground),
                        )
                    })
                    .when_some(rows, |column, rows| {
                        column.children(
                            rows.into_iter()
                                .map(|(path, row)| java_row(path, row, disabled, cx)),
                        )
                    }),
            )
    }
}

fn java_feedback(panel: &JavaPanel, profile: Option<&str>, cx: &App) -> Option<Label> {
    let (message, error) = panel.feedback(profile)?;
    let theme = cx.theme();
    let color = if error {
        theme.danger
    } else {
        theme.muted_foreground
    };
    let id = ElementId::Name(format!("java-feedback:{}", profile.unwrap_or_default()).into());
    Some(
        Label::new(message.to_owned())
            .id(id)
            .text_xs()
            .text_color(color),
    )
}

fn java_toolbar(disabled: bool, cx: &mut Context<'_, JavaPanel>) -> impl IntoElement + use<> {
    h_flex()
        .gap_2()
        .child(
            Button::new("java-add")
                .label("Add Java…")
                .disabled(disabled)
                .on_click(cx.listener(|view, _, _, cx| view.add_java(cx))),
        )
        .child(
            Button::new("java-discover")
                .label("Auto-detect Java")
                .ghost()
                .disabled(disabled)
                .on_click(cx.listener(|view, _, _, cx| view.run(Operation::Discover, cx))),
        )
}

#[derive(Default)]
struct RuntimeRow<'a> {
    runtime: Option<&'a JavaRuntime>,
    preferred: Vec<u32>,
}

fn catalog_rows(catalog: &JavaCatalog) -> BTreeMap<&Path, RuntimeRow<'_>> {
    let mut rows = BTreeMap::<&Path, RuntimeRow<'_>>::new();
    for path in catalog.paths() {
        rows.entry(path).or_default();
    }
    for runtime in catalog.available() {
        rows.entry(runtime.path()).or_default().runtime = Some(runtime);
    }
    for (major, path) in catalog.preferred() {
        if let Some(row) = rows.get_mut(path) {
            row.preferred.push(major);
        }
    }
    rows
}

fn java_row(
    path: &Path,
    row: RuntimeRow<'_>,
    disabled: bool,
    cx: &mut Context<'_, JavaPanel>,
) -> AnyElement {
    let title = row
        .runtime
        .map_or_else(|| "Unavailable".into(), runtime_title);
    let muted = cx.theme().muted_foreground;
    let statuses = row.preferred.iter().map(|&major| {
        let status = if row.runtime.is_some_and(|runtime| runtime.major() == major) {
            format!("Preferred for Java {major}")
        } else {
            format!("Preferred Java {major} unavailable")
        };
        (major, status)
    });
    h_flex()
        .items_start()
        .gap_3()
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(Label::new(title).id(java_id("java-title", path)).text_sm())
                .child(java_path_label(
                    java_id("java-path", path),
                    path.display().to_string(),
                    muted,
                ))
                .children(statuses.map(|(major, status)| {
                    Label::new(status)
                        .id(java_id(&format!("java-preference-{major}"), path))
                        .text_xs()
                        .text_color(muted)
                })),
        )
        .child(runtime_actions(path, &row, disabled, cx))
        .into_any_element()
}

fn runtime_actions(
    path: &Path,
    row: &RuntimeRow<'_>,
    disabled: bool,
    cx: &mut Context<'_, JavaPanel>,
) -> AnyElement {
    let major = row.runtime.map(JavaRuntime::major);
    let prefer = major
        .filter(|major| !row.preferred.contains(major))
        .map(|major| {
            let selected = path.to_owned();
            Button::new(java_id("java-prefer", path))
                .label("Set as preferred")
                .tooltip(format!("Prefer this installation for Java {major}"))
                .ghost()
                .small()
                .disabled(disabled)
                .on_click(cx.listener(move |view, _, _, cx| {
                    view.run(
                        Operation::Prefer {
                            major,
                            path: selected.clone(),
                        },
                        cx,
                    )
                }))
        });
    let selected = path.to_owned();
    let remove = Button::new(java_id("java-remove", path))
        .label("Remove")
        .tooltip("Remove this installation from the Java list")
        .ghost()
        .small()
        .disabled(disabled)
        .on_click(
            cx.listener(move |view, _, _, cx| view.run(Operation::Remove(selected.clone()), cx)),
        );
    h_flex()
        .flex_wrap()
        .gap_1()
        .children(prefer)
        .child(remove)
        .into_any_element()
}

fn java_id(kind: &str, path: &Path) -> ElementId {
    ElementId::Name(format!("{kind}:{}", path.display()).into())
}

fn java_path_label(id: ElementId, text: String, color: Hsla) -> impl IntoElement {
    let full = text.clone();
    div()
        .id(id.clone())
        .flex_1()
        .min_w_0()
        .tooltip(move |window, cx| Tooltip::new(full.clone()).build(window, cx))
        .child(
            Label::new(text)
                .id(id)
                .text_xs()
                .text_color(color)
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis_middle(),
        )
}

fn runtime_title(runtime: &JavaRuntime) -> String {
    match runtime.architecture() {
        Some(arch) => format!("Java {} · {arch}", runtime.full_version()),
        None => format!("Java {}", runtime.full_version()),
    }
}

#[cfg(test)]
#[path = "../../../../../tests/apps/frontend/src/panel/java/mod.rs"]
mod tests;
