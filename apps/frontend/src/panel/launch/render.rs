// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Rendering for the launch section.

use std::path::PathBuf;

use axlauncher_core::launch::LaunchCommand;
use axlauncher_core::storage::DataDir;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, Button, ButtonVariants, Context, Disableable, ElementId, GroupBox,
    Hsla, Input, IntoElement, Label, ScrollableElement, div, h_flex, px, v_flex,
};

use super::super::RootView;

fn fact(index: usize, label: &str, value: String, muted: Hsla) -> AnyElement {
    h_flex()
        .gap_2()
        .child(
            Label::new(label.to_string())
                .id(ElementId::named_usize("launch-label", index))
                .text_xs()
                .text_color(muted),
        )
        .child(
            Label::new(value)
                .id(ElementId::named_usize("launch-value", index))
                .text_xs(),
        )
        .into_any_element()
}

fn command_lines(command: &LaunchCommand) -> Vec<String> {
    let mut lines = vec![
        format!("program: {}", command.program),
        format!("working directory: {}", command.working_directory.display()),
        format!("arguments: {}", command.args.len()),
    ];
    lines.extend(command.args.iter().map(|argument| format!("  {argument}")));
    lines.extend(command.environment.iter().map(|(name, value)| match value {
        Some(value) => format!("environment: {name}={value}"),
        None => format!("environment: {name} (removed)"),
    }));
    lines
}

impl RootView {
    pub(crate) fn render_launch(&mut self, cx: &mut Context<'_, Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let success = cx.theme().success;
        let danger = cx.theme().danger;
        let facts = self.launch_facts(muted);
        let command_rows = self.launch_command_rows(muted);
        let status = self.launch_status(muted, success, danger);

        GroupBox::new()
            .id("launch")
            .title(Label::new("Launch").text_sm().text_color(muted))
            .child(
                v_flex()
                    .gap_3()
                    .child(
                        Label::new(self.launch_selection())
                            .text_xs()
                            .text_color(muted),
                    )
                    .when_some(self.launch_instance_directory(), |column, directory| {
                        column.child(
                            Label::new(directory.display().to_string())
                                .text_xs()
                                .text_color(muted),
                        )
                    })
                    .child(self.launch_actions(cx))
                    .when_some(status, |column, (message, color)| {
                        column.child(Label::new(message).text_xs().text_color(color))
                    })
                    .when_some(self.launch.preview_error.clone(), |column, error| {
                        column.child(Label::new(error).text_xs().text_color(danger))
                    })
                    .when(self.launch.inspected.is_none(), |column| {
                        column.child(
                            Label::new("Inspect a version to see its resolved fields.")
                                .text_xs()
                                .text_color(muted),
                        )
                    })
                    .children(facts)
                    .when(!command_rows.is_empty(), |column| {
                        column.child(
                            div()
                                .id("launch-command")
                                .max_h(px(180.))
                                .overflow_y_scrollbar()
                                .child(v_flex().gap_1().children(command_rows)),
                        )
                    }),
            )
            .into_any_element()
    }

    fn launch_selection(&self) -> String {
        self.instances
            .selected
            .as_ref()
            .map(|instance| {
                let version = instance
                    .game_version
                    .clone()
                    .unwrap_or_else(|| "not set".to_string());
                let loader = instance
                    .loader
                    .clone()
                    .unwrap_or_else(|| "not set".to_string());
                format!(
                    "{} ({}) · version {} · loader {}",
                    instance.name, instance.id, version, loader
                )
            })
            .unwrap_or_else(|| "Select an instance above.".to_string())
    }

    fn launch_instance_directory(&self) -> Option<PathBuf> {
        self.instances.selected.as_ref().and_then(|instance| {
            self.root
                .as_ref()
                .map(|root| DataDir::new(root).instances().join(&instance.id))
        })
    }

    fn launch_facts(&self, muted: Hsla) -> Vec<AnyElement> {
        let mut facts = Vec::new();
        if let Some(version) = &self.launch.inspected {
            facts.push(fact(0, "Version", version.id.clone(), muted));
            facts.push(fact(1, "Type", version.kind.clone(), muted));
            facts.push(fact(2, "Main class", version.main_class.clone(), muted));
            facts.push(fact(
                3,
                "Libraries",
                version.libraries.len().to_string(),
                muted,
            ));
            facts.push(fact(4, "Jar", version.jar_id().to_string(), muted));
            facts.push(fact(
                5,
                "Asset index",
                version
                    .asset_index
                    .as_ref()
                    .map(|index| index.id.clone())
                    .unwrap_or_else(|| "none".to_string()),
                muted,
            ));
            facts.push(fact(
                6,
                "Inherits from",
                version
                    .inherits_from
                    .clone()
                    .unwrap_or_else(|| "none".to_string()),
                muted,
            ));
        }
        if let Some(major) = self.launch.required_java {
            facts.push(fact(7, "Required Java", major.to_string(), muted));
        }
        facts.push(fact(
            8,
            "Java",
            self.launch
                .java
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "not found".to_string()),
            muted,
        ));
        facts
    }

    fn launch_command_rows(&self, muted: Hsla) -> Vec<AnyElement> {
        self.launch
            .command
            .as_ref()
            .map(command_lines)
            .unwrap_or_default()
            .into_iter()
            .enumerate()
            .map(|(index, line)| {
                Label::new(line)
                    .id(ElementId::named_usize("launch-line", index))
                    .text_xs()
                    .text_color(muted)
                    .into_any_element()
            })
            .collect()
    }

    fn launch_status(&self, muted: Hsla, success: Hsla, danger: Hsla) -> Option<(String, Hsla)> {
        match (&self.launch.result, self.launch.running) {
            (_, true) => Some((
                format!(
                    "Running {}...",
                    self.launch.running_label.as_deref().unwrap_or("version")
                ),
                muted,
            )),
            (Some(Ok(outcome)), false) => Some((
                format!(
                    "Launch finished; {} native files unpacked.",
                    outcome.natives
                ),
                success,
            )),
            (Some(Err(error)), false) => Some((format!("Launch failed: {error}"), danger)),
            (None, false) => None,
        }
    }
    fn launch_actions(&self, cx: &mut Context<'_, Self>) -> AnyElement {
        let has_instance = self.instances.selected.is_some();
        let can_launch = self.root.is_some() && has_instance;

        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        Input::new(&self.launch.version_input)
                            .id("launch-version-input")
                            .aria_label("Version id")
                            .w(px(220.)),
                    )
                    .child(
                        Input::new(&self.launch.player_input)
                            .id("launch-player-input")
                            .aria_label("Player name")
                            .w(px(180.)),
                    ),
            )
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        Button::new("launch-inspect")
                            .label("Inspect")
                            .disabled(self.root.is_none())
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.inspect_version(cx);
                            })),
                    )
                    .child(
                        Button::new("launch-preview")
                            .label("Preview command")
                            .disabled(!can_launch)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.preview_command(cx);
                            })),
                    )
                    .child(
                        Button::new("launch-start")
                            .label("Launch")
                            .primary()
                            .loading(self.launch.running)
                            .disabled(!can_launch || self.launch.running)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.start_launch(cx);
                            })),
                    ),
            )
            .into_any_element()
    }
}
