// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use crate::platform::executable_name;
use std::fs;
use std::sync::atomic::{AtomicU32, Ordering};

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("axlauncher-launch-{}-{unique}", std::process::id()));
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
    let search_path = std::env::join_paths([older.join("bin"), newer.join("bin")]).expect("path");

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

#[test]
fn an_installed_runtime_is_preferred_over_the_environment() {
    let temp = TempDir::new();
    let managed = jdk_home(&temp.path().join("managed"), "21.0.7");
    let environment = jdk_home(&temp.path().join("env"), "17.0.2");
    let search_path = std::env::join_paths([environment.join("bin")]).expect("path");

    let java = SystemJava::with_environment(None, Some(search_path))
        .with_runtime_roots(vec![managed.clone()]);

    let found = java.executable_for(21, JavaMode::Console).expect("java 21");

    assert!(
        found.starts_with(&managed),
        "expected {}, got {}",
        managed.display(),
        found.display()
    );
}

#[test]
fn a_chosen_runtime_wins_over_java_home_and_the_search_path() {
    let temp = TempDir::new();
    // All three offer major 21, so the test fails if the choice is not what
    // decided the answer.
    let chosen = jdk_home(&temp.path().join("chosen"), "21.0.7");
    let home = jdk_home(&temp.path().join("home"), "21.0.7");
    let on_path = jdk_home(&temp.path().join("path"), "21.0.7");
    let search_path = std::env::join_paths([on_path.join("bin")]).expect("path");
    let executable = chosen.join("bin").join(executable_name(JavaMode::Console));

    let java = SystemJava::with_environment(Some(home), Some(search_path))
        .with_chosen(vec![(21, executable.clone())]);

    assert_eq!(java.executable_for(21, JavaMode::Console), Some(executable));
}

#[test]
fn a_chosen_runtime_for_another_major_is_ignored() {
    let temp = TempDir::new();
    let chosen = jdk_home(&temp.path().join("chosen"), "25.0.1");
    let home = jdk_home(&temp.path().join("home"), "17.0.2");
    let chosen_executable = chosen.join("bin").join(executable_name(JavaMode::Console));
    let environment_executable = home.join("bin").join(executable_name(JavaMode::Console));

    let java =
        SystemJava::with_environment(Some(home), None).with_chosen(vec![(25, chosen_executable)]);

    assert_eq!(
        java.executable_for(17, JavaMode::Console),
        Some(environment_executable)
    );
}

#[test]
fn a_chosen_runtime_that_was_removed_falls_through_to_the_environment() {
    let temp = TempDir::new();
    let gone = temp
        .path()
        .join("gone")
        .join("bin")
        .join(executable_name(JavaMode::Console));
    let home = jdk_home(&temp.path().join("home"), "21.0.7");
    let environment_executable = home.join("bin").join(executable_name(JavaMode::Console));

    let java = SystemJava::with_environment(Some(home), None).with_chosen(vec![(21, gone)]);

    assert_eq!(
        java.executable_for(21, JavaMode::Console),
        Some(environment_executable)
    );
}

#[test]
fn the_default_executable_ignores_a_chosen_runtime() {
    let temp = TempDir::new();
    let chosen = jdk_home(&temp.path().join("chosen"), "21.0.7");
    let home = jdk_home(&temp.path().join("home"), "17.0.2");
    let chosen_executable = chosen.join("bin").join(executable_name(JavaMode::Console));
    let environment_executable = home.join("bin").join(executable_name(JavaMode::Console));

    // A choice is keyed by major, and this call has no major to match it
    // against. Applying it here would start every document on that runtime.
    let java =
        SystemJava::with_environment(Some(home), None).with_chosen(vec![(21, chosen_executable)]);

    assert_eq!(
        java.executable(None, JavaMode::Console),
        Some(environment_executable)
    );
}

#[test]
fn a_chosen_runtime_follows_the_requested_mode() {
    let temp = TempDir::new();
    let home = jdk_home(&temp.path().join("chosen"), "21.0.7");
    let bin = home.join("bin");
    let console = bin.join(executable_name(JavaMode::Console));
    java_in(&bin, JavaMode::Windowed);

    let java = SystemJava::with_environment(None, None).with_chosen(vec![(21, console)]);

    assert_eq!(
        java.executable_for(21, JavaMode::Windowed),
        Some(bin.join(executable_name(JavaMode::Windowed)))
    );
}

#[test]
fn a_version_without_a_stated_major_does_not_get_a_managed_runtime() {
    let temp = TempDir::new();
    let managed = jdk_home(&temp.path().join("managed"), "21.0.7");
    let environment = jdk_home(&temp.path().join("env"), "8.0.392");
    let search_path = std::env::join_paths([environment.join("bin")]).expect("path");

    // A pre-1.17 document asks for nothing in particular. Picking a managed
    // runtime there would start an old game on whatever the launcher fetched
    // last, so the user's own Java is the answer.
    let java =
        SystemJava::with_environment(None, Some(search_path)).with_runtime_roots(vec![managed]);

    let found = java.executable(None, JavaMode::Console).expect("java");

    assert!(
        found.starts_with(&environment),
        "expected the environment runtime {}, got {}",
        environment.display(),
        found.display()
    );
}
