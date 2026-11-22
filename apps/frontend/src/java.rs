// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The Java runtimes this launcher can start.
//!
//! A runtime the launcher installed is the one the game was tested against, so
//! it is searched before the environment's. A machine with no matching JDK then
//! still launches, and a machine whose JDK happens to be first on `PATH` does
//! not decide which runtime the game gets.

use std::path::{Path, PathBuf};

use axlauncher_core::launch::SystemJava;
use axlauncher_core::storage::DataDir;

/// A Java provider that also searches the runtimes under `data_root`.
pub(crate) fn managed(data_root: &Path) -> SystemJava {
    SystemJava::from_environment().with_runtime_roots(installed(data_root))
}

/// The runtime directories present under `data_root`.
pub(crate) fn installed(data_root: &Path) -> Vec<PathBuf> {
    let root = DataDir::new(data_root).java_versions();
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect()
}
