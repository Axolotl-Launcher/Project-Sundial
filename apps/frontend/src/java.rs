// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The Java runtimes this launcher can start.
//!
//! A runtime the user recorded in `javas.json` decides for its own major: the
//! choice is their intent and no scan can improve on it. The runtimes this
//! launcher installed remain the fallback, because they can exist without a
//! record while a recorded runtime may have been removed since.

use std::path::{Path, PathBuf};

use axlauncher_core::launch::SystemJava;
use axlauncher_core::state::javas;
use axlauncher_core::storage::DataDir;

/// A Java provider that also searches the runtimes under `data_root`.
///
/// The provider owns the listing: this only names where the launcher keeps the
/// runtimes it fetched and what the user recorded there. A record that cannot
/// be read is no record — a broken `javas.json` must not stop the launcher.
pub(crate) fn managed(data_root: &Path) -> SystemJava {
    SystemJava::from_environment()
        .with_runtime_roots(vec![DataDir::new(data_root).java_versions()])
        .with_chosen(chosen(data_root))
}

/// The runtimes the user recorded, as the provider wants them.
pub(crate) fn chosen(data_root: &Path) -> Vec<(u32, PathBuf)> {
    let Ok(loaded) = javas::load(data_root) else {
        return Vec::new();
    };
    loaded
        .javas
        .chosen()
        .iter()
        .map(|chosen| (chosen.major_version, PathBuf::from(&chosen.path)))
        .collect()
}
