// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The shared cancellation boundary for saving a verified account.

use axlauncher_worker::Cancellation;
use std::rc::Rc;

use super::AccountError;

#[derive(Clone, Default)]
pub struct SignInCommit {
    cancellation: Cancellation,
    on_saving: Option<Rc<dyn Fn()>>,
}

impl SignInCommit {
    pub(crate) fn reporting(cancellation: Cancellation, on_saving: impl Fn() + 'static) -> Self {
        Self {
            cancellation,
            on_saving: Some(Rc::new(on_saving)),
        }
    }

    #[cfg(test)]
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }

    pub fn is_pending(&self) -> bool {
        self.cancellation.is_pending()
    }

    pub async fn cancelled(&self) {
        self.cancellation.cancelled().await;
    }

    pub(super) fn begin(&self) -> Result<(), AccountError> {
        if !self.cancellation.begin_commit() {
            return Err(AccountError::Cancelled);
        }
        if let Some(on_saving) = &self.on_saving {
            on_saving();
        }
        Ok(())
    }
}
