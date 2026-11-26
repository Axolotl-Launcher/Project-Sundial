// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::cell::Cell;

use super::*;

fn installed_document(json: &str) -> Version {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::isolated(root.path()).unwrap();
    let document = data.game().version_document("1.21");
    std::fs::create_dir_all(document.parent().unwrap()).unwrap();
    std::fs::write(document, json).unwrap();
    version::load(&data.game().versions(), "1.21").unwrap()
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
    assert_eq!(
        catalog.manual_paths().collect::<Vec<_>>(),
        [registered.path()]
    );
    assert_eq!(
        managed(&data)
            .executable_for(42, JavaMode::Console)
            .as_deref(),
        Some(registered.path())
    );

    set_preferred(&data, 42, Some(registered.path())).unwrap();
    let environment = JavaEnvironment::load(Some(root.path()));
    assert_eq!(environment.chosen(), &[(42, registered.path().to_owned())]);
    std::fs::remove_file(&path).unwrap();
    let catalog = list(&data).unwrap();
    assert!(!catalog.available().contains(&registered));
    assert_eq!(catalog.manual_paths().count(), 1);
    assert!(remove(&data, registered.path()).unwrap());
    assert_eq!(list(&data).unwrap().preferred().count(), 0);
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
    let err = set_preferred(&data, 43, Some(&path)).unwrap_err();
    assert!(matches!(
        err,
        JavaCatalogError::InvalidRuntime {
            required: Some(43),
            ..
        }
    ));
    let saved = javas::load(data.root()).unwrap();
    assert_eq!(saved.manual_paths().count(), 0);
    assert_eq!(saved.preferred().count(), 0);
}
