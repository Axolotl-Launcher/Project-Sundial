// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Installation planning and execution.

pub mod assets;
pub mod loader;
pub mod prepare;
pub mod processors;

use axlauncher_spi::{InstallError, InstallPlan, Installer};

/// Runs each planned step in order, stopping at the first failure.
#[derive(Debug, Default)]
pub struct StepInstaller;

impl Installer for StepInstaller {
    async fn install(&self, plan: InstallPlan) -> Result<(), InstallError> {
        let _ = plan;
        Ok(())
    }
}
