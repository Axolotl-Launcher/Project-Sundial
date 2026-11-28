// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::task::Poll;

/// An exit code the child can report back; any non-zero value would do.
const FAILURE_CODE: i32 = 3;
const SUCCESS_CODE: i32 = 0;

/// A variable whose only value is the one the launcher supplies.
const GIVEN_NAME: &str = "AXLAUNCHER_LAUNCH_GIVEN";
const GIVEN_VALUE: &str = "launch-provided";

/// A variable every test runner inherits, so removing it is observable.
const REMOVED_NAME: &str = "PATH";

/// Windows needs the command processor's full path when `PATH` is removed
/// from the child.
#[cfg(windows)]
fn shell() -> String {
    std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_owned())
}

#[cfg(windows)]
fn exit_script(code: i32) -> (String, Vec<String>) {
    (shell(), vec!["/C".to_owned(), format!("exit {code}")])
}

#[cfg(not(windows))]
fn exit_script(code: i32) -> (String, Vec<String>) {
    (
        "/bin/sh".to_owned(),
        vec!["-c".to_owned(), format!("exit {code}")],
    )
}

#[cfg(windows)]
fn echo_variable(name: &str) -> (String, Vec<String>) {
    (shell(), vec!["/C".to_owned(), format!("echo %{name}%")])
}

#[cfg(not(windows))]
fn echo_variable(name: &str) -> (String, Vec<String>) {
    (
        "/bin/sh".to_owned(),
        vec!["-c".to_owned(), format!("printf %s \"${name}\"")],
    )
}

#[cfg(windows)]
fn environment_dump() -> (String, Vec<String>) {
    (shell(), vec!["/C".to_owned(), "set".to_owned()])
}

/// `env` is spawned directly rather than through a shell, because a shell
/// may supply a default `PATH` when the child has none.
#[cfg(not(windows))]
fn environment_dump() -> (String, Vec<String>) {
    ("/usr/bin/env".to_owned(), Vec::new())
}

#[cfg(windows)]
fn print_working_directory() -> (String, Vec<String>) {
    (shell(), vec!["/C".to_owned(), "cd".to_owned()])
}

#[cfg(not(windows))]
fn print_working_directory() -> (String, Vec<String>) {
    (
        "/bin/sh".to_owned(),
        vec!["-c".to_owned(), "pwd -P".to_owned()],
    )
}

/// Windows environment names are case-insensitive; POSIX ones are not.
#[cfg(windows)]
fn same_variable_name(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

#[cfg(not(windows))]
fn same_variable_name(left: &str, right: &str) -> bool {
    left == right
}

/// A command that will run `program`, in a directory that certainly
/// exists.
fn command_from((program, args): (String, Vec<String>)) -> LaunchCommand {
    LaunchCommand {
        program,
        args,
        working_directory: std::env::temp_dir(),
        environment: Vec::new(),
    }
}

/// Runs `future` to completion, parking between polls.
///
/// The launcher waits for the child on a worker thread, so its future is
/// legitimately pending until that thread reports back.
fn finish<F: Future>(future: F) -> F::Output {
    let waker = crate::process::current_thread_waker();
    let mut context = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);

    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}

fn output_of(command: &LaunchCommand) -> String {
    let output = command_for(command).output().expect("child should start");
    assert!(output.status.success(), "child failed: {output:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn environment_names(dump: &str) -> Vec<&str> {
    dump.lines()
        .filter_map(|line| line.split_once('=').map(|(name, _)| name))
        .collect()
}

pub(crate) struct TempDir(PathBuf);

impl TempDir {
    pub(crate) fn new() -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("axlauncher-launch-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&path).expect("temp directory");
        Self(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_successful_command_returns_ok() {
    let temp = TempDir::new();
    let mut command = command_from(exit_script(SUCCESS_CODE));
    command.working_directory = temp.path().to_path_buf();
    let result = finish(ProcessLauncher.launch(command));

    assert_eq!(result, Ok(()));
}

#[test]
fn a_non_zero_exit_reports_its_code() {
    let temp = TempDir::new();
    let mut command = command_from(exit_script(FAILURE_CODE));
    command.working_directory = temp.path().to_path_buf();
    let result = finish(ProcessLauncher.launch(command));

    assert!(matches!(
        result,
        Err(LaunchError::ExitedWithLog {
            code: Some(FAILURE_CODE),
            ..
        })
    ));
}

#[test]
fn a_missing_program_is_not_found() {
    let temp = TempDir::new();
    let mut command = command_from(("axlauncher-no-such-program".to_owned(), Vec::new()));
    command.working_directory = temp.path().to_path_buf();

    let result = finish(ProcessLauncher.launch(command));

    assert!(matches!(result, Err(LaunchError::NotFound(_))));
}

#[test]
fn environment_entries_reach_the_child() {
    let mut command = command_from(echo_variable(GIVEN_NAME));
    command
        .environment
        .push((GIVEN_NAME.to_owned(), Some(GIVEN_VALUE.to_owned())));

    assert_eq!(output_of(&command), GIVEN_VALUE);
}

#[test]
fn an_explicit_curseforge_key_override_does_not_reach_the_game() {
    use axlauncher_spi::credentials::CURSEFORGE_API_KEY;

    let mut command = command_from(echo_variable(CURSEFORGE_API_KEY));
    command.environment.push((
        CURSEFORGE_API_KEY.to_owned(),
        Some("synthetic-environment-probe".to_owned()),
    ));
    let child = command_for(&command);
    assert!(
        child.get_envs().any(|(name, value)| {
            name == std::ffi::OsStr::new(CURSEFORGE_API_KEY) && value.is_none()
        }),
        "the game command must remove the key after applying explicit overrides"
    );
}

#[test]
fn a_removed_environment_entry_is_absent_in_the_child() {
    assert!(
        std::env::var_os(REMOVED_NAME).is_some(),
        "test needs an inherited {REMOVED_NAME}"
    );
    let mut command = command_from(environment_dump());
    command.environment.push((REMOVED_NAME.to_owned(), None));

    let dump = output_of(&command);
    let names = environment_names(&dump);

    assert!(
        !names
            .iter()
            .any(|name| same_variable_name(name, REMOVED_NAME)),
        "{REMOVED_NAME} reached the child: {names:?}"
    );
}

#[test]
fn the_child_starts_in_the_working_directory() {
    let temp = TempDir::new();
    let mut command = command_from(print_working_directory());
    command.working_directory = temp.path().to_path_buf();

    let printed = output_of(&command);

    assert_eq!(
        std::fs::canonicalize(printed).expect("child directory"),
        std::fs::canonicalize(temp.path()).expect("temp directory")
    );
}

/// The child `command` describes, configured but not yet started.
///
/// Starting is left to the caller so a test can capture what the child prints
/// instead of only its exit status.
fn command_for(command: &LaunchCommand) -> std::process::Command {
    process::build(command)
}
