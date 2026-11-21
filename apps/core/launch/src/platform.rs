// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! The parts of launching that differ by operating system.
//!
//! Everything OS-conditional in this crate lives here, so the shared code
//! never branches on the target.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// Java's executable name, as this platform spells it.
#[cfg(windows)]
const JAVA: &str = "java.exe";
#[cfg(not(windows))]
const JAVA: &str = "java";

#[cfg(test)]
pub(crate) fn executable_name() -> &'static str {
    JAVA
}

/// The Java executable directly under `directory`.
pub(crate) fn executable_in(directory: &Path) -> Option<PathBuf> {
    let candidate = directory.join(JAVA);
    candidate.is_file().then_some(candidate)
}

/// The Java executable inside a JDK home, which keeps it in `bin/`.
pub(crate) fn executable_in_home(home: &Path) -> Option<PathBuf> {
    executable_in(&home.join("bin"))
}

/// The first Java executable on a `PATH`-shaped list of directories.
pub(crate) fn search(search_path: &OsStr) -> Option<PathBuf> {
    std::env::split_paths(search_path).find_map(|directory| executable_in(&directory))
}
