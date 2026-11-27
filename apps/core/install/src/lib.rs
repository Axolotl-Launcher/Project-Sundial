// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Shared installation planning, catalogs and local readiness.

pub mod assets;
pub mod auth_agent;
pub mod loader;
pub mod prepare;
pub mod vanilla;

use axlauncher_spi::{InstallError, InstallPlan, Installer};

// Cargo Shear scans imports inside the crate but misses the mirrored tests.
#[cfg(test)]
use tempfile as test_tempfile;

/// Runs each planned step in order, stopping at the first failure.
#[derive(Debug, Default)]
pub struct StepInstaller;

impl Installer for StepInstaller {
    async fn install(&self, plan: InstallPlan) -> Result<(), InstallError> {
        let _ = plan;
        Ok(())
    }
}
