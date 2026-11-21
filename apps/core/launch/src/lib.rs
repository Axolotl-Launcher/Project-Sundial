// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! Launching: arguments, Java discovery and process lifecycle.

use superlogical_spi::{LaunchCommand, LaunchError, Launcher};

/// Starts the command as a child process and waits for it.
#[derive(Debug, Default)]
pub struct ProcessLauncher;

impl Launcher for ProcessLauncher {
    async fn launch(&self, command: LaunchCommand) -> Result<(), LaunchError> {
        let status = std::process::Command::new(&command.program)
            .args(&command.args)
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
}
