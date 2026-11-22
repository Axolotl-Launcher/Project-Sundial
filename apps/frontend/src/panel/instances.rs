// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Instance discovery, creation and manifest editing.

use std::path::PathBuf;

use axlauncher_core::state::{Instance, create, discover, next_instance, save};
use axlauncher_core::storage::DataDir;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, Button, ButtonVariants, Context, Disableable, ElementId, Entity,
    GroupBox, Hsla, Input, InputState, IntoElement, Label, Window, h_flex, px, v_flex,
};

use super::RootView;

pub(super) struct InstancesState {
    pub(super) items: Vec<Instance>,
    pub(super) error: Option<String>,
    pub(super) selected: Option<Instance>,
    name_input: Entity<InputState>,
    game_version_input: Entity<InputState>,
    loader_input: Entity<InputState>,
    save_error: Option<String>,
}

impl InstancesState {
    pub(super) fn new(
        items: &[Instance],
        error: Option<String>,
        window: &mut Window,
        cx: &mut Context<'_, RootView>,
    ) -> Self {
        Self {
            items: items.to_vec(),
            error,
            selected: None,
            name_input: cx.new(|cx| InputState::new(window, cx).placeholder("Name")),
            game_version_input: cx
                .new(|cx| InputState::new(window, cx).placeholder("Game version")),
            loader_input: cx.new(|cx| InputState::new(window, cx).placeholder("Loader")),
            save_error: None,
        }
    }

    pub(super) fn replace(
        &mut self,
        items: Vec<Instance>,
        error: Option<String>,
        _window: &mut Window,
        _cx: &mut Context<'_, RootView>,
    ) {
        self.items = items;
        self.error = error;
        self.save_error = None;
    }

    pub(super) fn select(
        &mut self,
        instance: Instance,
        window: &mut Window,
        cx: &mut Context<'_, RootView>,
    ) {
        let name = instance.name.clone();
        let game_version = instance.game_version.clone().unwrap_or_default();
        let loader = instance.loader.clone().unwrap_or_default();
        self.name_input
            .update(cx, |state, cx| state.set_value(name, window, cx));
        self.game_version_input
            .update(cx, |state, cx| state.set_value(game_version, window, cx));
        self.loader_input
            .update(cx, |state, cx| state.set_value(loader, window, cx));
        self.selected = Some(instance);
        self.save_error = None;
    }

    pub(super) fn clear_selection(&mut self) {
        self.selected = None;
        self.save_error = None;
    }

    fn values(&self, cx: &mut Context<'_, RootView>) -> (String, Option<String>, Option<String>) {
        let name = self.name_input.read(cx).value().trim().to_string();
        let game_version = optional(self.game_version_input.read(cx).value().trim());
        let loader = optional(self.loader_input.read(cx).value().trim());
        (name, game_version, loader)
    }
}

fn optional(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn details(instance: &Instance) -> String {
    match (&instance.game_version, &instance.loader) {
        (Some(version), Some(loader)) => format!("{version} · {loader}"),
        (Some(version), None) => version.clone(),
        (None, Some(loader)) => loader.clone(),
        (None, None) => "unknown version".to_string(),
    }
}

fn instance_row(
    index: usize,
    instance: &Instance,
    selected: bool,
    muted: Hsla,
    active: Hsla,
    active_border: Hsla,
    cx: &mut Context<'_, RootView>,
) -> AnyElement {
    let id = instance.id.clone();
    let mut row = h_flex().justify_between().items_center().gap_4().p_3();
    if selected {
        row = row.bg(active).border_1().border_color(active_border);
    }
    let row = row.child(
        v_flex()
            .gap_1()
            .child(
                Label::new(instance.name.clone())
                    .id(ElementId::named_usize("instance-name", index))
                    .text_base(),
            )
            .child(
                Label::new(format!("{} · {}", instance.id, details(instance)))
                    .id(ElementId::named_usize("instance-detail", index))
                    .text_xs()
                    .text_color(muted),
            ),
    );

    if selected {
        row.child(Label::new("Selected").text_xs().text_color(muted))
            .into_any_element()
    } else {
        row.child(
            Button::new(ElementId::named_usize("instance-use", index))
                .label("Use")
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.use_instance(&id, window, cx);
                })),
        )
        .into_any_element()
    }
}

fn controls(cx: &mut Context<'_, RootView>, enabled: bool) -> AnyElement {
    h_flex()
        .gap_3()
        .child(
            Button::new("instances-refresh")
                .label("Refresh")
                .disabled(!enabled)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.refresh_instances(window, cx);
                    cx.notify();
                })),
        )
        .child(
            Button::new("instances-new")
                .label("New instance")
                .primary()
                .disabled(!enabled)
                .on_click(cx.listener(|this, _, window, cx| {
                    this.create_instance(window, cx);
                })),
        )
        .into_any_element()
}

fn editor(cx: &mut Context<'_, RootView>, state: &InstancesState) -> AnyElement {
    h_flex()
        .gap_3()
        .child(
            Input::new(&state.name_input)
                .id("instance-name-input")
                .aria_label("Instance name")
                .w(px(200.)),
        )
        .child(
            Input::new(&state.game_version_input)
                .id("instance-version-input")
                .aria_label("Game version")
                .w(px(170.)),
        )
        .child(
            Input::new(&state.loader_input)
                .id("instance-loader-input")
                .aria_label("Loader")
                .w(px(170.)),
        )
        .child(
            Button::new("instance-save")
                .label("Save")
                .disabled(state.selected.is_none())
                .on_click(cx.listener(|this, _, window, cx| {
                    this.save_instance(window, cx);
                })),
        )
        .into_any_element()
}

impl RootView {
    pub(super) fn render_instances(&mut self, cx: &mut Context<'_, Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let danger = cx.theme().danger;
        let active = cx.theme().list_active;
        let active_border = cx.theme().list_active_border;
        let enabled = self.root.is_some();

        let directory_label = self
            .instances_directory()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "data directory unknown".to_string());
        let rows = self.instance_rows(muted, active, active_border, cx);
        let controls = controls(cx, enabled);
        let editor = editor(cx, &self.instances);

        GroupBox::new()
            .id("instances")
            .title(Label::new("Instances").text_sm().text_color(muted))
            .child(
                v_flex()
                    .gap_4()
                    .child(Label::new(directory_label).text_xs().text_color(muted))
                    .child(controls)
                    .when(rows.is_empty(), |column| {
                        column.child(
                            Label::new(
                                "No instances in this directory. Create one with New instance.",
                            )
                            .text_xs()
                            .text_color(muted),
                        )
                    })
                    .children(rows)
                    .when_some(self.instances.error.clone(), |column, error| {
                        column.child(Label::new(error).text_xs().text_color(danger))
                    })
                    .child(editor)
                    .when_some(self.instances.save_error.clone(), |column, error| {
                        column.child(Label::new(error).text_xs().text_color(danger))
                    }),
            )
            .into_any_element()
    }

    fn instance_rows(
        &self,
        muted: Hsla,
        active: Hsla,
        active_border: Hsla,
        cx: &mut Context<'_, Self>,
    ) -> Vec<AnyElement> {
        self.instances
            .items
            .iter()
            .enumerate()
            .map(|(index, instance)| {
                let selected = self
                    .instances
                    .selected
                    .as_ref()
                    .is_some_and(|selected| selected.id == instance.id);
                instance_row(index, instance, selected, muted, active, active_border, cx)
            })
            .collect()
    }

    pub(super) fn use_instance(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        let found = self
            .instances
            .items
            .iter()
            .find(|instance| instance.id == id)
            .cloned();
        if let Some(instance) = found {
            self.select_instance(instance, window, cx);
            cx.notify();
        }
    }

    fn select_by_id(&mut self, id: &str, window: &mut Window, cx: &mut Context<'_, Self>) {
        let found = self
            .instances
            .items
            .iter()
            .find(|instance| instance.id == id)
            .cloned();
        if let Some(instance) = found {
            self.select_instance(instance, window, cx);
        }
    }

    pub(super) fn create_instance(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let Some(directory) = self.instances_directory() else {
            self.instances.error = Some("Choose a data directory first.".to_string());
            cx.notify();
            return;
        };

        let Some((id, name)) = next_instance(&directory) else {
            let message = "No free instance name after 100 attempts.".to_string();
            self.instances.error = Some(message.clone());
            self.activity.error(message);
            cx.notify();
            return;
        };

        match create(&directory, &Instance::new(id.clone(), name)) {
            Ok(()) => {
                self.activity
                    .success(format!("Created {id} under {}", directory.display()));
                self.instances.error = None;
                self.refresh_instances(window, cx);
                self.select_by_id(&id, window, cx);
            }
            Err(error) => {
                let message = error.to_string();
                self.instances.error = Some(message.clone());
                self.activity.error(format!("Create failed: {message}"));
            }
        }
        cx.notify();
    }

    pub(super) fn select_instance(
        &mut self,
        instance: Instance,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) {
        self.launch
            .set_version(instance.game_version.as_deref(), window, cx);
        self.instances.select(instance, window, cx);
    }

    pub(super) fn save_instance(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let Some(selected) = self.instances.selected.clone() else {
            self.instances.save_error = Some("Select an instance first.".to_string());
            cx.notify();
            return;
        };
        let Some(directory) = self.instances_directory() else {
            self.instances.save_error = Some("Choose a data directory first.".to_string());
            cx.notify();
            return;
        };

        let (name, game_version, loader) = self.instances.values(cx);
        let mut updated = selected;
        updated.name = name;
        updated.game_version = game_version;
        updated.loader = loader;

        match save(&directory, &updated) {
            Ok(()) => {
                self.activity.success(format!("Saved {}", updated.id));
                self.instances.save_error = None;
                self.refresh_instances(window, cx);
            }
            Err(error) => {
                let message = error.to_string();
                self.instances.save_error = Some(message.clone());
                self.activity.error(format!("Save failed: {message}"));
            }
        }
        cx.notify();
    }

    pub(super) fn refresh_instances(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        let Some(directory) = self.instances_directory() else {
            self.instances.error = Some("Choose a data directory first.".to_string());
            return;
        };
        let selected_id = self
            .instances
            .selected
            .as_ref()
            .map(|instance| instance.id.clone());

        match discover(&directory) {
            Ok(items) => self.instances.replace(items, None, window, cx),
            Err(error) => {
                let message = error.to_string();
                self.instances
                    .replace(Vec::new(), Some(message.clone()), window, cx);
                self.activity.error(format!("Discover failed: {message}"));
            }
        }

        let next = selected_id
            .and_then(|id| {
                self.instances
                    .items
                    .iter()
                    .find(|instance| instance.id == id)
                    .cloned()
            })
            .or_else(|| self.instances.items.first().cloned());
        match next {
            Some(instance) => self.select_instance(instance, window, cx),
            None => {
                self.instances.clear_selection();
                self.launch.clear();
            }
        }
    }

    fn instances_directory(&self) -> Option<PathBuf> {
        self.root
            .as_ref()
            .map(|root| DataDir::new(root).instances())
    }
}
