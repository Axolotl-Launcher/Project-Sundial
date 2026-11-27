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
    let mut roots = Vec::new();
    for key in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Some(path) = std::env::var_os(key)
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
        {
            roots.push(path);
        }
    }
    roots.extend(roaming_minecraft_runtime());
    roots
}

#[cfg(windows)]
fn roaming_minecraft_runtime() -> Option<PathBuf> {
    Some(dirs::data_dir()?.join(".minecraft/runtime"))
}

/// Platform locations for one Windows Java discovery pass.
#[cfg(windows)]
pub struct WindowsJavaPaths {
    pub drives: Vec<PathBuf>,
    pub exploration_roots: Vec<PathBuf>,
    pub registry_homes: Vec<PathBuf>,
    pub working_directory: Option<PathBuf>,
    pub store_runtime_root: Option<PathBuf>,
}

#[cfg(windows)]
impl WindowsJavaPaths {
    pub fn discover() -> Self {
        let drives = fixed_drives();
        Self {
            exploration_roots: exploration_roots(&drives),
            registry_homes: registry_homes(),
            working_directory: std::env::current_dir().ok(),
            store_runtime_root: store_runtime_root(),
            drives,
        }
    }
}

#[cfg(windows)]
fn fixed_drives() -> Vec<PathBuf> {
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
fn exploration_roots(drives: &[PathBuf]) -> Vec<PathBuf> {
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
fn store_runtime_root() -> Option<PathBuf> {
    Some(
        dirs::data_local_dir()?
            .join("Packages/Microsoft.4297127D64EC6_8wekyb3d8bbwe/LocalCache/Local/runtime"),
    )
}

/// Java homes registered by Windows installers.
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
