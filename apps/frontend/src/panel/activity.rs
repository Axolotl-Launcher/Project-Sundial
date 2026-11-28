// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The panel's in-memory activity log.

use std::collections::VecDeque;
use std::path::Path;
use std::time::{Duration, Instant};

use axlauncher_core::state::LauncherLanguage;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, App, Button, ButtonVariants, Context, Div, ElementId, GroupBox,
    IntoElement, Label, ScrollableElement, div, h_flex, px, v_flex,
};

use crate::i18n::{interpolate, language, text, tr};
use crate::launches::Launch;

use super::RootView;

const MAX_ENTRIES: usize = 100;

#[derive(Clone, Copy)]
enum Level {
    Info,
    Success,
    Error,
}

struct Entry {
    at: Duration,
    level: Level,
    message: Message,
}

enum Message {
    Diagnostic(String),
    Launch {
        directory: String,
        version: String,
        status: Launch,
    },
}

impl Message {
    fn render(&self, language: LauncherLanguage) -> String {
        match self {
            Self::Diagnostic(message) => message.clone(),
            Self::Launch {
                directory,
                version,
                status,
            } => interpolate(
                text(language, "activity.launch"),
                &[
                    ("directory", directory),
                    ("version", version),
                    ("status", status.message(language)),
                ],
            ),
        }
    }
}

pub(super) struct ActivityLog {
    start: Instant,
    entries: VecDeque<Entry>,
}

impl ActivityLog {
    pub(super) fn new() -> Self {
        Self {
            start: Instant::now(),
            entries: VecDeque::new(),
        }
    }

    pub(super) fn info(&mut self, message: impl Into<String>) {
        self.push(Level::Info, Message::Diagnostic(message.into()));
    }

    pub(super) fn success(&mut self, message: impl Into<String>) {
        self.push(Level::Success, Message::Diagnostic(message.into()));
    }

    pub(super) fn error(&mut self, message: impl Into<String>) {
        self.push(Level::Error, Message::Diagnostic(message.into()));
    }

    pub(super) fn record_launch(&mut self, directory: &Path, version: &str, status: &Launch) {
        let level = match status {
            Launch::Starting | Launch::Running => Level::Info,
            Launch::Exited => Level::Success,
            Launch::NeedsInstall(_) | Launch::Failed(_) => Level::Error,
        };
        self.push(
            level,
            Message::Launch {
                directory: directory.display().to_string(),
                version: version.to_owned(),
                status: status.clone(),
            },
        );
    }

    fn push(&mut self, level: Level, message: Message) {
        let at = self.start.elapsed();
        self.entries.push_front(Entry { at, level, message });
        self.entries.truncate(MAX_ENTRIES);
    }

    fn clear(&mut self) {
        self.entries.clear();
    }

    fn rows(&self, cx: &App) -> Vec<Div> {
        let muted = cx.theme().muted_foreground;
        let success = cx.theme().success;
        let danger = cx.theme().danger;
        let language = language(cx);

        self.entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let color = match entry.level {
                    Level::Info => muted,
                    Level::Success => success,
                    Level::Error => danger,
                };
                h_flex()
                    .gap_2()
                    .child(
                        Label::new(format!("{:>7.2}s", entry.at.as_secs_f64()))
                            .id(ElementId::named_usize("activity-time", index))
                            .text_xs()
                            .text_color(muted),
                    )
                    .child(
                        Label::new(entry.message.render(language))
                            .id(ElementId::named_usize("activity-message", index))
                            .text_xs()
                            .text_color(color),
                    )
            })
            .collect()
    }
}

impl RootView {
    pub(super) fn render_activity(&self, cx: &Context<'_, Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let rows = self.activity.rows(cx);
        GroupBox::new()
            .id("activity")
            .title(
                Label::new(tr(cx, "activity.title"))
                    .text_sm()
                    .text_color(muted),
            )
            .child(
                v_flex()
                    .gap_2()
                    .when(rows.is_empty(), |column| {
                        column.child(
                            Label::new(tr(cx, "activity.empty"))
                                .text_xs()
                                .text_color(muted),
                        )
                    })
                    .child(
                        div()
                            .id("activity-scroll")
                            .max_h(px(180.))
                            .overflow_y_scrollbar()
                            .child(v_flex().gap_1().children(rows)),
                    )
                    .child(
                        h_flex().justify_end().child(
                            Button::new("activity-clear")
                                .label(tr(cx, "activity.clear"))
                                .ghost()
                                .on_click(cx.listener(|view, _, _, cx| {
                                    view.activity.clear();
                                    cx.notify();
                                })),
                        ),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/frontend/src/panel/activity.rs"]
mod tests;
