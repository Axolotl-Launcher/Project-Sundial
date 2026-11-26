// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::fs;
use std::path::{Path, PathBuf};

use axlauncher_spi::JavaMode;
use axlauncher_storage::java_search_roots;

use crate::platform::{bundle_home, executable_in_home, managed_executable};

// Mojang uses a component/platform/component layout under its runtime root.
const SEARCH_DEPTH: u8 = 3;

pub(super) fn system_executables() -> Vec<PathBuf> {
    let mut found = Vec::new();
    for root in java_search_roots() {
        scan(&root, SEARCH_DEPTH, &mut found);
    }
    for home in registry_homes() {
        found.extend(executable_in_home(&home, JavaMode::Console));
    }
    found.sort();
    found.dedup();
    found
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

#[cfg(windows)]
fn registry_homes() -> Vec<PathBuf> {
    use windows_registry::{CURRENT_USER, LOCAL_MACHINE};

    [LOCAL_MACHINE, CURRENT_USER]
        .into_iter()
        .flat_map(|hive| {
            [r"SOFTWARE", r"SOFTWARE\WOW6432Node"]
                .into_iter()
                .filter_map(move |prefix| hive.open(prefix).ok())
        })
        .flat_map(|software| registry_vendor_homes(&software))
        .collect()
}

#[cfg(windows)]
fn registry_vendor_homes(software: &windows_registry::Key) -> Vec<PathBuf> {
    const REGISTRY_KEYS: &[(&str, &str)] = &[
        (r"JavaSoft\JDK", "JavaHome"),
        (r"JavaSoft\JRE", "JavaHome"),
        (r"JavaSoft\Java Development Kit", "JavaHome"),
        (r"JavaSoft\Java Runtime Environment", "JavaHome"),
        (r"Azul Systems\Zulu", "InstallationPath"),
        (r"BellSoft\Liberica", "InstallationPath"),
    ];
    REGISTRY_KEYS
        .iter()
        .filter_map(|(key, value)| Some((software.open(key).ok()?, value)))
        .flat_map(|(branch, value)| registry_installations(&branch, value))
        .collect()
}

#[cfg(windows)]
fn registry_installations(branch: &windows_registry::Key, value: &str) -> Vec<PathBuf> {
    let Ok(keys) = branch.keys() else {
        return Vec::new();
    };
    keys.filter_map(|key| branch.open(key).ok()?.get_string(value).ok())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .collect()
}

#[cfg(not(windows))]
fn registry_homes() -> Vec<PathBuf> {
    Vec::new()
}
