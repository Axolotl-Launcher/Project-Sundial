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

use crate::platform::{executable_in_home, managed_executable, managed_home, search_all, sibling};

/// Java found through a configured path, `JAVA_HOME`, then the search path.
#[derive(Debug, Clone, Default)]
pub struct SystemJava {
    java_home: Option<PathBuf>,
    search_path: Option<OsString>,
    /// Runtime directories this launcher installed, searched before the
    /// system's own: what the launcher fetched is the runtime the game was
    /// tested against.
    runtime_roots: Vec<PathBuf>,
    /// The runtime the user recorded for each major version, outranking
    /// discovery whenever the recorded file is still there.
    chosen: Vec<(u32, PathBuf)>,
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
            runtime_roots: Vec::new(),
            chosen: Vec::new(),
        }
    }

    /// Installed runtime directories, consulted when a major is known.
    ///
    /// They come before the environment's Java in that search: what the
    /// launcher fetched for that major is the runtime the game was tested
    /// against.
    pub fn with_runtime_roots(mut self, roots: Vec<PathBuf>) -> Self {
        self.runtime_roots = inventory(roots);
        self
    }

    /// The runtime the user recorded for each major version.
    ///
    /// A choice outranks every other source for its own major: the user named
    /// the runtime, and discovery cannot know it is wrong. The entry is skipped
    /// when its file is gone, so an uninstalled runtime leaves the game
    /// launchable on whatever else the machine offers.
    pub fn with_chosen(mut self, chosen: Vec<(u32, PathBuf)>) -> Self {
        self.chosen = chosen;
        self
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
        // A document that states no major gets the user's own default: the
        // managed runtimes are only an answer when a major is known, because
        // which of them fits is exactly the question this call cannot ask.
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
        // The user named this runtime for this major. Nothing discovery finds
        // can be a better answer, so the recorded path decides whenever it is
        // still on disk. A path that has since been removed must not fail the
        // launch: the search below still has the rest of the machine to offer.
        if let Some(chosen) = self.chosen_executable(major) {
            return Some(sibling(&chosen, mode).unwrap_or(chosen));
        }

        let candidates = self
            .runtime_roots
            .iter()
            .filter_map(|root| managed_executable(root, JavaMode::Console))
            .chain(
                self.java_home
                    .as_deref()
                    .and_then(|home| executable_in_home(home, JavaMode::Console)),
            )
            .chain(self.search_everywhere(JavaMode::Console));

        candidates
            .filter(|path| runtime_major(path) == Some(major))
            .map(|path| sibling(&path, mode).unwrap_or(path))
            .next()
    }
}

impl SystemJava {
    /// The recorded runtime for `major`, when its file is still there.
    fn chosen_executable(&self, major: u32) -> Option<PathBuf> {
        self.chosen
            .iter()
            .find(|(chosen_major, path)| *chosen_major == major && path.is_file())
            .map(|(_, path)| path.clone())
    }

    /// Every console executable the search path offers, in order.
    fn search_everywhere(&self, mode: JavaMode) -> impl Iterator<Item = PathBuf> + '_ {
        self.search_path
            .as_deref()
            .and_then(|path| search_all(path, mode))
            .into_iter()
            .flatten()
    }
}

/// The runtime directories under one of `roots`, in a stable order.
///
/// A `roots` entry may be a component directory, as the installer names it, or
/// the directory holding them. Listing happens here, in the crate that owns Java
/// discovery, so a caller only names the location. The order is stable so that
/// two runtimes claiming the same major are resolved the same way every launch.
fn inventory(roots: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for root in roots {
        if managed_home(&root).join("bin").is_dir() {
            found.push(root);
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        found.extend(
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir() && managed_home(path).join("bin").is_dir()),
        );
    }
    found.sort();
    found.dedup();
    found
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
