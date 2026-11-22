// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Java discovery from the user's configuration and environment.
//!
//! A machine can hold several runtimes at once, and a version document names
//! the major version it was built for. Returning whichever runtime happens to
//! be first on `PATH` starts a modern game on an old JVM, which fails with a
//! class-file version error before the game prints anything. Discovery
//! therefore checks the major version a runtime reports and only answers for a
//! runtime that can actually load the classes.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use axlauncher_spi::{Java, JavaMode};

use crate::platform::{executable_in_home, search_all, sibling};

/// Java found through a configured path, `JAVA_HOME`, then the search path.
#[derive(Debug, Clone, Default)]
pub struct SystemJava {
    java_home: Option<PathBuf>,
    search_path: Option<OsString>,
}

impl SystemJava {
    /// Reads the current process environment.
    pub fn from_environment() -> Self {
        Self::with_environment(
            std::env::var_os("JAVA_HOME").map(PathBuf::from),
            std::env::var_os("PATH"),
        )
    }

    /// A provider with the environment passed in, so the resolution order can
    /// be tested without changing the process environment.
    pub fn with_environment(java_home: Option<PathBuf>, search_path: Option<OsString>) -> Self {
        Self {
            java_home,
            search_path,
        }
    }

    /// The configured `JAVA_HOME`, when the environment names one.
    pub fn java_home(&self) -> Option<&Path> {
        self.java_home.as_deref()
    }
}

impl Java for SystemJava {
    fn executable(&self, preferred: Option<&Path>, mode: JavaMode) -> Option<PathBuf> {
        // Discovery looks for the console executable: it is the one every JDK
        // ships. The requested mode is reached through its sibling afterwards,
        // because a JDK keeps both on Windows.
        let candidates = std::iter::once(preferred)
            .flatten()
            .filter(|path| path.is_file())
            .map(Path::to_path_buf)
            .chain(
                self.java_home
                    .as_deref()
                    .and_then(|home| executable_in_home(home, JavaMode::Console)),
            )
            .chain(
                self.search_path
                    .as_deref()
                    .and_then(|path| search_all(path, JavaMode::Console))
                    .into_iter()
                    .flatten(),
            );

        let found = candidates.into_iter().next()?;
        Some(sibling(&found, mode).unwrap_or(found))
    }

    fn executable_for(&self, major: u32, mode: JavaMode) -> Option<PathBuf> {
        let candidates = self
            .java_home
            .as_deref()
            .and_then(|home| executable_in_home(home, JavaMode::Console))
            .into_iter()
            .chain(self.search_everywhere(JavaMode::Console));

        candidates
            .filter(|path| runtime_major(path) == Some(major))
            .map(|path| sibling(&path, mode).unwrap_or(path))
            .next()
    }
}

impl SystemJava {
    /// Every console executable the search path offers, in order.
    fn search_everywhere(&self, mode: JavaMode) -> impl Iterator<Item = PathBuf> + '_ {
        self.search_path
            .as_deref()
            .and_then(|path| search_all(path, mode))
            .into_iter()
            .flatten()
    }
}

/// The major version a runtime at `executable` reports, read from the release
/// file beside it.
///
/// A JDK ships `release` next to `bin/`, and its `JAVA_VERSION` line is the
/// runtime's own answer. Reading a file is chosen over starting a JVM: the
/// answer is needed to decide *whether* to start one, and a probe costs a
/// process launch per candidate.
///
/// `java`, `javaw`, `javaws`, and `lib/jvm.cfg` are all siblings of `bin/`, so
/// the release file is found by walking up from the executable's directory.
fn runtime_major(executable: &Path) -> Option<u32> {
    let home = executable.parent()?.parent()?;
    let release = std::fs::read_to_string(home.join("release")).ok()?;

    for line in release.lines() {
        let Some(value) = line.strip_prefix("JAVA_VERSION=") else {
            continue;
        };
        let value = value.trim().trim_matches('"');
        // `21.0.1` reports 21; `1.8.0_392` reports 8. The second form is why a
        // plain split on '.' is not enough.
        return value
            .strip_prefix("1.")
            .unwrap_or(value)
            .split(['.', '_'])
            .next()
            .and_then(|major| major.parse().ok());
    }
    None
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/launch/src/java.rs"]
mod tests;
