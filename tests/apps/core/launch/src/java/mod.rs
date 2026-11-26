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
        Self(dunce::canonicalize(path).expect("canonical temp directory"))
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
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("executable");
    }
    path
}

fn joined(paths: &[&Path]) -> OsString {
    std::env::join_paths(paths).expect("search path")
}

#[test]
fn java_home_is_searched_before_the_search_path() {
    let temp = TempDir::new();
    let home = temp.path().join("home");
    let installed = install_fixture(&home, "21.0.1");
    let on_path = install_fixture(&temp.path().join("path"), "17.0.1");
    let on_path = on_path.parent().unwrap();
    let java = SystemJava::with_environment(Some(home), Some(joined(&[on_path])));

    assert_eq!(java.executable(JavaMode::Console), Some(installed));
}

#[test]
fn the_search_path_is_scanned_in_order() {
    let temp = TempDir::new();
    let first = install_fixture(&temp.path().join("first"), "21.0.1");
    let second = install_fixture(&temp.path().join("second"), "17.0.1");
    let java = SystemJava::with_environment(
        None,
        Some(joined(&[
            temp.path().join("empty").as_path(),
            first.parent().expect("parent"),
            second.parent().expect("parent"),
        ])),
    );

    assert_eq!(java.executable(JavaMode::Console), Some(first));
}

#[test]
fn nothing_found_is_none() {
    let temp = TempDir::new();
    let java = SystemJava::with_environment(
        Some(temp.path().join("home")),
        Some(joined(&[temp.path().join("empty").as_path()])),
    );

    assert_eq!(java.executable(JavaMode::Console), None);
}

#[test]
fn the_windowed_mode_uses_its_own_executable_when_present() {
    let temp = TempDir::new();
    let home = temp.path().join("home");
    let console = install_fixture(&home, "21.0.1");
    let windowed = java_in(console.parent().unwrap(), JavaMode::Windowed);
    let java = SystemJava::with_environment(Some(home), None);

    assert_eq!(java.executable(JavaMode::Windowed), Some(windowed));
}

#[test]
fn the_windowed_mode_falls_back_to_the_console_executable() {
    let temp = TempDir::new();
    let home = temp.path().join("home");
    let console = install_fixture(&home, "21.0.1");
    let java = SystemJava::with_environment(Some(home), None);

    // On platforms whose two names are the same file this is trivially the
    // console path; on Windows it exercises the fallback.
    assert_eq!(java.executable(JavaMode::Windowed), Some(console));
}

/// A JDK home whose `release` file reports `version`.
fn jdk_home(root: &Path, version: &str) -> PathBuf {
    let home = root.join(format!("jdk-{version}"));
    install_fixture(&home, version);
    home
}

fn install_fixture(home: &Path, version: &str) -> PathBuf {
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
    java_in(&home.join("bin"), JavaMode::Console)
}

#[test]
fn a_major_version_is_read_from_the_release_file() {
    let temp = TempDir::new();
    let home = jdk_home(temp.path(), "21.0.1");
    let executable = home
        .join("bin")
        .join(crate::platform::name(JavaMode::Console));

    assert_eq!(JavaRuntime::inspect(&executable).unwrap().major(), 21);
}

#[test]
fn the_old_one_dot_versioning_reports_its_major() {
    let temp = TempDir::new();
    let home = jdk_home(temp.path(), "1.8.0_392");
    let executable = home
        .join("bin")
        .join(crate::platform::name(JavaMode::Console));

    assert_eq!(JavaRuntime::inspect(&executable).unwrap().major(), 8);
}

#[test]
fn a_home_without_a_release_file_reports_no_major() {
    let temp = TempDir::new();
    let bin = temp.path().join("home").join("bin");
    let executable = java_in(&bin, JavaMode::Console);

    assert_eq!(JavaRuntime::inspect(&executable), None);
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
fn the_newest_release_wins_across_discovery_sources() {
    let temp = TempDir::new();
    let managed = jdk_home(&temp.path().join("managed"), "21.0.9");
    fs::write(runtime_record(&managed), b"{}").unwrap();
    let environment = jdk_home(&temp.path().join("env"), "21.0.10");
    let search_path = std::env::join_paths([environment.join("bin")]).expect("path");

    let java =
        SystemJava::with_environment(None, Some(search_path)).with_runtime_roots(vec![managed]);

    let found = java.executable_for(21, JavaMode::Console).expect("java 21");

    assert!(
        found.starts_with(&environment),
        "expected {}, got {}",
        environment.display(),
        found.display()
    );
}

#[test]
fn managed_discovery_only_selects_published_installations() {
    let temp = TempDir::new();
    let managed = jdk_home(temp.path(), "21.0.7");
    let java = SystemJava::default().with_runtime_roots(vec![temp.path().to_owned()]);
    let receipt = runtime_record(&managed);

    assert!(java.discover().is_empty());
    assert!(java.executable_for(21, JavaMode::Console).is_none());
    assert!(java.executable(JavaMode::Console).is_none());

    fs::write(&receipt, b"{}").unwrap();
    assert!(java.executable_for(21, JavaMode::Console).is_some());
    fs::remove_file(receipt).unwrap();
    assert!(java.executable_for(21, JavaMode::Console).is_none());
}

#[test]
fn a_chosen_runtime_wins_over_java_home_and_the_search_path() {
    let temp = TempDir::new();
    let chosen = jdk_home(&temp.path().join("chosen"), "21.0.7");
    let home = jdk_home(&temp.path().join("home"), "21.0.8");
    let on_path = jdk_home(&temp.path().join("path"), "21.0.9");
    let search_path = std::env::join_paths([on_path.join("bin")]).expect("path");
    let executable = chosen.join("bin").join(executable_name(JavaMode::Console));

    let java = SystemJava::with_environment(Some(home), Some(search_path))
        .with_chosen(vec![(21, executable.clone())]);

    assert_eq!(java.executable_for(21, JavaMode::Console), Some(executable));
}

#[test]
fn runtime_build_breaks_a_tie_in_java_version() {
    let temp = TempDir::new();
    let earlier = jdk_home(&temp.path().join("a"), "25.0.3");
    let latest = jdk_home(&temp.path().join("b"), "25.0.3");
    for (home, build) in [(&earlier, 9), (&latest, 10)] {
        fs::write(
            home.join("release"),
            format!("JAVA_VERSION=\"25.0.3\"\nJAVA_RUNTIME_VERSION=\"25.0.3+{build}-LTS\""),
        )
        .unwrap();
    }
    let path = joined(&[&earlier.join("bin"), &latest.join("bin")]);
    let java = SystemJava::with_environment(None, Some(path));
    assert_eq!(
        java.executable_for(25, JavaMode::Console),
        Some(latest.join("bin").join(executable_name(JavaMode::Console)))
    );
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
        java.executable(JavaMode::Console),
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
fn a_version_without_a_stated_major_prefers_the_environment() {
    let temp = TempDir::new();
    let managed = jdk_home(&temp.path().join("managed"), "21.0.7");
    fs::write(runtime_record(&managed), b"{}").unwrap();
    let environment = jdk_home(&temp.path().join("env"), "8.0.392");
    let search_path = std::env::join_paths([environment.join("bin")]).expect("path");

    // A pre-1.17 document asks for nothing in particular. Picking a managed
    // runtime there would start an old game on whatever the launcher fetched
    // last, so the user's own Java is the answer.
    let java =
        SystemJava::with_environment(None, Some(search_path)).with_runtime_roots(vec![managed]);

    let found = java.executable(JavaMode::Console).expect("java");

    assert!(
        found.starts_with(&environment),
        "expected the environment runtime {}, got {}",
        environment.display(),
        found.display()
    );
}

#[test]
fn a_choice_replaced_with_a_different_major_falls_back_to_another_installation() {
    let temp = TempDir::new();
    let replaced = jdk_home(&temp.path().join("replaced"), "25-ea+4");
    let home = jdk_home(&temp.path().join("home"), "21.0.1");
    let replaced_executable = replaced
        .join("bin")
        .join(executable_name(JavaMode::Console));
    let expected = home.join("bin").join(executable_name(JavaMode::Console));
    let provider = SystemJava::with_environment(Some(home), None)
        .with_chosen(vec![(21, replaced_executable.clone())]);

    assert_eq!(
        JavaRuntime::inspect(&replaced_executable).unwrap().major(),
        25
    );
    assert_eq!(
        provider.executable_for(21, JavaMode::Console),
        Some(expected)
    );
}

#[test]
fn conventional_scanning_respects_the_directory_depth_and_handles_bundles() {
    let temp = TempDir::new();
    let nested = temp.path().join("component/platform/runtime");
    install_fixture(&nested, "21.0.1");
    let mac = temp.path().join("temurin.jdk/Contents/Home");
    install_fixture(&mac, "17.0.1");
    let mojang = temp
        .path()
        .join("gamma/mac-os/gamma/jre.bundle/Contents/Home");
    install_fixture(&mojang, "21.0.1");
    jdk_home(&temp.path().join("too/many/levels/deep"), "25.0.1");
    let mut found = Vec::new();
    discovery::scan(temp.path(), 3, &mut found);
    let name = executable_name(JavaMode::Console);
    assert_eq!(found.len(), 3);
    assert!(found.contains(&nested.join("bin").join(name)));
    assert!(found.contains(&mac.join("bin").join(name)));
    assert!(found.contains(&mojang.join("bin").join(name)));
}

#[test]
fn the_bundled_java_eight_executable_uses_the_enclosing_jdk_release() {
    let temp = TempDir::new();
    install_fixture(temp.path(), "1.8.0_392");
    let executable = java_in(&temp.path().join("jre/bin"), JavaMode::Console);

    assert_eq!(JavaRuntime::inspect(&executable).unwrap().major(), 8);
}

#[cfg(unix)]
#[test]
fn discovery_deduplicates_aliases_of_the_same_installation() {
    let temp = TempDir::new();
    let home = jdk_home(&temp.path().join("home"), "21.0.1");
    let executable = home.join("bin/java");
    let alias = temp.path().join("java-link");
    std::os::unix::fs::symlink(&executable, &alias).unwrap();
    let provider = SystemJava::with_environment(Some(home), None).with_registered(vec![alias]);
    let available = provider.discover();

    assert_eq!(available.len(), 1);
    assert_eq!(available[0].path(), executable);
}
