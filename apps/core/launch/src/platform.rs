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
pub(crate) fn executable_in(directory: &Path, mode: JavaMode) -> Option<PathBuf> {
    let candidate = directory.join(name(mode));
    candidate.is_file().then_some(candidate)
}

/// The Java executable for `mode` inside a JDK home, which keeps it in `bin/`.
pub(crate) fn executable_in_home(home: &Path, mode: JavaMode) -> Option<PathBuf> {
    executable_in(&home.join("bin"), mode)
}

/// The first Java executable for `mode` on a `PATH`-shaped list of directories.
pub(crate) fn search(search_path: &OsStr, mode: JavaMode) -> Option<PathBuf> {
    std::env::split_paths(search_path).find_map(|directory| executable_in(&directory, mode))
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
pub(crate) fn executable_name(mode: JavaMode) -> &'static str {
    name(mode)
}
