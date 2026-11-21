// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! The parts of launching that differ by operating system.
//!
//! Everything OS-conditional in this crate lives here, so the shared code
//! never branches on the target.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use superlogical_spi::JavaMode;

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

#[cfg(test)]
pub(crate) fn executable_name(mode: JavaMode) -> &'static str {
    name(mode)
}
