// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::cell::Cell;

use super::*;

fn installed_document(json: &str) -> Version {
    let root = tempfile::tempdir().unwrap();
    let data = DataDir::new(root.path());
    let directory = data.version("1.21");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("1.21.json"), json).unwrap();
    version::load(&data.versions(), "1.21").unwrap()
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

#[test]
fn managed_java_and_environment_share_the_recorded_choice() {
    let root = tempfile::tempdir().unwrap();
    let java = root.path().join("user-java");
    std::fs::write(&java, b"fixture").unwrap();
    javas::choose_if_unset(
        root.path(),
        ChosenJava {
            major_version: 21,
            full_version: "21.0.1".into(),
            architecture: "fixture".into(),
            path: java.display().to_string(),
        },
    )
    .unwrap();

    let environment = JavaEnvironment::load(Some(root.path()));
    assert_eq!(environment.chosen(), &[(21, java.clone())]);
    let provider = managed(&DataDir::new(root.path()));
    assert_eq!(provider.executable_for(21, JavaMode::Console), Some(java));
}
