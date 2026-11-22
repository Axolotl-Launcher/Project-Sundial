// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Java discovery from the user's configuration and environment.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use axlauncher_spi::{Java, JavaMode};

use crate::platform::{executable_in_home, search, sibling};

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
}

impl Java for SystemJava {
    fn executable(&self, preferred: Option<&Path>, mode: JavaMode) -> Option<PathBuf> {
        // Discovery looks for the console executable: it is the one every JDK
        // ships. The requested mode is reached through its sibling afterwards,
        // because a JDK keeps both on Windows.
        let found = preferred
            .filter(|path| path.is_file())
            .map(Path::to_path_buf)
            .or_else(|| {
                self.java_home
                    .as_deref()
                    .and_then(|home| executable_in_home(home, JavaMode::Console))
            })
            .or_else(|| {
                self.search_path
                    .as_deref()
                    .and_then(|path| search(path, JavaMode::Console))
            })?;

        Some(sibling(&found, mode).unwrap_or(found))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::executable_name;
    use std::fs;
    use std::sync::atomic::{AtomicU32, Ordering};

    static COUNTER: AtomicU32 = AtomicU32::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("axlauncher-launch-{}-{unique}", std::process::id()));
            fs::create_dir_all(&path).expect("temp directory");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A fake Java executable named for `mode` under `directory`.
    fn java_in(directory: &Path, mode: JavaMode) -> PathBuf {
        fs::create_dir_all(directory).expect("directory");
        let path = directory.join(executable_name(mode));
        fs::write(&path, b"fake").expect("java");
        path
    }

    fn joined(paths: &[&Path]) -> OsString {
        std::env::join_paths(paths).expect("search path")
    }

    #[test]
    fn a_configured_path_wins_when_it_exists() {
        let temp = TempDir::new();
        let preferred = java_in(&temp.path().join("custom"), JavaMode::Console);
        let home = temp.path().join("home");
        java_in(&home.join("bin"), JavaMode::Console);

        let java = SystemJava::with_environment(Some(home), None);

        assert_eq!(
            java.executable(Some(&preferred), JavaMode::Console),
            Some(preferred)
        );
    }

    #[test]
    fn a_missing_configured_path_falls_back_to_the_environment() {
        let temp = TempDir::new();
        let home = temp.path().join("home");
        let installed = java_in(&home.join("bin"), JavaMode::Console);
        let java = SystemJava::with_environment(Some(home), None);

        assert_eq!(
            java.executable(
                Some(
                    &temp
                        .path()
                        .join("nowhere")
                        .join(executable_name(JavaMode::Console))
                ),
                JavaMode::Console
            ),
            Some(installed)
        );
    }

    #[test]
    fn java_home_is_searched_before_the_search_path() {
        let temp = TempDir::new();
        let home = temp.path().join("home");
        let installed = java_in(&home.join("bin"), JavaMode::Console);
        let on_path = temp.path().join("path");
        java_in(&on_path, JavaMode::Console);
        let java = SystemJava::with_environment(Some(home), Some(joined(&[on_path.as_path()])));

        assert_eq!(java.executable(None, JavaMode::Console), Some(installed));
    }

    #[test]
    fn the_search_path_is_scanned_in_order() {
        let temp = TempDir::new();
        let first = java_in(&temp.path().join("first"), JavaMode::Console);
        let second = java_in(&temp.path().join("second"), JavaMode::Console);
        let java = SystemJava::with_environment(
            None,
            Some(joined(&[
                temp.path().join("empty").as_path(),
                first.parent().expect("parent"),
                second.parent().expect("parent"),
            ])),
        );

        assert_eq!(java.executable(None, JavaMode::Console), Some(first));
    }

    #[test]
    fn nothing_found_is_none() {
        let temp = TempDir::new();
        let java = SystemJava::with_environment(
            Some(temp.path().join("home")),
            Some(joined(&[temp.path().join("empty").as_path()])),
        );

        assert_eq!(java.executable(None, JavaMode::Console), None);
    }

    #[test]
    fn the_windowed_mode_uses_its_own_executable_when_present() {
        let temp = TempDir::new();
        let bin = temp.path().join("home").join("bin");
        java_in(&bin, JavaMode::Console);
        let windowed = java_in(&bin, JavaMode::Windowed);
        let java = SystemJava::with_environment(Some(temp.path().join("home")), None);

        assert_eq!(java.executable(None, JavaMode::Windowed), Some(windowed));
    }

    #[test]
    fn the_windowed_mode_falls_back_to_the_console_executable() {
        let temp = TempDir::new();
        let bin = temp.path().join("home").join("bin");
        let console = java_in(&bin, JavaMode::Console);
        let java = SystemJava::with_environment(Some(temp.path().join("home")), None);

        // On platforms whose two names are the same file this is trivially the
        // console path; on Windows it exercises the fallback.
        assert_eq!(java.executable(None, JavaMode::Windowed), Some(console));
    }
}
