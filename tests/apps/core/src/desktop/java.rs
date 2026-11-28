// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use axlauncher_launch::{Java, JavaMode};

#[test]
fn provisioned_java_requires_a_managed_receipt_and_returns_refreshed_preferences() {
    use axlauncher_storage::Document;
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let outside = test_runtime(&root.path().join("external"), 21);
    assert!(register_provisioned(&data, &outside, 21).is_err());
    let component = data.path(DataPath::JavaVersions).join("test-runtime");
    let runtime = test_runtime(&component, 21);
    assert!(register_provisioned(&data, &runtime, 21).is_err());
    std::fs::write(Document::RuntimeReceipt.path(&component), "{}").unwrap();
    assert!(register_provisioned(&data, &runtime, 17).is_err());
    let (paths, chosen) = register_provisioned(&data, &runtime, 21)
        .unwrap()
        .into_paths();
    let runtime = JavaRuntime::inspect(&runtime).unwrap();
    assert_eq!(paths, [runtime.path()]);
    assert_eq!(chosen, [(21, runtime.path().to_owned())]);
}

pub(in crate::desktop) fn test_runtime(home: &Path, major: u32) -> PathBuf {
    let bin = home.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let path = bin.join(if cfg!(windows) { "java.exe" } else { "java" });
    std::fs::write(&path, b"fixture").unwrap();
    std::fs::write(
        home.join("release"),
        format!("JAVA_VERSION=\"{major}.0.1\"\nOS_ARCH=\"fixture\"\n"),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    path
}

fn scan_paths(
    data: &DataDir,
    only_if_empty: bool,
    scan: impl FnOnce() -> Result<Vec<PathBuf>, String>,
) -> Result<JavaCatalog, JavaCatalogError> {
    discover_with(data, only_if_empty, scan, SystemJava::default)
}

#[test]
fn a_registered_runtime_is_listed_and_used_for_automatic_selection() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let path = test_runtime(&root.path().join("custom"), 42);

    let registered = add(&data, &path).unwrap();
    let catalog = list(&data).unwrap();
    assert!(catalog.available().contains(&registered));
    assert_eq!(catalog.paths().collect::<Vec<_>>(), [registered.path()]);
    assert_eq!(
        managed(&data)
            .executable_for(42, JavaMode::Console)
            .as_deref(),
        Some(registered.path())
    );

    assert_eq!(
        list(&data).unwrap().preferred().collect::<Vec<_>>(),
        [(42, registered.path())]
    );
    std::fs::remove_file(&path).unwrap();
    let catalog = list(&data).unwrap();
    assert!(!catalog.available().contains(&registered));
    assert_eq!(catalog.paths().count(), 1);
    assert!(remove(&data, registered.path()).unwrap());
    assert_eq!(list(&data).unwrap().preferred().count(), 0);
}

#[test]
fn a_saved_profile_runtime_remains_selected_when_its_file_disappears() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let path = test_runtime(&root.path().join("custom"), 42);
    assert!(matches!(
        set_profile_preference(&data, "profile", Some(&path)),
        Err(JavaCatalogError::Unregistered(_))
    ));
    let registered = add(&data, &path).unwrap();
    set_profile_preference(&data, "profile", Some(registered.path())).unwrap();

    let catalog = list(&data).unwrap();
    let saved = catalog.profile_runtime("profile").unwrap();
    assert_eq!(saved.major(), 42);
    assert!(catalog.available().contains(saved));

    std::fs::remove_file(path).unwrap();
    let catalog = list(&data).unwrap();
    assert!(catalog.profile_runtime("profile").is_none());
    assert!(catalog.profile_preference("profile").is_some());
}

#[test]
fn discovery_chooses_after_merging_the_whole_batch_and_keeps_existing_preferences() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let earlier = test_runtime(&root.path().join("earlier"), 42);
    let updated = root.path().join("updated");
    let latest = test_runtime(&updated, 42);
    std::fs::write(
        updated.join("release"),
        "JAVA_VERSION=\"42.0.10\"\nOS_ARCH=\"fixture\"\n",
    )
    .unwrap();
    let other = test_runtime(&root.path().join("other"), 43);
    let [earlier, latest, other] =
        [earlier, latest, other].map(|path| JavaRuntime::inspect(&path).unwrap());
    let initial = scan_paths(&data, true, || {
        Ok([&earlier, &latest, &other]
            .map(|runtime| runtime.path().to_owned())
            .into())
    })
    .unwrap();
    assert_eq!(
        initial.preferred().collect::<Vec<_>>(),
        [(42, latest.path()), (43, other.path())]
    );

    set_preferred(&data, 42, earlier.path()).unwrap();
    let repeated = scan_paths(&data, false, || Ok(vec![latest.path().to_owned()])).unwrap();
    assert_eq!(repeated.preferred().next(), Some((42, earlier.path())));
    assert!(remove(&data, earlier.path()).unwrap());
    let remaining = list(&data).unwrap();
    assert_eq!(remaining.preferred().next(), Some((42, latest.path())));
    assert!(!remaining.available().contains(&earlier));
}

#[test]
fn invalid_paths_and_wrong_major_preferences_leave_the_catalog_untouched() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let path = test_runtime(&root.path().join("custom"), 42);
    let err = add(&data, root.path()).unwrap_err();
    assert!(matches!(
        err,
        JavaCatalogError::InvalidRuntime { required: None, .. }
    ));
    let err = set_preferred(&data, 43, &path).unwrap_err();
    assert!(matches!(
        err,
        JavaCatalogError::InvalidRuntime {
            required: Some(43),
            ..
        }
    ));
    let saved = javas::load(data.path(DataPath::Root)).unwrap();
    assert_eq!(saved.paths().count(), 0);
    assert_eq!(saved.preferred().count(), 0);
}

#[test]
fn startup_scans_only_for_an_empty_saved_list() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let path = test_runtime(&root.path().join("system"), 25);
    let runtime = JavaRuntime::inspect(&path).unwrap();
    let startup = scan_paths(&data, true, || Ok(vec![path.clone()])).unwrap();
    assert_eq!(startup.preferred().next(), Some((25, runtime.path())));
    std::fs::remove_file(&path).unwrap();

    let loaded = scan_paths(&data, true, || {
        panic!("the unavailable entry is still listed")
    })
    .unwrap();
    assert_eq!(loaded.paths().collect::<Vec<_>>(), [runtime.path()]);
    assert!(loaded.available().is_empty());
    assert!(super::startup(&data).unwrap() == loaded);
    assert!(remove(&data, runtime.path()).unwrap());
    assert!(list(&data).unwrap().paths().next().is_none());
}

#[test]
fn adding_a_newer_runtime_keeps_the_preference_until_it_is_removed() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let first = add(&data, &test_runtime(&root.path().join("first"), 25)).unwrap();
    let home = root.path().join("newer");
    let path = test_runtime(&home, 25);
    std::fs::write(
        home.join("release"),
        "JAVA_VERSION=\"25.0.9\"\nOS_ARCH=\"fixture\"\n",
    )
    .unwrap();
    let newer = JavaRuntime::inspect(&path).unwrap();
    let added = scan_paths(&data, false, || Ok(vec![path])).unwrap();
    assert_eq!(added.preferred().next(), Some((25, first.path())));
    assert!(remove(&data, newer.path()).unwrap());
    assert_eq!(
        list(&data).unwrap().preferred().next(),
        Some((25, first.path()))
    );

    add(&data, newer.path()).unwrap();
    assert!(remove(&data, first.path()).unwrap());
    assert_eq!(
        list(&data).unwrap().preferred().next(),
        Some((25, newer.path()))
    );
    assert_eq!(
        managed(&data)
            .executable_for(25, JavaMode::Console)
            .as_deref(),
        Some(newer.path())
    );
}

#[test]
fn startup_merges_only_if_no_other_writer_populated_the_list() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let path = test_runtime(&root.path().join("concurrent"), 42);
    let discovered = test_runtime(&root.path().join("discovered"), 43);
    let catalog = scan_paths(&data, true, || {
        add(&data, &path).unwrap();
        Ok(vec![discovered])
    })
    .unwrap();
    assert_eq!(catalog.available().len(), 1);
    assert_eq!(catalog.available()[0].major(), 42);
}

#[test]
fn a_removed_runtime_stays_out_of_launch_selection_until_rediscovered() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let component = data.path(DataPath::JavaVersions).join("component");
    let home = if cfg!(target_os = "macos") {
        component.join("jre.bundle/Contents/Home")
    } else {
        component.clone()
    };
    let path = test_runtime(&home, 42);
    std::fs::write(
        axlauncher_storage::Document::RuntimeReceipt.path(&component),
        b"{}",
    )
    .unwrap();

    assert_eq!(managed(&data).executable(JavaMode::Console), None);
    let java = add(&data, &path).unwrap();
    assert!(remove(&data, java.path()).unwrap());
    assert!(path.is_file());
    assert!(list(&data).unwrap().available().is_empty());
    assert_eq!(managed(&data).executable_for(42, JavaMode::Console), None);

    let found = scan_paths(&data, false, || Ok(Vec::new())).unwrap();
    assert_eq!(found.preferred().next(), Some((42, java.path())));
    assert_eq!(
        managed(&data).executable(JavaMode::Console).as_deref(),
        Some(java.path())
    );
}

#[test]
fn an_unavailable_registration_still_counts_toward_its_major() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let preferred = add(&data, &test_runtime(&root.path().join("preferred"), 25)).unwrap();
    let unavailable = test_runtime(&root.path().join("unavailable"), 25);
    let unavailable_runtime = add(&data, &unavailable).unwrap();
    std::fs::remove_file(&unavailable).unwrap();
    assert!(remove(&data, preferred.path()).unwrap());
    assert!(list(&data).unwrap().preferred().next().is_none());

    let added = add(&data, &test_runtime(&root.path().join("added"), 25)).unwrap();
    let catalog = list(&data).unwrap();
    assert!(
        catalog
            .paths()
            .any(|path| path == unavailable_runtime.path())
    );
    assert!(catalog.preferred().next().is_none());
    assert_eq!(
        managed(&data)
            .executable_for(25, JavaMode::Console)
            .as_deref(),
        Some(added.path())
    );
}

#[test]
fn bundled_java_is_a_fallback_for_new_preferences_and_can_be_pinned() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let product = root.path().join("IDE");
    let bundled_home = product.join("jbr");
    let bundled = test_runtime(&bundled_home, 25);
    let standalone = test_runtime(&root.path().join("standalone"), 25);
    std::fs::write(
        bundled_home.join("release"),
        "JAVA_VERSION=\"25.0.9\"\nOS_ARCH=\"fixture\"\n",
    )
    .unwrap();
    let executable = bundled.file_name().unwrap().to_str().unwrap();
    std::fs::write(
        product.join("product-info.json"),
        format!(r#"{{"launch":[{{"javaExecutablePath":"jbr/bin/{executable}"}}]}}"#),
    )
    .unwrap();
    let [bundled, standalone] =
        [bundled, standalone].map(|path| JavaRuntime::inspect(&path).unwrap());
    let catalog = scan_paths(&data, true, || {
        Ok(vec![
            bundled.path().to_owned(),
            standalone.path().to_owned(),
        ])
    })
    .unwrap();
    assert_eq!(catalog.preferred().next(), Some((25, standalone.path())));

    set_preferred(&data, 25, bundled.path()).unwrap();
    assert_eq!(
        managed(&data)
            .executable_for(25, JavaMode::Console)
            .as_deref(),
        Some(bundled.path())
    );
    set_preferred(&data, 25, standalone.path()).unwrap();
    assert!(remove(&data, standalone.path()).unwrap());
    assert_eq!(
        list(&data).unwrap().preferred().next(),
        Some((25, bundled.path()))
    );
}

#[test]
fn a_successful_explicit_scan_merges_system_environment_and_completed_managed_paths() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let prior = add(&data, &test_runtime(&root.path().join("prior"), 17)).unwrap();
    let from_home = root.path().join("a home");
    let from_environment = test_runtime(&from_home, 21);
    let managed_root = data.path(DataPath::JavaVersions);
    let completed = managed_root.join("complete");
    let managed_path = test_runtime(&completed, 25);
    std::fs::write(
        axlauncher_storage::Document::RuntimeReceipt.path(&completed),
        b"{}",
    )
    .unwrap();
    let incomplete = test_runtime(&managed_root.join("incomplete"), 26);
    let supplied = test_runtime(&root.path().join("system"), 42);
    let expected = [from_environment.as_path(), &managed_path, &supplied]
        .map(|path| JavaRuntime::inspect(path).unwrap());

    let catalog = discover_with(
        &data,
        false,
        || Ok(vec![from_environment, incomplete, supplied]),
        || SystemJava::with_environment(Some(from_home), None),
    )
    .unwrap();
    assert_eq!(
        catalog.preferred().collect::<Vec<_>>(),
        [
            (17, prior.path()),
            (21, expected[0].path()),
            (25, expected[1].path()),
            (42, expected[2].path()),
        ]
    );
    assert_eq!(catalog.paths().count(), 4);
}

#[test]
fn an_unavailable_worker_never_falls_back_to_scanning_on_startup() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    assert!(matches!(
        startup(&data),
        Err(JavaCatalogError::Discovery(error)) if error.contains("unavailable")
    ));
    assert!(
        !axlauncher_storage::Document::JavaCatalog
            .path(data.path(DataPath::Root))
            .exists()
    );
}

#[test]
fn a_failed_or_cancelled_scan_never_changes_the_catalog() {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let path = add(&data, &test_runtime(&root.path().join("saved"), 21)).unwrap();
    set_profile_preference(&data, "profile", Some(path.path())).unwrap();
    let document = axlauncher_storage::Document::JavaCatalog.path(data.path(DataPath::Root));
    let before = std::fs::read(&document).unwrap();

    for error in ["The worker exited.", "Java discovery cancelled."] {
        let outcome = discover_with(
            &data,
            false,
            || Err(error.into()),
            || panic!("do not inspect or merge candidates on failure"),
        );
        assert!(matches!(outcome, Err(JavaCatalogError::Discovery(message)) if message == error));
        assert_eq!(std::fs::read(&document).unwrap(), before);
    }
}
