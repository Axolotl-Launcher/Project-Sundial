// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! A shared dialog for editing a single heap preference.

use std::sync::Arc;

use axlauncher_core::desktop::instance_settings::EditCancellation;
use axlauncher_core::domain::memory::MaximumHeap;
use axlauncher_core::storage::PhysicalMemory;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, App, AsyncWindowContext, Button, ButtonVariants, ClickEvent, Context, Disableable,
    Entity, InputEvent, InputState, IntoElement, Label, NumberInput, RadioGroup, Render, Slider,
    SliderEvent, SliderState, Task, WeakEntity, Window, WindowExt, h_flex, v_flex,
};

type Save = Task<Result<(), String>>;
type Commit = dyn Fn(Option<MaximumHeap>, Arc<EditCancellation>, &mut App) -> Result<Save, String>;

pub(in crate::panel) fn open(
    title: String,
    inherited_label: &'static str,
    saved: Option<MaximumHeap>,
    suggested: MaximumHeap,
    window: &mut Window,
    cx: &mut App,
    commit: impl Fn(Option<MaximumHeap>, Arc<EditCancellation>, &mut App) -> Result<Save, String>
    + 'static,
) {
    let editor = cx.new(|cx| {
        MemoryEditor::new(
            inherited_label,
            saved,
            suggested,
            Box::new(commit),
            window,
            cx,
        )
    });
    window.open_dialog(cx, move |dialog, _, _| {
        let confirm = editor.clone();
        let dismiss = editor.clone();
        dialog
            .title(title.clone())
            .overlay_closable(false)
            .on_ok(move |_, window, cx| {
                confirm.update(cx, |editor, cx| editor.submit(window, cx));
                false
            })
            .on_close(move |_, _, cx| dismiss.update(cx, |editor, _| editor.dismiss()))
            .child(editor.clone())
    });
}

struct MemoryEditor {
    commit: Box<Commit>,
    inherited_label: &'static str,
    manual: bool,
    limit_mib: u32,
    input: Entity<InputState>,
    slider: Entity<SliderState>,
    error: Option<String>,
    pending: Option<Arc<EditCancellation>>,
    dismissed: bool,
}

impl MemoryEditor {
    fn new(
        inherited_label: &'static str,
        saved: Option<MaximumHeap>,
        suggested: MaximumHeap,
        commit: Box<Commit>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        const BYTES_PER_MIB: u64 = 1024 * 1024;
        let mib = saved.unwrap_or(suggested).mib();
        let total = PhysicalMemory::read().total_bytes() / BYTES_PER_MIB;
        let limit_mib = u32::try_from(total)
            .unwrap_or(u32::MAX)
            .max(mib)
            .max(MaximumHeap::MINIMUM_MIB + 1);
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(mib.to_string())
                .validate(|value, _| value.bytes().all(|byte| byte.is_ascii_digit()))
        });
        let slider = cx.new(|_| {
            SliderState::new()
                .min(MaximumHeap::MINIMUM_MIB as f32)
                .max(limit_mib as f32)
                .default_value(mib as f32)
        });
        let slider_for_input = slider.clone();
        cx.subscribe_in(&input, window, move |_, input, event, window, cx| {
            sync_slider(input, &slider_for_input, event, window, cx);
        })
        .detach();
        let input_for_slider = input.clone();
        cx.subscribe_in(&slider, window, move |_, _, event, window, cx| {
            sync_input(&input_for_slider, event, limit_mib, window, cx);
        })
        .detach();
        Self {
            commit,
            inherited_label,
            manual: saved.is_some(),
            limit_mib,
            input,
            slider,
            error: None,
            pending: None,
            dismissed: false,
        }
    }

    fn heap(&self, cx: &App) -> Option<MaximumHeap> {
        let mib: u32 = self.input.read(cx).value().parse().ok()?;
        (mib <= self.limit_mib).then_some(())?;
        MaximumHeap::new(mib)
    }

    fn save_clicked(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<'_, Self>) {
        self.submit(window, cx);
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.pending.is_some() || self.dismissed {
            return;
        }
        let heap = self.manual.then(|| self.heap(cx)).flatten();
        if self.manual && heap.is_none() {
            return;
        }
        self.error = None;
        let cancel = Arc::new(EditCancellation::default());
        let task = match (self.commit)(heap, cancel.clone(), cx) {
            Ok(task) => task,
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return;
            }
        };
        self.pending = Some(cancel);
        cx.spawn_in(window, async move |editor, cx| {
            Self::complete_save(editor, cx, task.await);
        })
        .detach();
        cx.notify();
    }

    fn complete_save(
        editor: WeakEntity<Self>,
        cx: &mut AsyncWindowContext,
        result: Result<(), String>,
    ) {
        let _ = editor.update_in(cx, |editor, window, cx| {
            editor.finished(result, window, cx);
        });
    }

    fn finished(
        &mut self,
        result: Result<(), String>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.pending = None;
        if self.dismissed {
            return;
        }
        match result {
            Ok(()) => window.close_dialog(cx),
            Err(error) => {
                self.error = Some(error);
                cx.notify();
            }
        }
    }

    fn dismiss(&mut self) {
        self.dismissed = true;
        if let Some(cancel) = &self.pending {
            cancel.cancel();
        }
    }

    fn set_mode(&mut self, index: &usize, _: &mut Window, cx: &mut Context<'_, Self>) {
        self.manual = *index == 1;
        self.error = None;
        cx.notify();
    }

    fn render_maximum(&self) -> impl IntoElement + use<> {
        h_flex()
            .id("maximum-heap")
            .gap_3()
            .items_center()
            .child(Label::new("Maximum heap").text_sm().w_24())
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .child(Slider::new(&self.slider).accessibility_label("Maximum heap slider")),
            )
            .child(
                NumberInput::new(&self.input)
                    .stepper(false)
                    .w_24()
                    .flex_none()
                    .accessibility_label("Maximum heap in MiB")
                    .suffix(Label::new("MiB").text_xs()),
            )
    }
}

impl Drop for MemoryEditor {
    fn drop(&mut self) {
        self.dismiss();
    }
}

impl Render for MemoryEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let invalid = self.manual && self.heap(cx).is_none();
        v_flex()
            .gap_3()
            .child(
                RadioGroup::horizontal("memory-allocation-mode")
                    .children([self.inherited_label, "Manual"])
                    .selected_index(Some(usize::from(self.manual)))
                    .on_change(cx.listener(Self::set_mode)),
            )
            .when(self.manual, |form| form.child(self.render_maximum()))
            .child(
                Label::new("Heap options in the Minecraft version take priority.")
                    .text_sm()
                    .text_color(cx.theme().muted_foreground),
            )
            .children(invalid.then(|| {
                Label::new(format!(
                    "Enter a maximum heap of {}–{} MiB.",
                    MaximumHeap::MINIMUM_MIB,
                    self.limit_mib
                ))
                .text_sm()
                .text_color(cx.theme().danger)
            }))
            .children(self.error.as_ref().map(|error| {
                Label::new(error.clone())
                    .text_sm()
                    .text_color(cx.theme().danger)
            }))
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("cancel-memory")
                            .label("Cancel")
                            .on_click(cx.listener(|editor, _, window, cx| {
                                editor.dismiss();
                                window.close_dialog(cx);
                            })),
                    )
                    .child(
                        Button::new("save-memory")
                            .primary()
                            .label("Save")
                            .disabled(invalid || self.pending.is_some())
                            .on_click(cx.listener(Self::save_clicked)),
                    ),
            )
    }
}

fn sync_slider(
    input: &Entity<InputState>,
    slider: &Entity<SliderState>,
    event: &InputEvent,
    window: &mut Window,
    cx: &mut Context<'_, MemoryEditor>,
) {
    if !matches!(event, InputEvent::Change) {
        return;
    }
    if let Ok(value) = input.read(cx).value().parse::<u32>() {
        slider.update(cx, |slider, cx| {
            let value = (value as f32).clamp(slider.min_value(), slider.max_value());
            slider.set_value(value, window, cx);
        });
    }
    cx.notify();
}

fn sync_input(
    input: &Entity<InputState>,
    event: &SliderEvent,
    limit_mib: u32,
    window: &mut Window,
    cx: &mut Context<'_, MemoryEditor>,
) {
    let SliderEvent::Change(value) = event else {
        return;
    };
    let mib = (value.end() as u32).clamp(MaximumHeap::MINIMUM_MIB, limit_mib);
    input.update(cx, |input, cx| input.set_value(mib.to_string(), window, cx));
    cx.notify();
}
