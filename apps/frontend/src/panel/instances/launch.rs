// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Read-only version and launch diagnostics for the selected profile.

use axlauncher_core::desktop::launching::LaunchJob;
use axlauncher_core::launch::{LaunchCommand, flow, plan::Session};
use axlauncher_ui::prelude::*;
use axlauncher_ui::{Context, Task};

use super::super::RootView;

mod render;

#[derive(Default)]
pub(in crate::panel) struct Inspection {
    output: Option<Result<Output, String>>,
    task: Option<Task<()>>,
    predates_java_refresh: bool,
}

impl Inspection {
    pub(in crate::panel) fn mark_java_refresh(&mut self) {
        self.predates_java_refresh = self.output.is_some() || self.task.is_some();
    }

    pub(in crate::panel) fn invalidate_refreshed_catalog(&mut self) {
        if self.predates_java_refresh {
            *self = Self::default();
        }
    }

    fn begin_request(&mut self) {
        self.output = None;
        self.predates_java_refresh = false;
    }

    fn track(&mut self, task: Task<()>) {
        self.begin_request();
        self.task = Some(task);
    }

    fn finish(&mut self, output: Result<Output, String>) {
        self.output = Some(output);
        self.task = None;
    }

    fn busy(&self) -> bool {
        self.task.is_some()
    }
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
        if self.java_write_pending(entry.profile.id(), cx) {
            return;
        }
        let Some(job) = self.launch_job(entry, self.player_name(cx)) else {
            return;
        };
        let session = preview.then(|| self.preview_session(cx));
        let task = cx.background_spawn(async move { inspect(&job, session.as_ref()) });
        self.inspection.track(cx.spawn(async move |view, cx| {
            let output = task.await;
            let _ = view.update(cx, |view, cx| view.finish_inspection(output, cx));
        }));
        cx.notify();
    }

    fn finish_inspection(&mut self, output: Result<Output, String>, cx: &mut Context<'_, Self>) {
        self.inspection.finish(output);
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

#[cfg(test)]
#[path = "../../../../../tests/apps/frontend/src/panel/instances/launch.rs"]
mod tests;
