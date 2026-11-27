// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Local Java discovery and executable selection.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use axlauncher_spi::{Java, JavaMode};
use axlauncher_storage::runtime_record;

use crate::platform::{executable_in_home, java_bin, managed_executable, managed_home, search_all};

mod application;
mod discovery;
pub(crate) mod memory;
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
    registered_only: bool,
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
        self.runtime_roots = roots;
        self
    }

    /// A usable choice outranks discovery for its own major.
    pub fn with_chosen(mut self, chosen: Vec<(u32, PathBuf)>) -> Self {
        self.chosen = chosen;
        self
    }

    /// Includes registered installations in discovery and resolution.
    pub fn with_registered(mut self, paths: Vec<PathBuf>) -> Self {
        self.registered = paths;
        self
    }

    /// Uses canonical registrations with completion checks for managed paths.
    pub fn for_registered_paths(roots: Vec<PathBuf>) -> Self {
        Self {
            runtime_roots: roots,
            registered_only: true,
            ..Self::default()
        }
    }

    /// Scans conventional platform locations in addition to configured paths.
    pub fn discover_system(&self) -> Vec<JavaRuntime> {
        self.catalog(discovery::system_executables())
    }

    /// Lists available runtimes from configured paths once per executable.
    pub fn discover(&self) -> Vec<JavaRuntime> {
        self.catalog([])
    }

    fn catalog(&self, system_paths: impl IntoIterator<Item = PathBuf>) -> Vec<JavaRuntime> {
        let mut seen = HashSet::new();
        self.chosen
            .iter()
            .filter_map(|(_, path)| self.inspect_registered(path))
            .chain(
                self.managed_candidates()
                    .chain(self.environment_candidates())
                    .filter_map(|path| self.inspect_automatic(&path)),
            )
            .chain(self.registered_runtimes())
            .chain(
                system_paths
                    .into_iter()
                    .filter_map(|path| self.inspect_automatic(&path)),
            )
            .filter(|runtime| seen.insert(runtime.path().to_owned()))
            .collect()
    }

    /// Resolves the saved choice or the newest available runtime for a major.
    fn default_for<'a>(&self, major: u32, runtimes: &'a [JavaRuntime]) -> Option<&'a JavaRuntime> {
        let pinned = self.pinned(major).and_then(|chosen| {
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
                .max_by(|left, right| left.compare_default(right))
        })
    }

    /// Inspects a selected runtime, including the completion marker for managed paths.
    pub fn inspect(&self, path: &Path) -> Option<JavaRuntime> {
        let runtime = JavaRuntime::inspect(path)?;
        self.completed(path, &runtime).then_some(runtime)
    }

    /// Rechecks an executable against the provider's registration rules.
    pub fn inspect_registered(&self, path: &Path) -> Option<JavaRuntime> {
        self.inspect(path)
            .filter(|runtime| !self.registered_only || runtime.path() == path)
    }

    fn inspect_automatic(&self, path: &Path) -> Option<JavaRuntime> {
        let runtime = discovery::inspect_automatic(path)?;
        self.completed(path, &runtime).then_some(runtime)
    }

    fn completed(&self, path: &Path, runtime: &JavaRuntime) -> bool {
        self.runtime_roots.iter().all(|root| {
            let canonical = dunce::canonicalize(root).ok();
            completed_component(root, path)
                && canonical.is_none_or(|root| completed_component(&root, runtime.path()))
        })
    }

    fn pinned(&self, major: u32) -> Option<JavaRuntime> {
        self.chosen
            .iter()
            .filter(|(chosen_major, _)| *chosen_major == major)
            .filter_map(|(_, path)| self.inspect_registered(path))
            .find(|runtime| runtime.major() == major)
    }

    fn managed_candidates(&self) -> impl Iterator<Item = PathBuf> {
        (!self.registered_only)
            .then(|| inventory(&self.runtime_roots))
            .into_iter()
            .flatten()
            .filter(|root| runtime_record(root).is_file())
            .filter_map(|root| managed_executable(&root, JavaMode::Console))
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

    fn registered_runtimes(&self) -> impl Iterator<Item = JavaRuntime> + '_ {
        self.registered
            .iter()
            .filter_map(|path| self.inspect_registered(path))
    }
}

impl Java for SystemJava {
    fn executable(&self, mode: JavaMode) -> Option<PathBuf> {
        if self.registered_only {
            let runtime = self.registered_runtimes().next()?;
            return Some(
                self.pinned(runtime.major())
                    .unwrap_or(runtime)
                    .executable(mode),
            );
        }
        self.environment_candidates()
            .chain(self.managed_candidates())
            .filter_map(|path| self.inspect_automatic(&path))
            .chain(self.registered_runtimes())
            .next()
            .map(|runtime| runtime.executable(mode))
    }

    fn executable_for(&self, major: u32, mode: JavaMode) -> Option<PathBuf> {
        if let Some(runtime) = self.pinned(major) {
            return Some(runtime.executable(mode));
        }
        self.default_for(major, &self.discover())
            .map(|runtime| runtime.executable(mode))
    }

    fn executable_at(&self, path: &Path, major: Option<u32>, mode: JavaMode) -> Option<PathBuf> {
        let runtime = self.inspect_registered(path)?;
        if major.is_some_and(|major| runtime.major() != major) {
            return None;
        }
        Some(runtime.executable(mode))
    }
}

fn completed_component(root: &Path, executable: &Path) -> bool {
    let Ok(relative) = executable.strip_prefix(root) else {
        return true;
    };
    let Some(component) = relative.components().next() else {
        return false;
    };
    let directory = if java_bin(&managed_home(root)).is_dir() {
        root.to_owned()
    } else {
        root.join(component)
    };
    runtime_record(&directory).is_file()
}

/// The component directories installed by this launcher, in stable order.
fn inventory(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for root in roots {
        if java_bin(&managed_home(root)).is_dir() {
            found.push(root.clone());
            continue;
        }
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        found.extend(
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir() && java_bin(&managed_home(path)).is_dir()),
        );
    }
    found.sort();
    found.dedup();
    found
}

#[cfg(test)]
#[path = "../../../../../../tests/apps/core/minecraft/launch/src/java/selection.rs"]
mod selection_tests;
#[cfg(test)]
#[path = "../../../../../../tests/apps/core/minecraft/launch/src/java/mod.rs"]
mod tests;
