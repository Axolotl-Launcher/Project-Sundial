// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Windows Java discovery from installation locations and bounded exploration.

use std::collections::{HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};

use axlauncher_spi::JavaMode;
use axlauncher_storage::{
    java_exploration_roots, java_fixed_drives, java_store_runtime_root, java_working_directory,
};

use crate::platform::{executable_in, executable_in_home};

const EXPLORATION_DEPTH: u8 = 6;
const MAX_VERSION_DIRECTORY_LENGTH: usize = 20;
const EXCLUDED_FOLDERS: &[&str] = &[
    "javapath",
    "java8path",
    "common files",
    "netease",
    "wpsystem",
];
const INSTALLATION_KEYWORDS: &[&str] = &[
    "java",
    "jdk",
    "jre",
    "dragonwell",
    "azul",
    "zulu",
    "oracle",
    "open",
    "amazon",
    "corretto",
    "eclipse",
    "temurin",
    "hotspot",
    "semeru",
    "kona",
    "bellsoft",
];
const EXPLORATION_KEYWORDS: &[&str] = &[
    "environment",
    "env",
    "runtime",
    "x86_64",
    "amd64",
    "arm64",
    "x64",
    "pcl",
    "hmcl",
    "baka",
    "minecraft",
];

pub(super) fn executables() -> Vec<PathBuf> {
    let drives = java_fixed_drives();
    let roots = java_exploration_roots(&drives).into_iter().chain(
        drives
            .iter()
            .flat_map(|drive| child_directories(drive).filter(|path| is_installation_root(path))),
    );
    let mut found = explore(roots);
    found.extend(
        registry_homes()
            .iter()
            .filter_map(|home| executable_in_home(home, JavaMode::Console)),
    );
    found.extend(java_working_directory().and_then(|root| executable_in(&root, JavaMode::Console)));
    if let Some(root) = java_store_runtime_root() {
        found.extend(store_executables(&root));
    }
    found
}

pub(super) fn ignored(path: &Path) -> bool {
    path.components()
        .filter_map(|part| part.as_os_str().to_str())
        .any(|name| {
            EXCLUDED_FOLDERS
                .iter()
                .any(|item| name.eq_ignore_ascii_case(item))
        })
}

fn is_installation_root(path: &Path) -> bool {
    directory_name(path).is_some_and(|name| {
        INSTALLATION_KEYWORDS
            .iter()
            .any(|keyword| name.contains(keyword))
    })
}

fn explore(roots: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut queue: VecDeque<_> = roots.into_iter().map(|root| (root, 0)).collect();
    let mut visited = HashSet::new();
    let mut found = Vec::new();
    while let Some((path, depth)) = queue.pop_front() {
        if ignored(&path) {
            continue;
        }
        let Ok(directory) = dunce::canonicalize(path) else {
            continue;
        };
        if ignored(&directory) || !visited.insert(directory.clone()) {
            continue;
        }
        scan_children(&directory, depth, &mut queue, &mut found);
    }
    found
}

fn scan_children(
    directory: &Path,
    depth: u8,
    queue: &mut VecDeque<(PathBuf, u8)>,
    found: &mut Vec<PathBuf>,
) {
    for child in child_directories(directory).filter(|path| !ignored(path)) {
        if let Some(path) = executable_in(&child, JavaMode::Console) {
            found.push(path);
        } else if depth < EXPLORATION_DEPTH
            && directory_name(&child).is_some_and(|name| may_descend(&name))
        {
            queue.push_back((child, depth + 1));
        }
    }
}

fn child_directories(root: &Path) -> impl Iterator<Item = PathBuf> + use<> {
    fs::read_dir(root)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
}

fn directory_name(path: &Path) -> Option<String> {
    Some(path.file_name()?.to_str()?.to_ascii_lowercase())
}

fn may_descend(name: &str) -> bool {
    if EXCLUDED_FOLDERS
        .iter()
        .any(|excluded| name.contains(excluded))
    {
        return false;
    }
    INSTALLATION_KEYWORDS
        .iter()
        .chain(EXPLORATION_KEYWORDS)
        .any(|keyword| name.contains(keyword))
        || version_directory(name)
}

fn version_directory(name: &str) -> bool {
    name.len() <= MAX_VERSION_DIRECTORY_LENGTH
        && name.bytes().any(|byte| byte.is_ascii_digit())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-'))
}

fn store_executables(root: &Path) -> Vec<PathBuf> {
    let Ok(root) = dunce::canonicalize(root) else {
        return Vec::new();
    };
    if ignored(&root) {
        return Vec::new();
    }
    child_directories(&root)
        .filter(|path| directory_name(path).is_some_and(|name| name.starts_with("java-runtime")))
        .flat_map(|component| child_directories(&component))
        .flat_map(|architecture| child_directories(&architecture))
        .filter_map(|home| executable_in_home(&home, JavaMode::Console))
        .collect()
}

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

fn registry_installations(branch: &windows_registry::Key, value: &str) -> Vec<PathBuf> {
    let Ok(keys) = branch.keys() else {
        return Vec::new();
    };
    keys.filter_map(|key| branch.open(key).ok()?.get_string(value).ok())
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .collect()
}

#[cfg(test)]
#[path = "../../../../../../tests/apps/core/launch/src/java/discovery/windows.rs"]
mod tests;
