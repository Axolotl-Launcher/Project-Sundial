use std::fs;
use std::path::{Path, PathBuf};

use axlauncher_launch::java_paths::{bundle_home, executable_in_home, managed_executable};
use axlauncher_launch::{JavaMode, JavaRuntime};
use axlauncher_worker::Cancellation;

#[cfg(windows)]
use axlauncher_launch::java_paths::excluded_from_discovery;

// Mojang uses a component/platform/component layout under its runtime root.
const SEARCH_DEPTH: u8 = 3;

pub(super) struct Scanner<'scan> {
    cancellation: &'scan Cancellation,
    emit: &'scan mut dyn FnMut(PathBuf) -> Result<(), String>,
}

impl<'scan> Scanner<'scan> {
    pub(super) fn new(
        cancellation: &'scan Cancellation,
        emit: &'scan mut dyn FnMut(PathBuf) -> Result<(), String>,
    ) -> Self {
        Self { cancellation, emit }
    }

    pub(super) fn checkpoint(&self) -> Result<(), String> {
        self.cancellation
            .is_pending()
            .then_some(())
            .ok_or_else(|| "Java discovery cancelled.".to_owned())
    }

    pub(super) fn candidate(&mut self, path: PathBuf) -> Result<(), String> {
        self.checkpoint()?;
        (self.emit)(path)
    }

    pub(super) fn conventional_roots(
        &mut self,
        roots: impl IntoIterator<Item = PathBuf>,
    ) -> Result<(), String> {
        for root in roots {
            self.conventional(&root, SEARCH_DEPTH)?;
        }
        self.checkpoint()
    }

    fn conventional(&mut self, root: &Path, depth: u8) -> Result<(), String> {
        self.checkpoint()?;
        #[cfg(windows)]
        if excluded_from_discovery(root)
            || dunce::canonicalize(root).is_ok_and(|path| excluded_from_discovery(&path))
        {
            return Ok(());
        }
        if let Some(path) = conventional_executable(root) {
            return self.candidate(path);
        }
        if depth == 0 {
            return Ok(());
        }
        let Ok(entries) = fs::read_dir(root) else {
            return Ok(());
        };
        for entry in entries.flatten() {
            self.checkpoint()?;
            let next_depth = match entry.file_type() {
                Ok(kind) if kind.is_dir() => depth - 1,
                Ok(kind) if kind.is_symlink() => 0,
                _ => continue,
            };
            self.conventional(&entry.path(), next_depth)?;
        }
        Ok(())
    }
}

fn conventional_executable(root: &Path) -> Option<PathBuf> {
    [
        managed_executable(root, JavaMode::Console),
        executable_in_home(&bundle_home(root), JavaMode::Console),
    ]
    .into_iter()
    .flatten()
    .find(|path| JavaRuntime::inspect_automatic(path).is_some())
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/lib/java_discovery/src/scan.rs"]
pub(super) mod tests;
