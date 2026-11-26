// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::fs;
use std::path::{Path, PathBuf};

use axlauncher_spi::JavaMode;
use axlauncher_storage::java_search_roots;

use crate::platform::{bundle_home, executable_in_home, managed_executable};

use super::runtime::JavaRuntime;

#[cfg(windows)]
mod windows;

// Mojang uses a component/platform/component layout under its runtime root.
const SEARCH_DEPTH: u8 = 3;

pub(super) fn system_executables() -> Vec<PathBuf> {
    let mut found = Vec::new();
    for root in java_search_roots() {
        scan(&root, SEARCH_DEPTH, &mut found);
    }
    #[cfg(windows)]
    found.extend(windows::executables());
    found.sort();
    found.dedup();
    found
}

/// Inspects an automatic candidate and checks both its reported and resolved path.
pub(super) fn inspect_automatic(path: PathBuf) -> Option<JavaRuntime> {
    #[cfg(windows)]
    if windows::ignored(&path) {
        return None;
    }
    let runtime = JavaRuntime::inspect(&path)?;
    #[cfg(windows)]
    if windows::ignored(runtime.path()) {
        return None;
    }
    Some(runtime)
}

pub(super) fn scan(root: &Path, depth: u8, found: &mut Vec<PathBuf>) {
    let executable = managed_executable(root, JavaMode::Console)
        .or_else(|| executable_in_home(&bundle_home(root), JavaMode::Console));
    if let Some(path) = executable {
        found.push(path);
        return;
    }
    if depth == 0 {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => scan(&entry.path(), depth - 1, found),
            Ok(kind) if kind.is_symlink() => scan(&entry.path(), 0, found),
            _ => {}
        }
    }
}
