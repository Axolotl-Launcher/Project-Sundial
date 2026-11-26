// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use crate::test_support::TempRoot;

#[test]
fn registrations_and_preferences_round_trip_separately() {
    let root = TempRoot::new();
    let manual = root.path().join("jdk21/bin/java");
    let other = root.path().join("other-jdk21/bin/java");
    update(root.path(), |javas| {
        javas.add_manual(manual.clone());
        javas.set_chosen(21, Some(manual.clone()));
        javas.set_chosen(21, Some(other.clone()));
        javas.set_profile_preference("profile", Some(manual.clone()));
    })
    .unwrap();

    let loaded = load(root.path()).unwrap();
    assert_eq!(
        loaded.manual_paths().collect::<Vec<_>>(),
        [manual.as_path()]
    );
    assert_eq!(
        loaded.preferred().collect::<Vec<_>>(),
        [(21, other.as_path())]
    );
    assert_eq!(loaded.profile_preference("profile"), Some(manual.as_path()));
}

#[test]
fn removing_a_manual_entry_clears_preferences_for_that_path() {
    let root = TempRoot::new();
    let manual = root.path().join("jdk21/bin/java");
    let other = root.path().join("jdk8/bin/java");
    update(root.path(), |javas| {
        javas.add_manual(manual.clone());
        javas.set_chosen(21, Some(manual.clone()));
        javas.set_chosen(8, Some(other.clone()));
        javas.set_profile_preference("profile", Some(manual.clone()));
        assert!(javas.remove_manual(&manual));
    })
    .unwrap();

    let loaded = load(root.path()).unwrap();
    assert_eq!(loaded.manual_paths().count(), 0);
    assert_eq!(
        loaded.preferred().collect::<Vec<_>>(),
        [(8, other.as_path())]
    );
    assert_eq!(loaded.profile_preference("profile"), None);
}

#[test]
fn concurrent_catalog_edits_preserve_each_registration() {
    let root = TempRoot::new();
    let edits: Vec<_> = (0..8)
        .map(|index| {
            let root = root.path().to_owned();
            std::thread::spawn(move || register_fixture(&root, index))
        })
        .collect();
    for edit in edits {
        edit.join().unwrap();
    }
    assert_eq!(load(root.path()).unwrap().manual_paths().count(), 8);
}

fn register_fixture(root: &Path, index: u32) {
    update(root, |javas| javas.add_manual(root.join(index.to_string()))).unwrap();
}

#[test]
fn another_schema_is_never_replaced() {
    let root = TempRoot::new();
    let path = javas_path(root.path());
    let document = br#"{"schema_version": 9}"#;
    fs::write(&path, document).unwrap();

    let error = update(root.path(), |_| ()).unwrap_err();
    assert!(matches!(error, JavasError::RefusedSchema { found: 9, .. }));
    assert_eq!(fs::read(path).unwrap(), document);
}
