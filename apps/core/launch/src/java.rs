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

    /// A JDK home whose `release` file reports `version`.
    fn jdk_home(root: &Path, version: &str) -> PathBuf {
        let home = root.join(format!("jdk-{version}"));
        fs::create_dir_all(home.join("bin")).expect("bin");
        fs::write(
            home.join("release"),
            format!(
                "IMPLEMENTOR=\"Example\"
JAVA_VERSION=\"{version}\"
"
            ),
        )
        .expect("release");
        java_in(&home.join("bin"), JavaMode::Console);
        home
    }

    #[test]
    fn a_major_version_is_read_from_the_release_file() {
        let temp = TempDir::new();
        let home = jdk_home(temp.path(), "21.0.1");
        let executable = home
            .join("bin")
            .join(crate::platform::name(JavaMode::Console));

        assert_eq!(runtime_major(&executable), Some(21));
    }

    #[test]
    fn the_old_one_dot_versioning_reports_its_major() {
        let temp = TempDir::new();
        let home = jdk_home(temp.path(), "1.8.0_392");
        let executable = home
            .join("bin")
            .join(crate::platform::name(JavaMode::Console));

        assert_eq!(runtime_major(&executable), Some(8));
    }

    #[test]
    fn a_home_without_a_release_file_reports_no_major() {
        let temp = TempDir::new();
        let bin = temp.path().join("home").join("bin");
        let executable = java_in(&bin, JavaMode::Console);

        assert_eq!(runtime_major(&executable), None);
    }

    #[test]
    fn a_matching_major_is_found_among_the_search_path() {
        let temp = TempDir::new();
        let older = jdk_home(temp.path(), "17.0.2");
        let newer = jdk_home(temp.path(), "25.0.1");
        let search_path =
            std::env::join_paths([older.join("bin"), newer.join("bin")]).expect("path");

        let java = SystemJava::with_environment(None, Some(search_path));
        let found = java.executable_for(25, JavaMode::Console).expect("java 25");

        assert!(
            found.starts_with(&newer),
            "expected a runtime under {}, got {}",
            newer.display(),
            found.display()
        );
    }

    #[test]
    fn a_major_nothing_offers_is_reported_as_absent() {
        let temp = TempDir::new();
        let home = jdk_home(temp.path(), "17.0.2");
        let search_path = std::env::join_paths([home.join("bin")]).expect("path");

        let java = SystemJava::with_environment(None, Some(search_path));

        assert_eq!(java.executable_for(25, JavaMode::Console), None);
    }
}
