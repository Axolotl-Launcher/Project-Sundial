// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Desktop application services and their wiring ports.

use std::future::Future;

pub mod catalog;
pub mod installation;
pub mod java;
pub mod launching;
pub mod sources;

/// A desktop worker runs one operation with its own Tokio I/O driver.
fn on_worker<T>(operation: impl Future<Output = T>) -> std::io::Result<T> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    Ok(runtime.block_on(operation))
}
