// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::sync::Arc;
use std::task::Wake;
use std::thread::Thread;

use super::*;
use crate::TestTempDir as TempDir;
use std::path::PathBuf;

/// Runs `future` to completion on this thread, parking between polls.
fn block_on<F: Future>(future: F) -> F::Output {
    let waker = current_thread_waker();
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);

    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}

fn exit_command(code: i32, directory: &std::path::Path) -> LaunchCommand {
    #[cfg(windows)]
    let (program, args) = (
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_owned()),
        vec!["/C".to_owned(), format!("exit {code}")],
    );
    #[cfg(not(windows))]
    let (program, args) = (
        "/bin/sh".to_owned(),
        vec!["-c".to_owned(), format!("exit {code}")],
    );

    LaunchCommand {
        program,
        args,
        working_directory: directory.to_path_buf(),
        environment: Vec::new(),
    }
}

#[test]
fn a_successful_command_finishes() {
    let directory = TempDir::new();
    let (sender, started) = std::sync::mpsc::channel();
    assert_eq!(
        block_on(run(exit_command(0, directory.path()), None, move || sender
            .send(())
            .unwrap())),
        Ok(())
    );
    assert_eq!(started.try_recv(), Ok(()));
}

#[test]
fn a_non_zero_exit_reports_its_code() {
    let directory = TempDir::new();
    assert!(matches!(
        block_on(run(exit_command(7, directory.path()), None, || {})),
        Err(LaunchError::ExitedWithLog { code: Some(7), .. })
    ));
}

#[test]
fn a_missing_program_is_not_found() {
    let directory = TempDir::new();
    let previous = axlauncher_storage::open_instance_console_log(directory.path()).unwrap();
    let command = LaunchCommand {
        program: PathBuf::from("surely-not-a-program-here")
            .display()
            .to_string(),
        args: Vec::new(),
        working_directory: directory.path().to_path_buf(),
        environment: Vec::new(),
    };

    assert!(matches!(
        block_on(run(command, None, || panic!(
            "missing program did not start"
        ))),
        Err(LaunchError::NotFound(_))
    ));
    assert_eq!(
        std::fs::read_dir(previous.path().parent().unwrap())
            .unwrap()
            .count(),
        1
    );
}

#[test]
fn launch_captures_both_process_streams_without_changing_game_logs() {
    let directory = TempDir::new();
    let previous = axlauncher_storage::open_instance_console_log(directory.path()).unwrap();
    let game_log = directory.path().join("logs/latest.log");
    std::fs::write(&game_log, "existing game log").unwrap();
    std::fs::write(previous.path(), "previous process output").unwrap();

    let mut command = exit_command(0, directory.path());
    #[cfg(windows)]
    let script = "echo game-stdout:test-secret & echo jvm-stderr:test-secret 1>&2 & exit 7";
    #[cfg(not(windows))]
    let script = "echo game-stdout:test-secret; echo jvm-stderr:test-secret >&2; exit 7";
    command.args[1] = script.to_owned();
    let first = run(command.clone(), Some("test-secret".to_owned()), || {});
    let second = run(command, Some("test-secret".to_owned()), || {});
    let first = check_capture(block_on(first));
    let second = check_capture(block_on(second));
    assert_ne!(first, second);
    assert_eq!(
        std::fs::read_to_string(previous.path()).unwrap(),
        "previous process output"
    );
    assert_eq!(
        std::fs::read_to_string(game_log).unwrap(),
        "existing game log"
    );
}

fn check_capture(result: Result<(), LaunchError>) -> PathBuf {
    let LaunchError::ExitedWithLog { code: Some(7), log } = result.unwrap_err() else {
        panic!("expected a logged exit")
    };
    let captured = std::fs::read_to_string(&log).unwrap();
    assert!(captured.contains("game-stdout:[redacted]"));
    assert!(captured.contains("jvm-stderr:[redacted]"));
    assert!(!captured.contains("test-secret"));
    log
}

#[test]
fn a_completed_slot_returns_without_waiting() {
    let slot = Slot::default();
    slot.complete(Ok(()));

    let waker = current_thread_waker();
    assert_eq!(slot.poll(&waker), Poll::Ready(Ok(())));
}

#[test]
fn an_empty_slot_registers_the_waker() {
    let slot = Slot::default();
    let waker = current_thread_waker();

    assert!(slot.poll(&waker).is_pending());
    slot.complete(Ok(()));
    assert_eq!(slot.poll(&waker), Poll::Ready(Ok(())));
}

/// A waker that unparks the thread it was built on.
struct ThreadWaker(Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// A waker for the current thread, for callers that block on a future.
pub(crate) fn current_thread_waker() -> Waker {
    Waker::from(Arc::new(ThreadWaker(std::thread::current())))
}
