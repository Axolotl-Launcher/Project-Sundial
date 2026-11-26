// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Local Java discovery and executable selection.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use axlauncher_spi::{Java, JavaMode};
use axlauncher_storage::runtime_record;

use crate::platform::{executable_in_home, managed_executable, managed_home, search_all};

mod discovery;
mod release;
mod runtime;
pub use runtime::JavaRuntime;

/// Resolves Java from registered paths and the local machine.
#[derive(Debug, Clone, Default)]
pub struct SystemJava {
    java_home: Option<PathBuf>,
    search_path: Option<OsString>,
    runtime_roots: Vec<PathBuf>,
    chosen: Vec<(u32, PathBuf)>,
    registered: Vec<PathBuf>,
    system_discovery: bool,
}

impl SystemJava {
    /// Reads the current process environment.
    pub fn from_environment() -> Self {
        Self::with_environment(
            std::env::var_os("JAVA_HOME").map(PathBuf::from),
            std::env::var_os("PATH"),
        )
    }

    /// An explicit environment keeps isolated callers independent of process settings.
    pub fn with_environment(java_home: Option<PathBuf>, search_path: Option<OsString>) -> Self {
        Self {
            java_home,
            search_path,
            ..Self::default()
        }
    }

    /// Includes completed launcher-managed installations in the search.
    pub fn with_runtime_roots(mut self, roots: Vec<PathBuf>) -> Self {
        self.runtime_roots = inventory(roots);
        self
    }

    /// A usable choice outranks discovery for its own major.
    pub fn with_chosen(mut self, chosen: Vec<(u32, PathBuf)>) -> Self {
        self.chosen = chosen;
        self
    }

    /// Includes manually registered installations in discovery and resolution.
    pub fn with_registered(mut self, paths: Vec<PathBuf>) -> Self {
        self.registered = paths;
        self
    }

    /// Includes conventional platform locations and registrations.
    pub fn with_system_discovery(mut self) -> Self {
        self.system_discovery = true;
        self
    }

    /// Lists available Java runtimes with metadata, once per executable.
    pub fn discover(&self) -> Vec<JavaRuntime> {
        let mut seen = HashSet::new();
        self.chosen
            .iter()
            .map(|(_, path)| path.clone())
            .chain(self.candidates())
            .filter_map(|path| JavaRuntime::inspect(&path))
            .filter(|runtime| seen.insert(runtime.path().to_owned()))
            .collect()
    }

    fn candidates(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.managed_candidates()
            .chain(self.environment_candidates())
            .chain(self.additional_candidates())
    }

    /// Resolves the saved choice or the newest available runtime for a major.
    pub fn default_for<'a>(
        &self,
        major: u32,
        runtimes: &'a [JavaRuntime],
    ) -> Option<&'a JavaRuntime> {
        let pinned = self
            .chosen
            .iter()
            .filter(|(chosen_major, _)| *chosen_major == major)
            .filter_map(|(_, path)| JavaRuntime::inspect(path))
            .filter(|runtime| runtime.major() == major)
            .find_map(|chosen| {
                runtimes
                    .iter()
                    .find(|runtime| runtime.major() == major && runtime.path() == chosen.path())
            });
        pinned.or_else(|| {
            // max_by keeps the last tie, so reverse the original discovery order.
            runtimes
                .iter()
                .rev()
                .filter(|runtime| runtime.major() == major)
                .max_by(|left, right| left.compare_release(right))
        })
    }

    fn managed_candidates(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.runtime_roots
            .iter()
            .filter(|root| runtime_record(root).is_file())
            .filter_map(|root| managed_executable(root, JavaMode::Console))
    }

    fn environment_candidates(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.java_home
            .as_deref()
            .and_then(|home| executable_in_home(home, JavaMode::Console))
            .into_iter()
            .chain(
                self.search_path
                    .as_deref()
                    .and_then(|path| search_all(path, JavaMode::Console))
                    .into_iter()
                    .flatten(),
            )
    }

    fn additional_candidates(&self) -> impl Iterator<Item = PathBuf> + '_ {
        self.registered.iter().cloned().chain(
            self.system_discovery
                .then_some(())
                .into_iter()
                .flat_map(|()| discovery::system_executables()),
        )
    }
}

impl Java for SystemJava {
    fn executable(&self, mode: JavaMode) -> Option<PathBuf> {
        self.environment_candidates()
            .chain(self.managed_candidates())
            .chain(self.additional_candidates())
            .find_map(|path| self.executable_at(&path, None, mode))
    }

    fn executable_for(&self, major: u32, mode: JavaMode) -> Option<PathBuf> {
        self.default_for(major, &self.discover())
            .map(|runtime| runtime.executable(mode))
    }

    fn executable_at(&self, path: &Path, major: Option<u32>, mode: JavaMode) -> Option<PathBuf> {
        let runtime = JavaRuntime::inspect(path)?;
        if major.is_some_and(|major| runtime.major() != major) {
            return None;
        }
        Some(runtime.executable(mode))
    }
}

/// The component directories installed by this launcher, in stable order.
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

#[cfg(test)]
#[path = "../../../../../tests/apps/core/launch/src/java/mod.rs"]
mod tests;
