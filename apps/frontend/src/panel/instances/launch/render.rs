// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The selected profile and its read-only launch diagnostics.

use axlauncher_core::launch::{LaunchCommand, flow::VersionInspection};
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, Button, Context, Disableable, ElementId, GroupBox, Hsla, IntoElement,
    Label, ScrollableElement, div, h_flex, v_flex,
};

use crate::i18n::tr;

use super::{Inspection, Output, RootView};

impl RootView {
    pub(in crate::panel) fn render_launch(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let danger = cx.theme().danger;
        let busy = self.inspection.busy();
        let java_pending = self
            .instances
            .selected()
            .is_some_and(|entry| self.java_write_pending(entry.profile.id(), cx));
        let disabled = busy || java_pending || self.selected_version().is_none();

        GroupBox::new()
            .id("launch-diagnostics")
            .title(
                Label::new(tr(cx, "launch.diagnostics.title"))
                    .text_sm()
                    .text_color(muted),
            )
            .child(
                v_flex()
                    .gap_3()
                    .child(self.inspection_target(muted, cx))
                    .child(
                        h_flex()
                            .gap_3()
                            .child(
                                Button::new("launch-inspect")
                                    .label(tr(cx, "launch.diagnostics.inspect"))
                                    .disabled(disabled)
                                    .on_click(
                                        cx.listener(|view, _, _, cx| {
                                            view.inspect_launch(false, cx)
                                        }),
                                    ),
                            )
                            .child(
                                Button::new("launch-preview")
                                    .label(tr(cx, "launch.diagnostics.preview"))
                                    .disabled(disabled || self.account_selection_pending(cx))
                                    .on_click(
                                        cx.listener(|view, _, _, cx| view.inspect_launch(true, cx)),
                                    ),
                            ),
                    )
                    .when(busy, |column| {
                        column.child(
                            Label::new(tr(cx, "launch.diagnostics.reading"))
                                .text_xs()
                                .text_color(muted),
                        )
                    })
                    .children(self.inspection.render_output(muted, danger)),
            )
            .into_any_element()
    }

    fn inspection_target(&self, muted: Hsla, cx: &axlauncher_ui::App) -> AnyElement {
        let Some(entry) = self.instances.selected() else {
            return Label::new(tr(cx, "launch.diagnostics.select-profile"))
                .text_xs()
                .text_color(muted)
                .into_any_element();
        };
        let version = self.selected_version().unwrap_or("No game version");
        v_flex()
            .gap_1()
            .child(Label::new(format!(
                "{} · {version}",
                entry.profile.instance.name
            )))
            .child(
                Label::new(entry.directory.display().to_string())
                    .text_xs()
                    .text_color(muted),
            )
            .into_any_element()
    }
}

impl Inspection {
    fn render_output(&self, muted: Hsla, danger: Hsla) -> Option<AnyElement> {
        self.output.as_ref().map(|output| match output {
            Ok(Output::Version(version)) => version_details(version, muted),
            Ok(Output::Command(command)) => command_details(command, muted),
            Err(error) => Label::new(error.clone())
                .text_xs()
                .text_color(danger)
                .into_any_element(),
        })
    }
}

fn version_details(inspection: &VersionInspection, muted: Hsla) -> AnyElement {
    let version = inspection.version();
    let asset_index = version
        .asset_index
        .as_ref()
        .map_or("None", |index| index.id.as_str());
    let inherited = inspection.parent().unwrap_or("None");
    let java = version
        .java_version
        .as_ref()
        .map_or_else(|| "System default".into(), |java| java.major.to_string());
    let lines = [
        format!("Version: {} ({})", version.id, version.kind),
        format!("Main class: {}", version.main_class),
        format!("Libraries: {}", version.libraries.len()),
        format!("Client JAR: {}", version.jar_id()),
        format!("Asset index: {asset_index}"),
        format!("Inherits from: {inherited}"),
        format!("Required Java: {java}"),
        format!(
            "Java executable: {}",
            inspection.java().map_or_else(
                || "No matching runtime found".to_owned(),
                |path| path.display().to_string()
            )
        ),
    ];
    v_flex()
        .gap_1()
        .children(line_labels(lines, "version-detail", muted))
        .into_any_element()
}

fn command_details(command: &LaunchCommand, muted: Hsla) -> AnyElement {
    let mut lines = vec![
        format!("Program: {}", command.program),
        format!("Working directory: {}", command.working_directory.display()),
        format!("Arguments ({})", command.args.len()),
    ];
    lines.extend(command.args.iter().cloned());
    lines.extend(command.environment.iter().map(|(name, value)| match value {
        Some(value) => format!("{name}={value}"),
        None => format!("{name} (removed)"),
    }));
    div()
        .id("launch-command")
        .max_h_48()
        .overflow_y_scrollbar()
        .child(
            v_flex()
                .gap_1()
                .children(line_labels(lines, "command-line", muted)),
        )
        .into_any_element()
}

fn line_labels(
    lines: impl IntoIterator<Item = String>,
    id: &'static str,
    muted: Hsla,
) -> Vec<Label> {
    lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            Label::new(line)
                .id(ElementId::named_usize(id, index))
                .text_xs()
                .text_color(muted)
        })
        .collect()
}
