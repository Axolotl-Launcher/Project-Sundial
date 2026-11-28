// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The parts of launching that differ by operating system.
//!
//! Everything OS-conditional in this crate lives here, so the shared code
//! never branches on the target.

use std::ffi::OsStr;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use axlauncher_spi::JavaMode;

/// Win32's `CREATE_NO_WINDOW` process creation flag, whose value is given in
/// the Process Creation Flags reference (WinBase.h).
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Java's console executable, as this platform spells it.
#[cfg(windows)]
const CONSOLE: &str = "java.exe";
#[cfg(not(windows))]
const CONSOLE: &str = "java";

/// Java's windowless executable. Only Windows has a second one.
#[cfg(windows)]
const WINDOWED: &str = "javaw.exe";
#[cfg(not(windows))]
const WINDOWED: &str = CONSOLE;

/// The file name Java uses for `mode` on this platform.
pub(crate) fn name(mode: JavaMode) -> &'static str {
    match mode {
        JavaMode::Windowed => WINDOWED,
        JavaMode::Console => CONSOLE,
    }
}

/// The Java executable for `mode` directly under `directory`.
pub fn executable_in(directory: &Path, mode: JavaMode) -> Option<PathBuf> {
    let candidate = directory.join(name(mode));
    candidate.is_file().then_some(candidate)
}

/// The Java executable for `mode` inside a JDK home, which keeps it in `bin/`.
pub fn executable_in_home(home: &Path, mode: JavaMode) -> Option<PathBuf> {
    executable_in(&java_bin(home), mode)
}

/// The directory containing executables in a Java installation.
pub(crate) fn java_bin(home: &Path) -> PathBuf {
    home.join("bin")
}

/// The JDK home a launcher-managed runtime directory holds.
///
/// A macOS runtime is a bundle, so its home sits below `jre.bundle`; elsewhere
/// the component directory is the home. Discovery and installation must agree
/// on this, or a runtime this launcher fetched is never found again.
pub(crate) fn managed_home(root: &Path) -> PathBuf {
    let bundle = bundle_home(&root.join("jre.bundle"));
    if bundle.is_dir() {
        return bundle;
    }
    root.to_path_buf()
}

/// The Java home inside a macOS bundle.
pub fn bundle_home(root: &Path) -> PathBuf {
    root.join("Contents").join("Home")
}

/// The Java executable in a launcher-managed runtime directory.
pub fn managed_executable(root: &Path, mode: JavaMode) -> Option<PathBuf> {
    executable_in_home(&managed_home(root), mode)
}

#[cfg(windows)]
const EXCLUDED_JAVA_DIRECTORIES: &[&str] = &[
    "javapath",
    "java8path",
    "common files",
    "netease",
    "wpsystem",
];

/// Rejects Windows paths that pass through an excluded directory.
pub fn excluded_from_discovery(path: &Path) -> bool {
    #[cfg(windows)]
    return path
        .components()
        .filter_map(|part| part.as_os_str().to_str())
        .any(|name| {
            EXCLUDED_JAVA_DIRECTORIES
                .iter()
                .any(|excluded| name.eq_ignore_ascii_case(excluded))
        });

    #[cfg(not(windows))]
    {
        let _ = path;
        false
    }
}

/// Rejects exploratory Windows directory names containing an exclusion keyword.
#[cfg(windows)]
pub fn contains_excluded_java_directory(lowercase_name: &str) -> bool {
    EXCLUDED_JAVA_DIRECTORIES
        .iter()
        .any(|excluded| lowercase_name.contains(excluded))
}

/// Every Java executable for `mode` on a `PATH`-shaped list, in path order.
///
/// A `PATH` that holds several JDKs offers several answers, and a caller that
/// needs one matching a major version must be able to see them all.
pub(crate) fn search_all(search_path: &OsStr, mode: JavaMode) -> Option<Vec<PathBuf>> {
    let found: Vec<PathBuf> = std::env::split_paths(search_path)
        .filter_map(|directory| executable_in(&directory, mode))
        .collect();
    (!found.is_empty()).then_some(found)
}

/// `path` with its file name replaced by the one `mode` uses, when that file
/// exists; otherwise `path` itself.
///
/// A JDK on Windows ships both executables next to each other, so a caller can
/// ask for the other one without searching again.
pub(crate) fn sibling(path: &Path, mode: JavaMode) -> Option<PathBuf> {
    let candidate = path.with_file_name(name(mode));
    candidate.is_file().then_some(candidate)
}

/// Suppresses the console window a freshly started child would otherwise open.
///
/// Only Windows has such a window; elsewhere the child shares the terminal's
/// console, so there is nothing to turn off.
pub(crate) fn hide_console(command: &mut Command) {
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    #[cfg(not(windows))]
    let _ = command;
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/launch/src/platform.rs"]
mod test_support;
#[cfg(test)]
pub(crate) use test_support::executable_name;
