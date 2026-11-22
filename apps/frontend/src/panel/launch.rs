// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Version inspection, launch command preview and the actual launch.

use std::path::{Path, PathBuf};

use axlauncher_core::BRAND_NAME;
use axlauncher_core::launch::{
    Java, JavaMode, LaunchCommand, LaunchOutcome, LaunchRequest, Layout, ProcessLauncher, Session,
    SystemJava, Target, Version, flow, plan, version,
};
use axlauncher_core::storage::DataDir;
use axlauncher_ui::prelude::*;
use axlauncher_ui::{AsyncApp, Context, Entity, InputState, WeakEntity, Window};

use super::RootView;

mod render;

const NATIVES_DIR: &str = "natives";
const DEFAULT_PLAYER: &str = "Player";

#[derive(Debug, Clone, PartialEq, Eq)]
struct LaunchPaths {
    game_root: PathBuf,
    instance_directory: PathBuf,
    versions_root: PathBuf,
}

fn launch_paths(root: &Path, instance_id: &str) -> LaunchPaths {
    let data = DataDir::new(root);
    LaunchPaths {
        game_root: root.to_path_buf(),
        instance_directory: data.instances().join(instance_id),
        versions_root: data.versions(),
    }
}

pub(super) struct LaunchSection {
    version_input: Entity<InputState>,
    player_input: Entity<InputState>,
    inspected: Option<Version>,
    required_java: Option<u32>,
    java: Option<PathBuf>,
    command: Option<LaunchCommand>,
    preview_error: Option<String>,
    running: bool,
    running_label: Option<String>,
    attempt: u64,
    pending: Option<(u64, LaunchRequest)>,
    result: Option<Result<LaunchOutcome, String>>,
}

impl LaunchSection {
    pub(super) fn new(window: &mut Window, cx: &mut Context<'_, RootView>) -> Self {
        Self {
            version_input: cx.new(|cx| InputState::new(window, cx).placeholder("Version id")),
            player_input: cx.new(|cx| InputState::new(window, cx).default_value(DEFAULT_PLAYER)),
            inspected: None,
            required_java: None,
            java: None,
            command: None,
            preview_error: None,
            running: false,
            running_label: None,
            attempt: 0,
            pending: None,
            result: None,
        }
    }

    pub(super) fn set_version(
        &mut self,
        version: Option<&str>,
        window: &mut Window,
        cx: &mut Context<'_, RootView>,
    ) {
        let value = version.unwrap_or_default().to_string();
        self.version_input
            .update(cx, |state, cx| state.set_value(value, window, cx));
        self.clear_output();
    }

    pub(super) fn clear(&mut self) {
        self.clear_output();
    }

    fn clear_output(&mut self) {
        self.inspected = None;
        self.required_java = None;
        self.java = None;
        self.command = None;
        self.preview_error = None;
        self.result = None;
    }

    pub(super) fn finish(
        &mut self,
        attempt: u64,
        result: Result<LaunchOutcome, String>,
    ) -> Option<Result<LaunchOutcome, String>> {
        if attempt != self.attempt || !self.running {
            return None;
        }
        self.running = false;
        self.running_label = None;
        Some(result)
    }
}

struct Preview {
    version: Version,
    required: Option<u32>,
    java: PathBuf,
    command: LaunchCommand,
}

fn preview(root: &Path, instance_id: &str, id: &str, player: &str) -> Result<Preview, String> {
    let paths = launch_paths(root, instance_id);
    let version =
        version::load_resolved(&paths.versions_root, id).map_err(|error| error.to_string())?;
    let java = SystemJava::from_environment();
    let (required, executable) = resolve_java(&java, &version);
    let executable = executable.ok_or_else(|| missing_java(required))?;
    let layout = Layout {
        game_root: paths.game_root,
        game_directory: paths.instance_directory.clone(),
        natives_directory: paths.instance_directory.join(NATIVES_DIR),
        launcher_name: BRAND_NAME.to_string(),
        launcher_version: env!("CARGO_PKG_VERSION").to_string(),
    };
    let command = plan::build(
        &executable,
        &version,
        &layout,
        &Session::offline(player),
        &Target::host(),
    )
    .map_err(|error| error.to_string())?;

    Ok(Preview {
        version,
        required,
        java: executable,
        command,
    })
}

fn resolve_java(java: &SystemJava, version: &Version) -> (Option<u32>, Option<PathBuf>) {
    match version.java_version.as_ref().map(|java| java.major) {
        Some(major) => (Some(major), java.executable_for(major, JavaMode::Windowed)),
        None => (None, java.executable(None, JavaMode::Windowed)),
    }
}

fn missing_java(required: Option<u32>) -> String {
    match required {
        Some(major) => format!("No Java {major} runtime found in JAVA_HOME or PATH."),
        None => "No Java runtime found in JAVA_HOME or PATH.".to_string(),
    }
}

async fn run_launch(request: LaunchRequest) -> Result<LaunchOutcome, String> {
    let java = SystemJava::from_environment();
    let launcher = ProcessLauncher;
    flow::launch(&java, &launcher, request)
        .await
        .map_err(|error| error.to_string())
}

async fn drive_launch(weak: WeakEntity<RootView>, cx: &mut AsyncApp) {
    let Ok(Some((attempt, request))) = weak.update(cx, |view, _| view.launch.pending.take()) else {
        return;
    };
    let outcome = cx.background_executor().spawn(run_launch(request)).await;
    let _ = weak.update(cx, |view, cx| {
        view.apply_launch_result(attempt, outcome, cx);
    });
}

impl RootView {
    fn launch_inputs(&self, cx: &mut Context<'_, Self>) -> Result<(String, String), &'static str> {
        let id = self
            .launch
            .version_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        let player = self.launch.player_input.read(cx).value().trim().to_string();
        if id.is_empty() {
            return Err("Enter a version id.");
        }
        if player.is_empty() {
            return Err("Enter a player name.");
        }
        Ok((id, player))
    }

    fn fail(&mut self, message: impl Into<String>, cx: &mut Context<'_, Self>) {
        let message = message.into();
        self.launch.preview_error = Some(message.clone());
        self.activity.error(message);
        cx.notify();
    }

    pub(super) fn inspect_version(&mut self, cx: &mut Context<'_, Self>) {
        let Some(root) = self.root.clone() else {
            self.fail("Choose a data directory first.", cx);
            return;
        };
        let id = self
            .launch
            .version_input
            .read(cx)
            .value()
            .trim()
            .to_string();
        if id.is_empty() {
            self.fail("Enter a version id.", cx);
            return;
        }

        match version::load_resolved(&DataDir::new(&root).versions(), &id) {
            Ok(loaded) => {
                let java = SystemJava::from_environment();
                let (required, executable) = resolve_java(&java, &loaded);
                self.launch.inspected = Some(loaded);
                self.launch.required_java = required;
                self.launch.java = executable;
                self.launch.command = None;
                self.launch.preview_error = None;
                self.launch.result = None;
                self.activity.success(format!("Inspected version {id}"));
                cx.notify();
            }
            Err(error) => self.fail(format!("Inspect failed: {error}"), cx),
        }
    }

    pub(super) fn preview_command(&mut self, cx: &mut Context<'_, Self>) {
        let Some(root) = self.root.clone() else {
            self.fail("Choose a data directory first.", cx);
            return;
        };
        let Some(instance) = self.instances.selected.clone() else {
            self.fail("Select an instance first.", cx);
            return;
        };
        let (id, player) = match self.launch_inputs(cx) {
            Ok(inputs) => inputs,
            Err(message) => {
                self.fail(message, cx);
                return;
            }
        };

        match preview(&root, &instance.id, &id, &player) {
            Ok(preview) => {
                self.launch.inspected = Some(preview.version);
                self.launch.required_java = preview.required;
                self.launch.java = Some(preview.java);
                self.launch.command = Some(preview.command);
                self.launch.preview_error = None;
                self.activity
                    .success(format!("Built launch command for {id}"));
                cx.notify();
            }
            Err(message) => self.fail(format!("Preview failed: {message}"), cx),
        }
    }

    pub(super) fn start_launch(&mut self, cx: &mut Context<'_, Self>) {
        if self.launch.running {
            return;
        }
        let Some(root) = self.root.clone() else {
            self.fail("Choose a data directory first.", cx);
            return;
        };
        let Some(instance) = self.instances.selected.clone() else {
            self.fail("Select an instance first.", cx);
            return;
        };
        let (id, player) = match self.launch_inputs(cx) {
            Ok(inputs) => inputs,
            Err(message) => {
                self.fail(message, cx);
                return;
            }
        };

        let paths = launch_paths(&root, &instance.id);
        let request = LaunchRequest {
            game_root: paths.game_root,
            instance_directory: paths.instance_directory,
            version_id: id.clone(),
            java_preference: None,
            player_name: player,
        };

        self.launch.attempt = self.launch.attempt.wrapping_add(1);
        let attempt = self.launch.attempt;
        self.launch.running = true;
        self.launch.running_label = Some(id.clone());
        self.launch.result = None;
        self.launch.preview_error = None;
        self.launch.pending = Some((attempt, request));
        self.activity.info(format!("Launching {id}..."));

        cx.spawn(drive_launch).detach();
        cx.notify();
    }

    fn apply_launch_result(
        &mut self,
        attempt: u64,
        outcome: Result<LaunchOutcome, String>,
        cx: &mut Context<'_, Self>,
    ) {
        let Some(result) = self.launch.finish(attempt, outcome) else {
            return;
        };
        match &result {
            Ok(outcome) => self.activity.success(format!(
                "Launch finished; {} native files unpacked.",
                outcome.natives
            )),
            Err(error) => self.activity.error(format!("Launch failed: {error}")),
        }
        self.launch.result = Some(result);
        cx.notify();
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/frontend/src/panel/launch/tests.rs"]
mod tests;
