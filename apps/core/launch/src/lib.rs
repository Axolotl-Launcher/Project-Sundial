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

#[cfg(test)]
#[path = "../../../../tests/apps/core/launch/src/lib.rs"]
mod tests;
