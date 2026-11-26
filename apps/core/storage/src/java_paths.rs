// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::path::PathBuf;
#[cfg(windows)]
use std::path::{Component, Prefix};

/// Conventional system, user and Mojang Java installation roots.
pub fn java_search_roots() -> Vec<PathBuf> {
    let mut roots = platform_roots();
    if let Some(home) = dirs::home_dir() {
        roots.extend([
            home.join(".jdks"),
            home.join(".sdkman/candidates/java"),
            home.join(".asdf/installs/java"),
        ]);
    }
    roots.sort();
    roots.dedup();
    roots
}

#[cfg(windows)]
fn platform_roots() -> Vec<PathBuf> {
    const VENDORS: &[&str] = &[
        "Java",
        "Eclipse Adoptium",
        "AdoptOpenJDK",
        "Microsoft",
        "Amazon Corretto",
        "Azul Systems",
        "BellSoft",
        "Semeru",
        "RedHat",
    ];
    let mut roots = Vec::new();
    for key in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(path) = std::env::var_os(key)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
        {
            roots.extend(VENDORS.iter().map(|vendor| path.join(vendor)));
            roots.push(path.join("Minecraft Launcher/runtime"));
        }
    }
    roots.extend(roaming_minecraft_runtime());
    roots
}

#[cfg(windows)]
fn roaming_minecraft_runtime() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join(".minecraft/runtime"))
}

/// Fixed, mounted drive letters available for the Windows Java search.
#[cfg(windows)]
pub fn java_fixed_drives() -> Vec<PathBuf> {
    sysinfo::Disks::new_with_refreshed_list()
        .list()
        .iter()
        .filter(|disk| !disk.is_removable())
        .map(sysinfo::Disk::mount_point)
        .filter(|path| {
            path.parent().is_none()
                && matches!(
                    path.components().next(),
                    Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_))
                )
        })
        .map(ToOwned::to_owned)
        .collect()
}

/// Locations explored for Windows Java installations by directory name.
#[cfg(windows)]
pub fn java_exploration_roots(drives: &[PathBuf]) -> Vec<PathBuf> {
    let mut roots: Vec<_> = roaming_minecraft_runtime()
        .into_iter()
        .chain(dirs::data_local_dir())
        .chain(dirs::home_dir())
        .collect();
    roots.extend(drives.iter().flat_map(|drive| {
        [
            drive.join("Program Files"),
            drive.join("Program Files (x86)"),
        ]
    }));
    roots
}

/// Minecraft Launcher's published Microsoft Store runtime location.
#[cfg(windows)]
pub fn java_store_runtime_root() -> Option<PathBuf> {
    Some(
        dirs::data_local_dir()?
            .join("Packages/Microsoft.4297127D64EC6_8wekyb3d8bbwe/LocalCache/Local/runtime"),
    )
}

/// The extra directory Windows' executable search checks before PATH.
#[cfg(windows)]
pub fn java_working_directory() -> Option<PathBuf> {
    std::env::current_dir().ok()
}

#[cfg(target_os = "macos")]
fn platform_roots() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from("/Library/Java/JavaVirtualMachines")];
    if let Some(home) = dirs::home_dir() {
        roots.extend([
            home.join("Library/Java/JavaVirtualMachines"),
            home.join("Library/Application Support/minecraft/runtime"),
        ]);
    }
    roots
}

#[cfg(target_os = "linux")]
fn platform_roots() -> Vec<PathBuf> {
    let mut roots = vec![PathBuf::from("/usr/lib/jvm"), PathBuf::from("/usr/java")];
    if let Some(home) = dirs::home_dir() {
        roots.push(home.join(".minecraft/runtime"));
    }
    roots
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn platform_roots() -> Vec<PathBuf> {
    Vec::new()
}
