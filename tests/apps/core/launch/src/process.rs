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
        block_on(run(exit_command(0, directory.path()), move || sender
            .send(())
            .unwrap())),
        Ok(())
    );
    assert_eq!(started.try_recv(), Ok(()));
}

#[test]
fn a_non_zero_exit_reports_its_code() {
    let directory = TempDir::new();
    assert_eq!(
        block_on(run(exit_command(7, directory.path()), || {})),
        Err(LaunchError::Exited { code: Some(7) })
    );
}

#[test]
fn a_missing_program_is_not_found() {
    let directory = TempDir::new();
    let command = LaunchCommand {
        program: PathBuf::from("surely-not-a-program-here")
            .display()
            .to_string(),
        args: Vec::new(),
        working_directory: directory.path().to_path_buf(),
        environment: Vec::new(),
    };

    assert!(matches!(
        block_on(run(command, || panic!("missing program did not start"))),
        Err(LaunchError::NotFound(_))
    ));
}

#[test]
fn launch_captures_both_process_streams_without_changing_game_logs() {
    let directory = TempDir::new();
    let log = axlauncher_storage::instance_console_log(directory.path());
    let game_log = directory.path().join("logs/latest.log");
    std::fs::create_dir(game_log.parent().unwrap()).unwrap();
    std::fs::write(&game_log, "existing game log").unwrap();
    std::fs::write(&log, "previous process output").unwrap();

    let mut command = exit_command(0, directory.path());
    #[cfg(windows)]
    let script = "echo game-stdout & echo jvm-stderr 1>&2 & exit 7";
    #[cfg(not(windows))]
    let script = "echo game-stdout; echo jvm-stderr >&2; exit 7";
    command.args[1] = script.to_owned();
    assert_eq!(
        block_on(run(command, || {})),
        Err(LaunchError::Exited { code: Some(7) })
    );

    let captured = std::fs::read_to_string(log).unwrap();
    assert_eq!(
        captured.lines().map(str::trim).collect::<Vec<_>>(),
        ["game-stdout", "jvm-stderr"]
    );
    assert_eq!(
        std::fs::read_to_string(game_log).unwrap(),
        "existing game log"
    );
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
