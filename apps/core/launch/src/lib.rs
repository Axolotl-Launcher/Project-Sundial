// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Launching: arguments, Java discovery and process lifecycle.

pub mod arguments;
pub mod chain;
pub mod classpath;
pub mod flow;
mod jar;
mod java;
mod language;
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
pub use java::{
    JavaRuntime, SystemJava,
    memory::{HeapDefaults, HeapPolicy},
};
pub use language::GameLanguage;
pub use plan::{Layout, Session};
pub use rules::Target;
pub use version::Version;

pub use axlauncher_spi::{Java, JavaMode, LaunchCommand, LaunchError};

use axlauncher_spi::Launcher;

/// Starts the command as a child process and waits for it.
#[derive(Debug, Default)]
pub struct ProcessLauncher;

impl Launcher for ProcessLauncher {
    async fn launch(&self, command: LaunchCommand) -> Result<(), LaunchError> {
        process::run(command, None, || {}).await
    }

    async fn launch_redacting(
        &self,
        command: LaunchCommand,
        token: Option<&str>,
    ) -> Result<(), LaunchError> {
        process::run(command, token.map(str::to_owned), || {}).await
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
        self.launch_redacting(command, None).await
    }

    async fn launch_redacting(
        &self,
        command: LaunchCommand,
        token: Option<&str>,
    ) -> Result<(), LaunchError> {
        let started = self.0.clone();
        process::run(command, token.map(str::to_owned), move || started()).await
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/launch/src/lib.rs"]
mod tests;
#[cfg(test)]
pub(crate) use tests::TempDir as TestTempDir;
