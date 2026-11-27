// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The launcher-wide heap preference and its editor.

use std::path::Path;

use axlauncher_core::desktop::memory;
use axlauncher_core::domain::memory::{MaximumHeap, MemoryAllocation};
use axlauncher_core::state::SettingsOrigin;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, Button, Context, Disableable, GroupBox, IntoElement, Label, Window,
    h_flex,
};

use super::super::RootView;

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
        let root = paths.root().to_path_buf();
        let view = cx.weak_entity();
        let saved = settings.memory().manual_heap();
        open_editor(
            "Minecraft memory".into(),
            "Automatic",
            saved,
            MaximumHeap::default(),
            window,
            cx,
            move |heap, cx| {
                view.update(cx, |view, cx| view.save_global_memory(&root, heap, cx))
                    .map_err(|error| error.to_string())?
            },
        );
    }

    fn save_global_memory(
        &mut self,
        root: &Path,
        heap: Option<MaximumHeap>,
        cx: &mut Context<'_, Self>,
    ) -> Result<(), String> {
        if self.settings_readonly() || self.paths.as_ref().is_none_or(|paths| paths.root() != root)
        {
            return Err("The settings location changed. Reopen the editor to continue.".into());
        }
        let allocation = heap.map_or(MemoryAllocation::Automatic, MemoryAllocation::Manual);
        let data = memory::set_global(root, allocation).map_err(|error| error.to_string())?;
        self.settings = Some(data);
        self.origin = Some(SettingsOrigin::Stored);
        self.invalidate_inspection();
        cx.notify();
        Ok(())
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
