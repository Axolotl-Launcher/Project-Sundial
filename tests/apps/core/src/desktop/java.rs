// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::cell::Cell;

use super::*;

fn installed_document(json: &str) -> Version {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let document = data
        .game()
        .version_path("1.21", axlauncher_storage::VersionPath::Document);
    std::fs::create_dir_all(document.parent().unwrap()).unwrap();
    std::fs::write(document, json).unwrap();
    version::load(data.game(), "1.21").unwrap()
}

#[test]
fn a_document_without_java_version_fetches_nothing() {
    let document = installed_document(r#"{"id":"1.21"}"#);
    let required = missing_java(&document, |_| {
        panic!("a version stating no major must not ask")
    });
    assert_eq!(required, None);
}

#[test]
fn a_major_with_a_runtime_fetches_nothing() {
    let document = installed_document(r#"{"id":"1.21","javaVersion":{"majorVersion":21}}"#);
    let asked = Cell::new(None);
    let required = missing_java(&document, |major| {
        asked.set(Some(major));
        true
    });
    assert_eq!(required, None);
    assert_eq!(asked.get(), Some(21));
}

#[test]
fn a_major_without_a_runtime_asks_for_that_major() {
    let document = installed_document(r#"{"id":"1.21","javaVersion":{"majorVersion":21}}"#);
    assert_eq!(missing_java(&document, |_| false), Some(21));
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
    let initial = discover_with(&data, true, || {
        vec![earlier.clone(), latest.clone(), other.clone()]
    })
    .unwrap();
    assert_eq!(
        initial.preferred().collect::<Vec<_>>(),
        [(42, latest.path()), (43, other.path())]
    );

    set_preferred(&data, 42, earlier.path()).unwrap();
    let repeated = discover_with(&data, false, || vec![latest.clone()]).unwrap();
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
    let startup = discover_with(&data, true, || vec![runtime.clone()]).unwrap();
    assert_eq!(startup.preferred().next(), Some((25, runtime.path())));
    std::fs::remove_file(&path).unwrap();

    let loaded = discover_with(&data, true, || {
        panic!("the unavailable entry is still listed")
    })
    .unwrap();
    assert_eq!(loaded.paths().collect::<Vec<_>>(), [runtime.path()]);
    assert!(loaded.available().is_empty());
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
    let added = discover_with(&data, false, || vec![newer.clone()]).unwrap();
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
    let catalog = discover_with(&data, true, || {
        add(&data, &path).unwrap();
        vec![JavaRuntime::inspect(&discovered).unwrap()]
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

    let found = discover_with(&data, false, || {
        SystemJava::default()
            .with_runtime_roots(vec![data.path(DataPath::JavaVersions).to_owned()])
            .discover()
    })
    .unwrap();
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
    let catalog = discover_with(&data, true, || vec![bundled.clone(), standalone.clone()]).unwrap();
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
