// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::path::PathBuf;

use axlauncher_core::state::profile::{ProfileDiscovery, create_blank, discover};
use axlauncher_core::storage::DataDir;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, AsyncWindowContext, Button, ButtonVariants, Context, Disableable,
    Entity, FocusHandle, Focusable, Input, InputEvent, InputState, IntoElement, KeyDownEvent,
    Label, WeakEntity, Window, h_flex, v_flex,
};

use super::RootView;

pub(super) struct CreateInstance {
    name: Entity<InputState>,
    version: Entity<InputState>,
    trigger: FocusHandle,
    open: bool,
    saving: bool,
    error: Option<String>,
}

impl CreateInstance {
    pub(super) fn new(window: &mut Window, cx: &mut Context<'_, RootView>) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx));
        let version = cx.new(|cx| InputState::new(window, cx).placeholder("1.21.11"));
        for input in [&name, &version] {
            cx.subscribe_in(input, window, RootView::instance_form_event)
                .detach();
        }
        Self {
            name,
            version,
            trigger: cx.focus_handle(),
            open: false,
            saving: false,
            error: None,
        }
    }
}

impl RootView {
    fn instance_form_event(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        match event {
            InputEvent::PressEnter {
                secondary: false,
                shift: false,
            } => self.create_instance(window, cx),
            InputEvent::Change => {
                self.create_instance.error = None;
                cx.notify();
            }
            _ => (),
        }
    }

    pub(super) fn render_create_instance(
        &self,
        data: Option<&DataDir>,
        cx: &mut Context<'_, Self>,
    ) -> AnyElement {
        let state = &self.create_instance;
        let muted = cx.theme().muted_foreground;
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_3()
                    .justify_between()
                    .when_some(data, |row, data| {
                        row.child(
                            Label::new(data.instances().display().to_string())
                                .text_xs()
                                .text_color(muted),
                        )
                    })
                    .child(
                        Button::new("open-create-instance")
                            .label("Create blank instance…")
                            .track_focus(&state.trigger)
                            .toggled(state.open)
                            .disabled(data.is_none() || state.open)
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.create_instance.open = true;
                                view.create_instance
                                    .name
                                    .read(cx)
                                    .focus_handle(cx)
                                    .focus(window, cx);
                                cx.notify();
                            })),
                    ),
            )
            .when(state.open, |column| {
                column.child(self.render_instance_form(cx))
            })
            .into_any_element()
    }

    fn render_instance_form(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let state = &self.create_instance;
        let danger = cx.theme().danger;
        v_flex()
            .on_key_down(cx.listener(Self::instance_form_key))
            .gap_2()
            .child(
                h_flex()
                    .flex_wrap()
                    .items_end()
                    .gap_3()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_40()
                            .gap_1()
                            .child(Label::new("Instance name").text_sm())
                            .child(
                                Input::new(&state.name)
                                    .id("new-instance-name")
                                    .aria_label("Instance name")
                                    .disabled(state.saving),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .w_48()
                            .child(Label::new("Minecraft version (optional)").text_sm())
                            .child(
                                Input::new(&state.version)
                                    .id("new-instance-version")
                                    .aria_label("Minecraft version (optional)")
                                    .disabled(state.saving),
                            ),
                    )
                    .child(
                        Button::new("cancel-create-instance")
                            .label("Cancel")
                            .ghost()
                            .disabled(state.saving)
                            .on_click(cx.listener(|view, _, window, cx| {
                                view.close_instance_form(window, cx)
                            })),
                    )
                    .child(
                        Button::new("create-instance")
                            .label("Create")
                            .primary()
                            .loading(state.saving)
                            .disabled(self.root.is_none())
                            .on_click(
                                cx.listener(|view, _, window, cx| view.create_instance(window, cx)),
                            ),
                    ),
            )
            .when_some(state.error.clone(), |column, error| {
                column.child(Label::new(error).text_sm().text_color(danger))
            })
            .into_any_element()
    }

    fn instance_form_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        if event.keystroke.key == "escape" && !self.create_instance.saving {
            cx.stop_propagation();
            self.close_instance_form(window, cx);
        }
    }

    fn create_instance(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let state = &mut self.create_instance;
        if !state.open || state.saving {
            return;
        }
        let Some(root) = self.root.clone() else {
            return;
        };
        let name = state.name.read(cx).value().to_string();
        let version = state.version.read(cx).value().to_string();
        state.saving = true;
        state.error = None;
        let task = cx.background_spawn(async move {
            let instances = DataDir::new(&root).instances();
            let result = create_blank(&instances, &name, Some(&version))
                .map(|path| (path, discover(&instances)))
                .map_err(|error| error.to_string());
            (root, result)
        });
        cx.spawn_in(window, async move |view, cx| {
            let (root, result) = task.await;
            complete_creation(view, cx, root, result);
        })
        .detach();
        cx.notify();
    }

    fn finish_create_instance(
        &mut self,
        root: PathBuf,
        result: Result<(PathBuf, ProfileDiscovery), String>,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.create_instance.saving = false;
        let (path, profiles) = match result {
            Ok(created) => created,
            Err(error) => {
                self.activity.error(error.clone());
                self.create_instance.error = Some(error);
                cx.notify();
                return;
            }
        };
        self.activity
            .success(format!("Created instance in {}", path.display()));
        if self.root.as_ref() == Some(&root) {
            self.instances.replace(profiles, true);
            self.select_instance(path, cx);
        }
        self.close_instance_form(window, cx);
    }

    fn close_instance_form(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let state = &mut self.create_instance;
        state.open = false;
        state.error = None;
        for input in [&state.name, &state.version] {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        state.trigger.focus(window, cx);
        cx.notify();
    }
}

fn complete_creation(
    view: WeakEntity<RootView>,
    cx: &mut AsyncWindowContext,
    root: PathBuf,
    result: Result<(PathBuf, ProfileDiscovery), String>,
) {
    let _ = view.update_in(cx, |view, window, cx| {
        view.finish_create_instance(root, result, window, cx)
    });
}
