// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Desktop operations awaiting domain-specific request and result types.

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

/// The outcome a caller can handle while an operation awaits implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DesktopOperationError {
    NotImplemented { operation: DesktopOperation },
}

impl fmt::Display for DesktopOperationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotImplemented { operation } => {
                write!(
                    formatter,
                    "Desktop operation {operation} is not implemented"
                )
            }
        }
    }
}

impl std::error::Error for DesktopOperationError {}

/// A wiring port for desktop operations while their typed domain ports are added.
///
/// The operation is the only input. Data and progress contracts belong to the
/// owning system once an operation gains a real implementation.
pub trait DesktopBackend {
    fn request(
        &self,
        operation: DesktopOperation,
    ) -> impl Future<Output = Result<(), DesktopOperationError>>;
}
