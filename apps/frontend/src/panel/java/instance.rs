// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Java selection for the currently displayed instance.

use std::path::{Path, PathBuf};

use axlauncher_core::desktop::java::JavaCatalog;
use axlauncher_ui as gpui;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, App, Button, Context, Disableable, DropdownMenu, ElementId, Entity,
    IconName, IntoElement, Label, PopupMenu, PopupMenuItem, RenderOnce, Sizable, WeakEntity,
    Window, v_flex,
};

use super::{JavaPanel, Operation, java_feedback, java_id, java_path_label, runtime_title};

#[derive(IntoElement)]
pub(in crate::panel) struct InstancePicker {
    panel: Entity<JavaPanel>,
    profile: String,
    name: String,
}

impl InstancePicker {
    pub(in crate::panel) fn new(panel: Entity<JavaPanel>, profile: &str, name: &str) -> Self {
        Self {
            panel,
            profile: profile.to_owned(),
            name: name.to_owned(),
        }
    }

    fn render_control(&self, panel: &JavaPanel, label: String, cx: &App) -> AnyElement {
        let Some(catalog) = panel.snapshot() else {
            return java_picker_placeholder(panel.busy(), cx);
        };
        let selected = catalog.profile_preference(&self.profile);
        let title = selection_title(&catalog, &self.profile);
        let target = InstanceChoice {
            panel: self.panel.downgrade(),
            scope: panel.scope,
            profile: self.profile.clone(),
        };
        let id = ElementId::Name(format!("instance-java-menu:{}", self.profile).into());
        let trigger = Button::new(id)
            .label(title)
            .accessibility_label(label)
            .icon(IconName::ChevronDown)
            .outline()
            .small()
            .when_some(selected, |trigger, path| {
                trigger.tooltip(path.display().to_string())
            });
        if panel.busy() {
            return trigger.disabled(true).into_any_element();
        }
        trigger
            .dropdown_menu(move |menu, _, _| target.menu(menu, &catalog))
            .into_any_element()
    }
}

impl RenderOnce for InstancePicker {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let panel = self.panel.read(cx);
        let label = format!("Java for {}", self.name);
        v_flex()
            .gap_1()
            .child(Label::new(label.clone()).text_sm().truncate())
            .child(self.render_control(panel, label, cx))
            .children(java_feedback(panel, Some(&self.profile), cx))
    }
}

fn java_picker_placeholder(busy: bool, cx: &App) -> AnyElement {
    let message = if busy {
        "Loading Java list…"
    } else {
        "Java list unavailable. Refresh the Java section to try again."
    };
    Label::new(message)
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .into_any_element()
}

fn selection_title(catalog: &JavaCatalog, profile: &str) -> String {
    if catalog.profile_preference(profile).is_none() {
        return "Automatic".into();
    }
    catalog.profile_runtime(profile).map_or_else(
        || "Automatic (saved Java unavailable)".into(),
        runtime_title,
    )
}

#[derive(Clone)]
struct InstanceChoice {
    panel: WeakEntity<JavaPanel>,
    scope: u64,
    profile: String,
}

impl InstanceChoice {
    fn menu(&self, menu: PopupMenu, catalog: &JavaCatalog) -> PopupMenu {
        let selected = catalog.profile_preference(&self.profile);
        let mut menu = menu
            .scrollable(true)
            .item(self.item("Automatic".into(), None, selected));
        for runtime in catalog.available() {
            let title = format!("{} · {}", runtime_title(runtime), runtime.path().display());
            menu = menu.item(self.item(title, Some(runtime.path().to_owned()), selected));
        }
        if let Some(path) = selected
            && let Some(runtime) = catalog.profile_runtime(&self.profile)
            && !catalog
                .available()
                .iter()
                .any(|available| available.path() == path)
        {
            let title = format!("{} · {}", runtime_title(runtime), path.display());
            menu = menu.item(self.item(title, Some(path.to_owned()), selected));
        }
        menu
    }

    fn item(&self, label: String, path: Option<PathBuf>, selected: Option<&Path>) -> PopupMenuItem {
        let checked = path.as_deref() == selected;
        let id = path.as_deref().map_or_else(
            || ElementId::Name("instance-java-automatic".into()),
            |path| java_id("instance-java-option", path),
        );
        let item = PopupMenuItem::element(move |_, cx| {
            java_path_label(id.clone(), label.clone(), cx.theme().foreground)
        })
        .checked(checked);
        if checked {
            return item;
        }
        let target = self.clone();
        item.on_click(move |_, _, cx| {
            let _ = target
                .panel
                .update(cx, |view, cx| target.select(view, path.clone(), cx));
        })
    }

    fn select(&self, view: &mut JavaPanel, path: Option<PathBuf>, cx: &mut Context<'_, JavaPanel>) {
        if self.scope != view.scope {
            return;
        }
        view.run(
            Operation::Select {
                profile: self.profile.clone(),
                path,
            },
            cx,
        );
    }
}
