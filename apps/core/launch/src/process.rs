// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! Running a prepared command without blocking the caller's thread.
//!
//! Waiting for a child process is blocking work. Doing it inside an async task
//! would stall whatever drives that task — a UI event loop among them — for as
//! long as the game runs. The wait therefore happens on its own thread, which
//! reports back through a channel; the future polls that channel and parks the
//! waiting thread until the sender wakes it.
//!
//! No runtime is involved: a oneshot channel carries one result, and the wake
//! it performs on send is the only thing the future needs.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
#[cfg(test)]
use std::task::Wake;
use std::task::{Context, Poll, Waker};
#[cfg(test)]
use std::thread::Thread;

use superlogical_spi::{LaunchCommand, LaunchError};

use crate::platform;

/// Starts `command` and waits for it on a worker thread.
pub(crate) fn run(command: LaunchCommand) -> impl Future<Output = Result<(), LaunchError>> {
    let slot = Arc::new(Slot::default());

    std::thread::spawn({
        let slot = Arc::clone(&slot);
        move || {
            let outcome = wait(&command);
            slot.complete(outcome);
        }
    });

    Awaiting { slot }
}

fn wait(command: &LaunchCommand) -> Result<(), LaunchError> {
    let status = build(command)
        .status()
        .map_err(|error| LaunchError::NotFound(error.to_string()))?;

    if status.success() {
        Ok(())
    } else {
        Err(LaunchError::Exited {
            code: status.code(),
        })
    }
}

/// The child command a `LaunchCommand` describes.
pub(crate) fn build(command: &LaunchCommand) -> std::process::Command {
    let mut child = std::process::Command::new(&command.program);
    child
        .args(&command.args)
        .current_dir(&command.working_directory);
    for (name, value) in &command.environment {
        match value {
            Some(value) => {
                child.env(name, value);
            }
            None => {
                child.env_remove(name);
            }
        }
    }
    platform::hide_console(&mut child);
    child
}

/// The single result the worker thread hands back.
///
/// `waker` is set by the first poll and taken by the worker: whichever of the
/// two happens second wakes the other, and neither can miss the other.
#[derive(Default)]
struct Slot {
    state: Mutex<SlotState>,
}

#[derive(Default)]
struct SlotState {
    outcome: Option<Result<(), LaunchError>>,
    waker: Option<Waker>,
}

impl Slot {
    /// Stores `outcome` and wakes a waiting task, if any.
    fn complete(&self, outcome: Result<(), LaunchError>) {
        let waker = {
            let mut state = self.state.lock().expect("slot state");
            state.outcome = Some(outcome);
            state.waker.take()
        };
        if let Some(waker) = waker {
            waker.wake();
        }
    }

    /// Takes the outcome, or registers `waker` for the completion.
    fn poll(&self, waker: &Waker) -> Poll<Result<(), LaunchError>> {
        let mut state = self.state.lock().expect("slot state");
        if let Some(outcome) = state.outcome.take() {
            return Poll::Ready(outcome);
        }
        state.waker = Some(waker.clone());
        Poll::Pending
    }
}

struct Awaiting {
    slot: Arc<Slot>,
}

impl Future for Awaiting {
    type Output = Result<(), LaunchError>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        self.slot.poll(context.waker())
    }
}

/// A waker that unparks the thread it was built on.
#[cfg(test)]
struct ThreadWaker(Thread);

#[cfg(test)]
impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

/// A waker for the current thread, for callers that block on a future.
#[cfg(test)]
pub(crate) fn current_thread_waker() -> Waker {
    Waker::from(Arc::new(ThreadWaker(std::thread::current())))
}

#[cfg(test)]
mod tests {
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
        assert_eq!(block_on(run(exit_command(0))), Ok(()));
    }

    #[test]
    fn a_non_zero_exit_reports_its_code() {
        assert_eq!(
            block_on(run(exit_command(7))),
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
            block_on(run(command)),
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
}
