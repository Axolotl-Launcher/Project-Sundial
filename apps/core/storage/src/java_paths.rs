// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::path::PathBuf;

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
    if let Some(data) = dirs::data_dir() {
        roots.push(data.join(".minecraft/runtime"));
    }
    if let Some(local) = dirs::data_local_dir() {
        roots.push(
            local.join("Packages/Microsoft.4297127D64EC6_8wekyb3d8bbwe/LocalCache/Local/runtime"),
        );
    }
    roots
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
