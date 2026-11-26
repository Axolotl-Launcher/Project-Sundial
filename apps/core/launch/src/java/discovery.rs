// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axlauncher_spi::JavaMode;
use axlauncher_storage::java_search_roots;

use crate::platform::{bundle_home, executable_in_home, managed_executable};

use super::runtime::JavaRuntime;

#[cfg(windows)]
mod windows;

// Mojang uses a component/platform/component layout under its runtime root.
const SEARCH_DEPTH: u8 = 3;
const SEARCH_MAX_AGE: Duration = Duration::from_secs(60);

struct SystemSearch {
    scanned: Instant,
    paths: Arc<[PathBuf]>,
}

/// Shares system search results across launch jobs. Callers still inspect the files.
pub(super) fn system_candidates(refresh: bool) -> Arc<[PathBuf]> {
    static SEARCH: Mutex<Option<SystemSearch>> = Mutex::new(None);
    let mut search = SEARCH.lock().expect("system Java discovery lock");
    search_paths(&mut search, refresh, SEARCH_MAX_AGE, system_executables)
}

fn search_paths(
    search: &mut Option<SystemSearch>,
    refresh: bool,
    max_age: Duration,
    scan: impl FnOnce() -> Vec<PathBuf>,
) -> Arc<[PathBuf]> {
    if !refresh
        && let Some(previous) = search.as_ref()
        && previous.scanned.elapsed() < max_age
    {
        return Arc::clone(&previous.paths);
    }
    let paths: Arc<[_]> = scan().into();
    *search = Some(SystemSearch {
        scanned: Instant::now(),
        paths: Arc::clone(&paths),
    });
    paths
}

fn system_executables() -> Vec<PathBuf> {
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
pub(super) fn inspect_automatic(path: &Path) -> Option<JavaRuntime> {
    #[cfg(windows)]
    if windows::ignored(path) {
        return None;
    }
    let runtime = JavaRuntime::inspect(path)?;
    #[cfg(windows)]
    if windows::ignored(runtime.path()) {
        return None;
    }
    Some(runtime)
}

pub(super) fn scan(root: &Path, depth: u8, found: &mut Vec<PathBuf>) {
    #[cfg(windows)]
    if windows::ignored(root) || dunce::canonicalize(root).is_ok_and(|path| windows::ignored(&path))
    {
        return;
    }
    let candidates = [
        managed_executable(root, JavaMode::Console),
        executable_in_home(&bundle_home(root), JavaMode::Console),
    ];
    if let Some(path) = candidates
        .into_iter()
        .flatten()
        .find(|path| inspect_automatic(path).is_some())
    {
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

#[cfg(test)]
#[path = "../../../../../tests/apps/core/launch/src/java/discovery.rs"]
mod tests;
