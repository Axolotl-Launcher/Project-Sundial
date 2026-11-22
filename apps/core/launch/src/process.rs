// SPDX-FileCopyrightText: 2026 Axolotl Team
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
use std::task::{Context, Poll, Waker};

use axlauncher_spi::{LaunchCommand, LaunchError};

use crate::platform;

/// Starts `command` and waits for it on a worker thread.
pub(crate) fn run(
    command: LaunchCommand,
    started: impl FnOnce() + Send + 'static,
) -> impl Future<Output = Result<(), LaunchError>> {
    let slot = Arc::new(Slot::default());

    std::thread::spawn({
        let slot = Arc::clone(&slot);
        move || {
            let outcome = wait(&command, started);
            slot.complete(outcome);
        }
    });

    Awaiting { slot }
}

fn wait(command: &LaunchCommand, started: impl FnOnce()) -> Result<(), LaunchError> {
    let mut child = build(command)
        .spawn()
        .map_err(|error| LaunchError::NotFound(error.to_string()))?;
    started();
    let status = child
        .wait()
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

#[cfg(test)]
#[path = "../../../../tests/apps/core/launch/src/process.rs"]
mod tests;
#[cfg(test)]
pub(crate) use tests::current_thread_waker;
