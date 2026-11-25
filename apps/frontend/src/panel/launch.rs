// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Read-only version and launch diagnostics for the selected profile.

use axlauncher_core::desktop::launching::LaunchJob;
use axlauncher_core::launch::{LaunchCommand, flow, plan::Session};
use axlauncher_ui::prelude::*;
use axlauncher_ui::{Context, Task};

use super::RootView;

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
        let game = self.instances.selected()?.profile.game()?;
        game.effective_version()
    }

    fn inspect_launch(&mut self, preview: bool, cx: &mut Context<'_, Self>) {
        let Some(entry) = self.instances.selected() else {
            return;
        };
        let Some(job) = self.launch_job(entry, self.player_name(cx)) else {
            return;
        };
        let session = preview.then(|| self.preview_session(cx));
        let task = cx.background_spawn(async move { inspect(&job, session.as_ref()) });
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

fn inspect(job: &LaunchJob, session: Option<&Session>) -> Result<Output, String> {
    if let Some(session) = session {
        return job
            .preview(session)
            .map(Output::Command)
            .map_err(|error| error.to_string());
    }
    job.inspect()
        .map(Box::new)
        .map(Output::Version)
        .map_err(|error| error.to_string())
}
