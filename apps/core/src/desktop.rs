// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Desktop application services and their wiring ports.

use std::future::Future;

pub mod catalog;
pub mod installation;
pub mod instance_settings;
pub mod java;
pub mod launching;
pub mod project_images;
pub mod sources;

pub mod accounts {
    pub use axlauncher_worker::DesktopAccounts;
    pub use axlauncher_worker::accounts::{
        Account, AccountId, AccountKey, AccountSnapshot, Action, Change, ChoiceId, LauncherAccount,
        LauncherId, Method, Persistence, Profile, Reply, SignInCommit,
    };
}

pub mod downloads {
    pub use axlauncher_worker::downloads::{Downloads, Route as DownloadRoute, stats};
}

/// A desktop worker runs one operation with its own Tokio I/O driver.
fn on_worker<T>(operation: impl Future<Output = T>) -> std::io::Result<T> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    Ok(runtime.block_on(operation))
}
