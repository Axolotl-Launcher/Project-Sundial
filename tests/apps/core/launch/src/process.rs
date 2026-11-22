// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::sync::Arc;
use std::task::Wake;
use std::thread::Thread;

use super::*;
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

fn exit_command(code: i32) -> LaunchCommand {
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
        working_directory: std::env::temp_dir(),
        environment: Vec::new(),
    }
}

#[test]
fn a_successful_command_finishes() {
    let (sender, started) = std::sync::mpsc::channel();
    assert_eq!(
        block_on(run(exit_command(0), move || sender.send(()).unwrap())),
        Ok(())
    );
    assert_eq!(started.try_recv(), Ok(()));
}

#[test]
fn a_non_zero_exit_reports_its_code() {
    assert_eq!(
        block_on(run(exit_command(7), || {})),
        Err(LaunchError::Exited { code: Some(7) })
    );
}

#[test]
fn a_missing_program_is_not_found() {
    let command = LaunchCommand {
        program: PathBuf::from("surely-not-a-program-here")
            .display()
            .to_string(),
        args: Vec::new(),
        working_directory: std::env::temp_dir(),
        environment: Vec::new(),
    };

    assert!(matches!(
        block_on(run(command, || panic!("missing program did not start"))),
        Err(LaunchError::NotFound(_))
    ));
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
