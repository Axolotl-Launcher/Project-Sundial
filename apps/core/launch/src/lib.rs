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
        process::run(command, || {}).await
    }
}

impl ProcessLauncher {
    /// A process provider that reports when the child has actually started.
    pub fn reporting(started: impl Fn() + Send + Sync + 'static) -> impl Launcher + Send + Sync {
        ReportingProcessLauncher(std::sync::Arc::new(started))
    }
}

struct ReportingProcessLauncher<F>(std::sync::Arc<F>);

impl<F: Fn() + Send + Sync + 'static> Launcher for ReportingProcessLauncher<F> {
    async fn launch(&self, command: LaunchCommand) -> Result<(), LaunchError> {
        let started = self.0.clone();
        process::run(command, move || started()).await
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/launch/src/lib.rs"]
mod tests;
