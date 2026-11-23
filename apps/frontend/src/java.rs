// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The Java runtimes this launcher can start.
//!
//! A runtime the launcher installed is the one the game was tested against, so
//! it is searched before the environment's. A machine with no matching JDK then
//! still launches, and a machine whose JDK happens to be first on `PATH` does
//! not decide which runtime the game gets.

use std::path::Path;

use axlauncher_core::launch::SystemJava;
use axlauncher_core::storage::DataDir;

/// A Java provider that also searches the runtimes under `data_root`.
///
/// The provider owns the listing: this only names where the launcher keeps the
/// runtimes it fetched.
pub(crate) fn managed(data_root: &Path) -> SystemJava {
    SystemJava::from_environment().with_runtime_roots(vec![DataDir::new(data_root).java_versions()])
}
