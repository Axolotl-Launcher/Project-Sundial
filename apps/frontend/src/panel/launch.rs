// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Read-only version and launch diagnostics for the selected profile.

use axlauncher_core::launch::{LaunchCommand, LaunchRequest, flow};
use axlauncher_ui::prelude::*;
use axlauncher_ui::{Context, Task};
use std::path::Path;

use super::{RootView, profile_version};

mod render;

#[derive(Default)]
pub(super) struct Inspection {
    output: Option<Result<Output, String>>,
    task: Option<Task<()>>,
}

enum Output {
    Version(Box<flow::VersionInspection>),
    Command(LaunchCommand),
}

impl RootView {
    fn selected_version(&self) -> Option<&str> {
        let game = self.instances.selected()?.profile.game.as_ref()?;
        profile_version(game)
    }

    fn inspect_launch(&mut self, preview: bool, cx: &mut Context<'_, Self>) {
        let Some(entry) = self.instances.selected() else {
            return;
        };
        let Some(request) = self.launch_request(entry, self.player_name(cx)) else {
            return;
        };
        let Some(data_root) = self.root.clone() else {
            return;
        };
        let task = cx.background_spawn(async move { inspect(&request, preview, &data_root) });
        self.inspection.output = None;
        self.inspection.task = Some(cx.spawn(async move |view, cx| {
            let output = task.await;
            let _ = view.update(cx, |view, cx| view.finish_inspection(output, cx));
        }));
        cx.notify();
    }

    fn finish_inspection(&mut self, output: Result<Output, String>, cx: &mut Context<'_, Self>) {
        self.inspection.output = Some(output);
        self.inspection.task = None;
        cx.notify();
    }
}

fn inspect(request: &LaunchRequest, preview: bool, data_root: &Path) -> Result<Output, String> {
    let java = crate::java::managed(data_root);
    if preview {
        return flow::preview(&java, request)
            .map(Output::Command)
            .map_err(|error| error.to_string());
    }
    flow::inspect_version(&java, request)
        .map(Box::new)
        .map(Output::Version)
        .map_err(|error| error.to_string())
}
