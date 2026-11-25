// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The desktop operation inventory. Executable workflows use typed services.

use std::fmt;

pub mod operations;

/// One known operation in the desktop wiring inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DesktopOperation {
    group: &'static str,
    name: &'static str,
}

impl DesktopOperation {
    pub const fn group(self) -> &'static str {
        self.group
    }

    pub const fn name(self) -> &'static str {
        self.name
    }
}

impl fmt::Display for DesktopOperation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.group, self.name)
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/spi/src/desktop.rs"]
mod tests;
