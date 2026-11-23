// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The shared cancellation boundary for saving a verified account.

use tokio::sync::watch;

use super::AccountError;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Pending,
    Cancelled,
    Saving,
}

/// Coordinates a sign-in worker with the UI that may cancel it.
#[derive(Clone)]
pub struct SignInCommit(watch::Sender<State>);

impl Default for SignInCommit {
    fn default() -> Self {
        Self(watch::Sender::new(State::Pending))
    }
}

impl SignInCommit {
    pub fn cancel(&self) {
        self.transition(State::Cancelled);
    }

    pub fn is_pending(&self) -> bool {
        *self.0.borrow() == State::Pending
    }

    pub fn is_saving(&self) -> bool {
        *self.0.borrow() == State::Saving
    }

    pub async fn cancelled(&self) {
        let _ = self
            .0
            .subscribe()
            .wait_for(|state| *state == State::Cancelled)
            .await;
    }

    pub async fn settled(&self) {
        let _ = self
            .0
            .subscribe()
            .wait_for(|state| *state != State::Pending)
            .await;
    }

    pub(super) fn begin(&self) -> Result<(), AccountError> {
        self.transition(State::Saving)
            .then_some(())
            .ok_or(AccountError::Cancelled)
    }

    fn transition(&self, next: State) -> bool {
        self.0.send_if_modified(|state| transition(state, next))
    }
}

fn transition(state: &mut State, next: State) -> bool {
    if *state != State::Pending {
        return false;
    }
    *state = next;
    true
}
