// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Desktop policy for choosing and registering Java.

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use axlauncher_launch::{JavaRuntime, SystemJava};
use axlauncher_state::javas::{self, Javas, JavasError};
use axlauncher_storage::{DataDir, DataPath};
use axlauncher_worker::installation::JavaPaths;
use axlauncher_worker::java_discovery;

/// A recorded runtime outranks the newest discovered runtime for its major.
#[cfg(test)]
fn managed(data: &DataDir) -> SystemJava {
    provider(data, &preferences(data.path(DataPath::Root)))
}

pub(super) fn install_snapshot(data: &DataDir) -> JavaPaths {
    snapshot(&preferences(data.path(DataPath::Root)))
}

fn snapshot(javas: &Javas) -> JavaPaths {
    JavaPaths::new(
        javas.paths().map(Path::to_path_buf).collect(),
        chosen(javas),
    )
}

/// Only a complete, verified managed executable can be acknowledged to the installer.
pub(super) fn register_provisioned(
    data: &DataDir,
    path: &Path,
    major: u32,
) -> Result<JavaPaths, String> {
    let root = data.path(DataPath::JavaVersions);
    let runtime = validate(data, path, Some(major)).map_err(|error| error.to_string())?;
    let canonical = |path: &Path| std::fs::canonicalize(path).map_err(|error| error.to_string());
    if !path.starts_with(root) || !canonical(runtime.path())?.starts_with(canonical(root)?) {
        return Err(
            "The installer returned a Java path outside the managed runtime directory.".into(),
        );
    }
    javas::update(data.path(DataPath::Root), |saved| {
        register(&inspector(data), saved, &[runtime]);
        snapshot(saved)
    })
    .map_err(|error| error.to_string())
}

fn preferences(root: &Path) -> Javas {
    javas::load(root).unwrap_or_default()
}

pub(super) fn provider(data: &DataDir, javas: &Javas) -> SystemJava {
    inspector(data)
        .with_chosen(chosen(javas))
        .with_registered(javas.paths().map(Path::to_path_buf).collect())
}

fn inspector(data: &DataDir) -> SystemJava {
    SystemJava::for_registered_paths(vec![data.path(DataPath::JavaVersions).to_owned()])
}

fn chosen(javas: &Javas) -> Vec<(u32, PathBuf)> {
    javas
        .preferred()
        .filter(|(_, path)| javas.contains(path))
        .map(|(major, path)| (major, path.to_owned()))
        .collect()
}

/// The available installations and the preferences saved on this machine.
#[derive(PartialEq, Eq)]
pub struct JavaCatalog {
    available: Vec<JavaRuntime>,
    saved: Javas,
}

impl JavaCatalog {
    pub fn available(&self) -> &[JavaRuntime] {
        &self.available
    }

    /// Includes registrations whose files are currently unavailable.
    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        self.saved.paths()
    }

    pub fn preferred(&self) -> impl Iterator<Item = (u32, &Path)> {
        self.saved.preferred()
    }

    pub fn profile_preference(&self, profile_id: &str) -> Option<&Path> {
        self.saved.profile_preference(profile_id)
    }

    /// Resolves a saved choice while its listed executable is available.
    pub fn profile_runtime(&self, profile_id: &str) -> Option<&JavaRuntime> {
        let path = self.profile_preference(profile_id)?;
        self.available.iter().find(|runtime| runtime.path() == path)
    }
}

#[derive(Debug)]
pub enum JavaCatalogError {
    State(JavasError),
    Discovery(String),
    InvalidRuntime {
        path: PathBuf,
        required: Option<u32>,
    },
    Unregistered(PathBuf),
}

impl fmt::Display for JavaCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::State(error) => error.fmt(formatter),
            Self::Discovery(error) => write!(formatter, "Could not discover Java: {error}"),
            Self::Unregistered(path) => {
                write!(formatter, "{} is not in the Java list", path.display())
            }
            Self::InvalidRuntime {
                path,
                required: Some(major),
            } => {
                write!(
                    formatter,
                    "{} does not provide a usable Java {major}",
                    path.display()
                )
            }
            Self::InvalidRuntime {
                path,
                required: None,
            } => {
                write!(
                    formatter,
                    "{} is not a usable Java installation",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for JavaCatalogError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::State(error) => Some(error),
            Self::Discovery(_) | Self::InvalidRuntime { .. } | Self::Unregistered(_) => None,
        }
    }
}

impl From<JavasError> for JavaCatalogError {
    fn from(error: JavasError) -> Self {
        Self::State(error)
    }
}

/// Reads the saved list and checks its executable paths.
pub fn list(data: &DataDir) -> Result<JavaCatalog, JavaCatalogError> {
    Ok(catalog(data, javas::load(data.path(DataPath::Root))?))
}

fn catalog(data: &DataDir, saved: Javas) -> JavaCatalog {
    let available = inspect_registered(&inspector(data), &saved);
    JavaCatalog { available, saved }
}

fn inspect_registered(inspector: &SystemJava, saved: &Javas) -> Vec<JavaRuntime> {
    saved
        .paths()
        .filter_map(|path| inspector.inspect_registered(path))
        .collect()
}

/// Loads the Java list at application startup, discovering only when it is empty.
pub fn startup(data: &DataDir) -> Result<JavaCatalog, JavaCatalogError> {
    discover_with(data, true, || scan(data), SystemJava::from_environment)
}

/// Scans the machine and merges the results into the saved Java list.
pub fn discover(data: &DataDir) -> Result<JavaCatalog, JavaCatalogError> {
    discover_with(data, false, || scan(data), SystemJava::from_environment)
}

fn scan(data: &DataDir) -> Result<Vec<PathBuf>, String> {
    super::on_worker(java_discovery::scan(data)).map_err(|error| error.to_string())?
}

fn discover_with(
    data: &DataDir,
    only_if_empty: bool,
    scan: impl FnOnce() -> Result<Vec<PathBuf>, String>,
    inspect: impl FnOnce() -> SystemJava,
) -> Result<JavaCatalog, JavaCatalogError> {
    let saved = javas::load(data.path(DataPath::Root))?;
    if only_if_empty && !saved.is_empty() {
        return Ok(catalog(data, saved));
    }
    let candidates = scan().map_err(JavaCatalogError::Discovery)?;
    let found = inspect()
        .with_runtime_roots(vec![data.path(DataPath::JavaVersions).to_owned()])
        .discover_with_candidates(candidates);
    let inspector = inspector(data);
    let saved = javas::update(data.path(DataPath::Root), |saved| {
        if !only_if_empty || saved.is_empty() {
            register(&inspector, saved, &found);
        }
        saved.clone()
    })?;
    Ok(catalog(data, saved))
}

fn register(inspector: &SystemJava, saved: &mut Javas, found: &[JavaRuntime]) {
    let existing: BTreeSet<_> = saved
        .installations()
        .filter_map(|(path, major)| {
            major.or_else(|| {
                inspector
                    .inspect_registered(path)
                    .map(|runtime| runtime.major())
            })
        })
        .collect();
    let new_majors: BTreeSet<_> = found.iter().map(JavaRuntime::major).collect();
    for runtime in found {
        saved.add(runtime.path().to_owned(), runtime.major());
    }
    for &major in new_majors.difference(&existing) {
        decide_preferred(saved, major, found);
    }
}

fn decide_preferred(saved: &mut Javas, major: u32, candidates: &[JavaRuntime]) {
    let selected = JavaRuntime::preferred_candidate(major, candidates)
        .map(|runtime| runtime.path().to_owned());
    saved.set_preferred(major, selected);
}

/// Registers a Java executable after checking its installation.
pub fn add(data: &DataDir, path: &Path) -> Result<JavaRuntime, JavaCatalogError> {
    let runtime = validate(data, path, None)?;
    javas::update(data.path(DataPath::Root), |javas| {
        register(&inspector(data), javas, std::slice::from_ref(&runtime));
    })?;
    Ok(runtime)
}

/// Removes a listed Java and replaces any per-major preference that pointed to it.
pub fn remove(data: &DataDir, path: &Path) -> Result<bool, JavaCatalogError> {
    Ok(javas::update(data.path(DataPath::Root), |saved| {
        let affected = saved
            .preferred()
            .find_map(|(major, selected)| (selected == path).then_some(major));
        if !saved.remove(path) {
            return false;
        }
        if let Some(major) = affected {
            let inspector = inspector(data);
            let remaining = inspect_registered(&inspector, saved);
            decide_preferred(saved, major, &remaining);
        }
        true
    })?)
}

/// Pins a listed runtime for a Java major.
pub fn set_preferred(data: &DataDir, major: u32, path: &Path) -> Result<(), JavaCatalogError> {
    let path = validate(data, path, Some(major))?.path().to_owned();
    javas::update(data.path(DataPath::Root), |saved| {
        require_registered(saved, Some(&path))?;
        saved.set_preferred(major, Some(path));
        Ok(())
    })?
}

/// Pins a runtime for a local instance, or restores its automatic selection.
pub fn set_profile_preference(
    data: &DataDir,
    profile_id: &str,
    path: Option<&Path>,
) -> Result<(), JavaCatalogError> {
    let path = validated_path(data, path, None)?;
    javas::update(data.path(DataPath::Root), |saved| {
        require_registered(saved, path.as_deref())?;
        saved.set_profile_preference(profile_id, path);
        Ok(())
    })?
}

fn require_registered(saved: &Javas, path: Option<&Path>) -> Result<(), JavaCatalogError> {
    match path {
        Some(path) if !saved.contains(path) => Err(JavaCatalogError::Unregistered(path.to_owned())),
        _ => Ok(()),
    }
}

fn validated_path(
    data: &DataDir,
    path: Option<&Path>,
    required: Option<u32>,
) -> Result<Option<PathBuf>, JavaCatalogError> {
    path.map(|path| validate(data, path, required).map(|runtime| runtime.path().to_owned()))
        .transpose()
}

fn validate(
    data: &DataDir,
    path: &Path,
    required: Option<u32>,
) -> Result<JavaRuntime, JavaCatalogError> {
    inspector(data)
        .inspect(path)
        .filter(|runtime| required.is_none_or(|major| runtime.major() == major))
        .ok_or_else(|| JavaCatalogError::InvalidRuntime {
            path: path.to_owned(),
            required,
        })
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/src/desktop/java.rs"]
mod tests;
#[cfg(test)]
pub(super) use tests::test_runtime;
