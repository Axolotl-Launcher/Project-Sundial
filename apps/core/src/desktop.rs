// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The desktop wiring port and its placeholder provider.

pub use axlauncher_spi::desktop::{
    DesktopBackend, DesktopOperation, DesktopOperationError, operations,
};

/// A provider for wiring UI paths before their behavior is implemented.
///
/// ```
/// use axlauncher_core::desktop::{DesktopBackend, PendingDesktopBackend, operations};
///
/// async fn list_instances(backend: &impl DesktopBackend) {
///     let result = backend.request(operations::instance::LIST).await;
///     assert!(result.is_err());
/// }
///
/// let backend = PendingDesktopBackend;
/// let _request = list_instances(&backend);
/// ```
#[derive(Debug, Default)]
pub struct PendingDesktopBackend;

impl DesktopBackend for PendingDesktopBackend {
    async fn request(&self, operation: DesktopOperation) -> Result<(), DesktopOperationError> {
        Err(DesktopOperationError::NotImplemented { operation })
    }
}
