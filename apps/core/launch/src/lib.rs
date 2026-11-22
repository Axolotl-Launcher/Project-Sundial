// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Launching: arguments, Java discovery and process lifecycle.

pub mod arguments;
pub mod chain;
pub mod classpath;
pub mod flow;
mod jar;
mod java;
pub mod natives;
pub mod plan;
mod platform;
mod process;
pub mod rules;
pub mod version;

pub use arguments::Values;
pub use chain::{ChainError, merge, resolve};
pub use flow::{FlowError, LaunchOutcome, LaunchRequest};
pub use jar::JarReader;
pub use java::SystemJava;
pub use plan::{Layout, Session};
pub use rules::Target;
pub use version::Version;

use axlauncher_spi::{LaunchCommand, LaunchError, Launcher};

/// Starts the command as a child process and waits for it.
#[derive(Debug, Default)]
pub struct ProcessLauncher;

impl Launcher for ProcessLauncher {
    async fn launch(&self, command: LaunchCommand) -> Result<(), LaunchError> {
        process::run(command).await
    }
}

/// The child `command` describes, configured but not yet started.
///
/// Starting is left to the caller so a test can capture what the child prints
/// instead of only its exit status.
#[cfg(test)]
fn command_for(command: &LaunchCommand) -> std::process::Command {
    process::build(command)
}

#[cfg(test)]
mod tests {
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

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("axlauncher-launch-{}-{unique}", std::process::id()));
            std::fs::create_dir_all(&path).expect("temp directory");
            Self(path)
        }

        fn path(&self) -> &Path {
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
        let result = finish(ProcessLauncher.launch(command_from(exit_script(SUCCESS_CODE))));

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn a_non_zero_exit_reports_its_code() {
        let result = finish(ProcessLauncher.launch(command_from(exit_script(FAILURE_CODE))));

        assert_eq!(
            result,
            Err(LaunchError::Exited {
                code: Some(FAILURE_CODE)
            })
        );
    }

    #[test]
    fn a_missing_program_is_not_found() {
        let command = command_from(("axlauncher-no-such-program".to_owned(), Vec::new()));

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
}
