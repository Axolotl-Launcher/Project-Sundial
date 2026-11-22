// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The panel's in-memory activity log.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use axlauncher_ui::prelude::*;
use axlauncher_ui::{
    ActiveTheme, AnyElement, Button, ButtonVariants, Context, ElementId, GroupBox, IntoElement,
    Label, ScrollableElement, div, h_flex, px, v_flex,
};

use super::RootView;

const MAX_ENTRIES: usize = 100;

#[derive(Clone, Copy)]
pub(super) enum Level {
    Info,
    Success,
    Error,
}

struct Entry {
    at: Duration,
    level: Level,
    message: String,
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
        self.push(Level::Info, message.into());
    }

    pub(super) fn success(&mut self, message: impl Into<String>) {
        self.push(Level::Success, message.into());
    }

    pub(super) fn error(&mut self, message: impl Into<String>) {
        self.push(Level::Error, message.into());
    }

    fn push(&mut self, level: Level, message: String) {
        let at = self.start.elapsed();
        self.entries.push_front(Entry { at, level, message });
        self.entries.truncate(MAX_ENTRIES);
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
    }
}

impl RootView {
    pub(super) fn render_activity(&mut self, cx: &mut Context<'_, Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let success = cx.theme().success;
        let danger = cx.theme().danger;

        let rows = self
            .activity
            .entries
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
                        Label::new(entry.message.clone())
                            .id(ElementId::named_usize("activity-message", index))
                            .text_xs()
                            .text_color(color),
                    )
            })
            .collect::<Vec<_>>();

        GroupBox::new()
            .id("activity")
            .title(Label::new("Activity").text_sm().text_color(muted))
            .child(
                v_flex()
                    .gap_2()
                    .when(rows.is_empty(), |column| {
                        column.child(Label::new("No activity yet.").text_xs().text_color(muted))
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
                                .label("Clear")
                                .ghost()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.activity.clear();
                                    cx.notify();
                                })),
                        ),
                    ),
            )
            .into_any_element()
    }
}
