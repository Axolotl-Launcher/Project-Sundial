// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The instance rename form and its save feedback.

use std::cell::Cell;

use axlauncher_core::domain::is_visible_instance_name;
use axlauncher_core::state::profile::ProfileFolderName;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, App, AsyncWindowContext, Button, ButtonVariants, Checkbox, Context, Disableable,
    Entity, FocusHandle, Focusable, Input, InputEvent, InputState, IntoElement, Label, Render,
    Task, WeakEntity, Window, WindowExt, h_flex, v_flex,
};

type Save = Task<Result<(), String>>;
type Commit = dyn Fn(String, Option<ProfileFolderName>, &mut App) -> Result<Save, String>;

pub(super) fn open(
    name: String,
    window: &mut Window,
    cx: &mut App,
    commit: impl Fn(String, Option<ProfileFolderName>, &mut App) -> Result<Save, String> + 'static,
) {
    let editor = cx.new(|cx| RenameEditor::new(Box::new(commit), name, window, cx));
    let first_render = Cell::new(true);
    window.open_dialog(cx, move |dialog, window, cx| {
        if first_render.replace(false) {
            let focus = editor.read(cx).name_focus(cx);
            focus.focus(window, cx);
        }
        let confirm = editor.clone();
        let cancel = editor.clone();
        dialog
            .title("Rename instance")
            .close_button(false)
            .overlay_closable(false)
            .on_ok(move |_, window, cx| {
                confirm.update(cx, |editor, cx| editor.submit(window, cx));
                false
            })
            .on_cancel(move |_, _, cx| cancel.read(cx).is_dismissible())
            .child(editor.clone())
    });
}

struct RenameEditor {
    commit: Box<Commit>,
    name: Entity<InputState>,
    rename_folder: bool,
    saving: bool,
    error: Option<String>,
}

impl RenameEditor {
    fn name_focus(&self, cx: &App) -> FocusHandle {
        self.name.read(cx).focus_handle(cx)
    }

    fn is_dismissible(&self) -> bool {
        !self.saving
    }

    fn new(
        commit: Box<Commit>,
        name: String,
        window: &mut Window,
        cx: &mut Context<'_, Self>,
    ) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).default_value(name));
        cx.subscribe(&name, Self::name_changed).detach();
        Self {
            commit,
            name,
            rename_folder: false,
            saving: false,
            error: None,
        }
    }

    fn name_changed(
        &mut self,
        _: Entity<InputState>,
        event: &InputEvent,
        cx: &mut Context<'_, Self>,
    ) {
        if matches!(event, InputEvent::Change) {
            self.error = None;
            cx.notify();
        }
    }

    fn folder(&self, name: &str) -> Result<Option<ProfileFolderName>, String> {
        if !is_visible_instance_name(name) {
            return Err("Enter an instance name.".into());
        }
        self.rename_folder
            .then(|| ProfileFolderName::new(name))
            .transpose()
            .map_err(|error| error.to_string())
    }

    fn submit(&mut self, window: &mut Window, cx: &mut Context<'_, Self>) {
        if self.saving {
            return;
        }
        let name = self.name.read(cx).value();
        let name = name.trim();
        let Ok(folder) = self.folder(name) else {
            return;
        };
        let task = (self.commit)(name.to_owned(), folder, cx);
        let task = match task {
            Ok(task) => task,
            Err(error) => {
                self.error = Some(error);
                cx.notify();
                return;
            }
        };
        self.saving = true;
        self.error = None;
        cx.spawn_in(window, async move |view, cx| {
            Self::complete(view, task.await, cx)
        })
        .detach();
        cx.notify();
    }

    fn complete(view: WeakEntity<Self>, result: Result<(), String>, cx: &mut AsyncWindowContext) {
        let _ = view.update_in(cx, |editor, window, cx| {
            editor.saving = false;
            match result {
                Ok(()) => window.close_dialog(cx),
                Err(error) => editor.error = Some(error),
            }
            cx.notify();
        });
    }

    fn toggle_folder(&mut self, checked: &bool, _: &mut Window, cx: &mut Context<'_, Self>) {
        self.rename_folder = *checked;
        self.error = None;
        cx.notify();
    }

    fn actions(&self, invalid: bool, cx: &mut Context<'_, Self>) -> impl IntoElement + use<> {
        h_flex()
            .gap_2()
            .justify_end()
            .child(
                Button::new("cancel-instance-rename")
                    .label("Cancel")
                    .disabled(self.saving)
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                Button::new("save-instance-rename")
                    .label("Rename")
                    .primary()
                    .disabled(self.saving || invalid)
                    .loading(self.saving)
                    .on_click(cx.listener(|editor, _, window, cx| editor.submit(window, cx))),
            )
    }
}

impl Render for RenameEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<'_, Self>) -> impl IntoElement {
        let name = self.name.read(cx).value();
        let name = name.trim();
        let folder = self.folder(name);
        let destination = matches!(folder, Ok(Some(_))).then_some(name);
        let error = self.error.as_ref().or_else(|| folder.as_ref().err());
        v_flex()
            .gap_3()
            .child(
                v_flex()
                    .gap_1()
                    .child(Label::new("Instance name").text_sm())
                    .child(
                        Input::new(&self.name)
                            .id("instance-rename-name")
                            .aria_label("Instance name")
                            .disabled(self.saving),
                    ),
            )
            .child(
                Checkbox::new("instance-rename-folder")
                    .label("Also rename the instance folder")
                    .checked(self.rename_folder)
                    .disabled(self.saving)
                    .on_change(cx.listener(Self::toggle_folder)),
            )
            .when_some(destination, |form, name| {
                form.child(
                    Label::new(format!("Folder name · {name}"))
                        .text_sm()
                        .text_color(cx.theme().muted_foreground),
                )
            })
            .when_some(error, |form, error| {
                form.child(
                    Label::new(error.clone())
                        .text_sm()
                        .text_color(cx.theme().danger),
                )
            })
            .child(self.actions(folder.is_err(), cx))
    }
}
