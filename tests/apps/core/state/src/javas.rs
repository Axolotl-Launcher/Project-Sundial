// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use crate::test_support::TempRoot;

fn chosen(major: u32, path: &str) -> ChosenJava {
    ChosenJava {
        major_version: major,
        full_version: format!("{major}.0.1"),
        architecture: "x86_64".into(),
        path: path.into(),
    }
}

fn discovered(major: u32) -> DiscoveredJava {
    DiscoveredJava {
        major_version: major,
        full_version: format!("{major}.0.1"),
        architecture: "x86_64".into(),
        file_size: 0,
        file_mtime_ms: 0,
    }
}

#[test]
fn a_choice_replaces_the_previous_one_for_its_major() {
    let mut javas = Javas::new();
    javas.choose(chosen(21, "/old/bin/java"));
    javas.choose(chosen(21, "/new/bin/java"));

    assert_eq!(javas.chosen().len(), 1);
    assert_eq!(javas.chosen_for(21).expect("chosen").path, "/new/bin/java");
}

#[test]
fn a_managed_runtime_cannot_replace_an_existing_choice() {
    let root = TempRoot::new();
    let user = chosen(21, "/user/jdk/bin/java");
    let managed = chosen(17, "/managed/jdk17/bin/java");
    choose_if_unset(root.path(), user.clone()).expect("record choice");
    choose_if_unset(root.path(), chosen(21, "/managed/jdk21/bin/java")).expect("keep choice");
    choose_if_unset(root.path(), managed.clone()).expect("record missing major");

    let loaded = load(root.path()).expect("read choices").javas;
    assert_eq!(loaded.chosen_for(21), Some(&user));
    assert_eq!(loaded.chosen_for(17), Some(&managed));
}

#[test]
fn choices_and_discovery_round_trip_independently() {
    let root = TempRoot::new();
    let mut javas = Javas::new();
    javas.choose(chosen(21, "/opt/java21/bin/java"));
    javas.discover("C:/java8/bin/java.exe", discovered(8));
    save(root.path(), &javas).expect("save");

    let loaded = load(root.path()).expect("load");
    assert_eq!(loaded.origin, JavasOrigin::Stored);
    assert_eq!(
        loaded.javas.chosen_for(21).expect("chosen").path,
        "/opt/java21/bin/java"
    );
    assert!(
        loaded
            .javas
            .discovered()
            .contains_key("C:/java8/bin/java.exe")
    );
}

#[test]
fn a_cache_entry_whose_file_changed_is_not_usable() {
    let root = TempRoot::new();
    let java = root.path().join("bin").join("java");
    fs::create_dir_all(java.parent().expect("parent")).expect("dir");
    fs::write(&java, b"binary").expect("java");

    let metadata = fs::metadata(&java).expect("metadata");
    let mut record = discovered(21);
    record.file_size = metadata.len();
    record.file_mtime_ms = modification_ms(&metadata).expect("mtime");

    let mut javas = Javas::new();
    javas.discover(&java.display().to_string(), record);
    assert_eq!(javas.usable(21).count(), 1);

    fs::write(&java, b"a different binary").expect("replace");
    assert_eq!(
        javas.usable(21).count(),
        0,
        "a changed size invalidates the cache"
    );
}

#[test]
fn another_schema_is_read_free_and_never_replaced() {
    let root = TempRoot::new();
    let document = br#"{"schema_version": 9}"#;
    fs::write(root.path().join("javas.json"), document).expect("seed");

    let loaded = load(root.path()).expect("load");
    assert_eq!(loaded.origin, JavasOrigin::ReadOnlyOtherSchema { found: 9 });

    let error = save(root.path(), &Javas::new()).expect_err("refused");
    assert!(matches!(error, JavasError::RefusedSchema { found: 9, .. }));
}
