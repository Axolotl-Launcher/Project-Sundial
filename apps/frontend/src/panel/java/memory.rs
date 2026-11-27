// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The launcher-wide heap preference and its editor.

use std::sync::Arc;

use axlauncher_core::desktop::instance_settings::{self, EditCancellation, GlobalHeap};
use axlauncher_core::domain::memory::{MaximumHeap, MemoryAllocation};
use axlauncher_core::state::SettingsOrigin;
use axlauncher_core::storage::DataDir;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, App, Button, Context, Disableable, GroupBox, IntoElement, Label, Task,
    WeakEntity, Window, h_flex,
};

use super::super::RootView;
use super::super::memory_edits::MemoryScope;

mod editor;
pub(in crate::panel) use editor::open as open_editor;

impl RootView {
    pub(in crate::panel) fn render_memory(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let mode = self.settings.as_ref().map(|settings| settings.memory());
        let button = Button::new("configure-global-memory")
            .label("Configure memory…")
            .disabled(self.settings_readonly())
            .on_click(cx.listener(|view, _, window, cx| view.open_global_memory(window, cx)));
        GroupBox::new()
            .id("global-memory")
            .title(
                Label::new("Minecraft memory")
                    .text_sm()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                h_flex()
                    .gap_3()
                    .items_center()
                    .child(Label::new(global_label(mode)))
                    .child(button),
            )
            .into_any_element()
    }

    fn open_global_memory(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.settings_readonly() {
            return;
        }
        let Some((paths, settings)) = self.paths.as_ref().zip(self.settings.as_ref()) else {
            return;
        };
        let paths = paths.clone();
        let scope = self.memory_edits.scope();
        let view = cx.weak_entity();
        let expected = settings.memory();
        let saved = settings.memory().manual_heap();
        open_editor(
            "Minecraft memory".into(),
            "Automatic",
            saved,
            MaximumHeap::default(),
            window,
            cx,
            move |heap, cancel, cx| {
                Self::start_global_memory_edit(
                    view.clone(),
                    paths.clone(),
                    scope.clone(),
                    expected,
                    heap,
                    cancel,
                    cx,
                )
            },
        );
    }

    fn commit_global_memory_edit(
        &mut self,
        paths: DataDir,
        scope: MemoryScope,
        expected: MemoryAllocation,
        heap: Option<MaximumHeap>,
        cancel: Arc<EditCancellation>,
        cx: &mut Context<'_, Self>,
    ) -> Result<Task<Result<(), String>>, String> {
        if self.settings_readonly() || !scope.matches(&self.memory_edits.scope()) {
            return Err("The settings location changed. Reopen the editor to continue.".into());
        }
        let choice = heap.map_or(GlobalHeap::Automatic, |heap| {
            GlobalHeap::ManualMib(heap.mib())
        });
        Ok(self.save_memory(
            scope,
            cancel,
            cx,
            move |cancel| {
                instance_settings::set_global_heap(&paths, expected, choice, cancel)
                    .map_err(|error| error.to_string())
            },
            move |view, memory, cx| view.apply_global_memory(expected, memory, cx),
        ))
    }

    fn apply_global_memory(
        &mut self,
        expected: MemoryAllocation,
        memory: MemoryAllocation,
        cx: &mut Context<'_, Self>,
    ) -> Result<(), String> {
        if let Some(settings) = self
            .settings
            .as_mut()
            .filter(|data| data.memory() == expected)
        {
            settings.set_memory(memory);
            self.origin = Some(SettingsOrigin::Stored);
            self.invalidate_inspection();
            cx.notify();
        }
        Ok(())
    }

    fn start_global_memory_edit(
        view: WeakEntity<Self>,
        paths: DataDir,
        scope: MemoryScope,
        expected: MemoryAllocation,
        heap: Option<MaximumHeap>,
        cancel: Arc<EditCancellation>,
        cx: &mut App,
    ) -> Result<Task<Result<(), String>>, String> {
        view.update(cx, |view, cx| {
            view.commit_global_memory_edit(paths, scope, expected, heap, cancel, cx)
        })
        .map_err(|error| error.to_string())?
    }
}

fn global_label(mode: Option<MemoryAllocation>) -> String {
    match mode {
        Some(MemoryAllocation::Automatic) => "Automatic allocation".into(),
        Some(MemoryAllocation::Manual(heap)) => {
            format!("{} MiB maximum", heap.mib())
        }
        None => "Settings unavailable".into(),
    }
}
