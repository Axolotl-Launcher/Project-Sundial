// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::fs;
use std::path::Path;

use super::super::SCHEMA_VERSION;
use super::{DiscoveryError, discover};
use crate::test_support::TempRoot;

const ID_A: &str = "b09d9f2d-6351-4602-8f53-74a4dc645837";
const ID_B: &str = "0d94c0a4-4f3e-4b2e-9b3a-2f6a1c7d8e90";
const ID_C: &str = "5f4a3b21-6c7d-4e8f-9a0b-1c2d3e4f5a6b";

fn profile_bytes(id: &str, name: &str) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "id": id,
        "instance": {"name": name}
    }))
    .expect("profile json")
}

fn put(root: &Path, directory: &str, bytes: &[u8]) {
    let instance = root.join(directory);
    fs::create_dir_all(&instance).expect("instance directory");
    fs::write(instance.join(".axlprofile"), bytes).expect("profile");
}

#[test]
fn an_empty_directory_discovers_nothing() {
    let temp = TempRoot::new();

    let found = discover(temp.path());

    assert!(found.instances.is_empty());
    assert!(found.issues.is_empty());
}

#[test]
fn a_missing_profiles_directory_is_an_empty_result() {
    let temp = TempRoot::new();

    let found = discover(&temp.path().join("absent"));

    assert!(found.instances.is_empty());
    assert!(found.issues.is_empty());
}

#[test]
fn one_valid_instance_is_discovered_with_its_directory() {
    let temp = TempRoot::new();
    put(temp.path(), "survival", &profile_bytes(ID_A, "Survival"));

    let found = discover(temp.path());

    assert!(found.issues.is_empty());
    assert_eq!(found.instances.len(), 1);
    assert_eq!(found.instances[0].directory, temp.path().join("survival"));
    assert_eq!(found.instances[0].profile.instance.name, "Survival");
    assert_eq!(found.instances[0].profile.id, ID_A);
}

#[test]
fn a_directory_without_a_profile_document_is_skipped() {
    let temp = TempRoot::new();
    fs::create_dir(temp.path().join("empty")).expect("directory");
    fs::create_dir(temp.path().join("loose")).expect("directory");
    fs::write(temp.path().join("loose").join("notes.txt"), b"hello").expect("note");
    fs::write(temp.path().join("stray.txt"), b"hello").expect("file");

    let found = discover(temp.path());

    assert!(found.instances.is_empty());
    assert!(found.issues.is_empty());
}

#[test]
fn a_malformed_profile_is_reported_rather_than_dropped() {
    let temp = TempRoot::new();
    put(temp.path(), "broken", b"{ not json");
    put(temp.path(), "survival", &profile_bytes(ID_A, "Survival"));

    let found = discover(temp.path());

    assert_eq!(found.instances.len(), 1);
    assert_eq!(found.instances[0].profile.instance.name, "Survival");
    assert_eq!(found.issues.len(), 1);
    assert!(matches!(found.issues[0], DiscoveryError::Parse { .. }));
    assert!(
        found.issues[0].to_string().contains(
            &temp
                .path()
                .join("broken")
                .join(".axlprofile")
                .display()
                .to_string()
        )
    );
}

#[test]
fn an_instance_with_an_unsupported_schema_is_reported() {
    let temp = TempRoot::new();
    let body = serde_json::json!({
        "schema_version": SCHEMA_VERSION + 1,
        "id": ID_A,
        "instance": {"name": "Future"}
    });
    put(
        temp.path(),
        "future",
        &serde_json::to_vec(&body).expect("profile json"),
    );

    let found = discover(temp.path());

    assert!(found.instances.is_empty());
    assert_eq!(found.issues.len(), 1);
    assert!(matches!(
        found.issues[0],
        DiscoveryError::UnsupportedSchema { .. }
    ));
}

#[test]
fn instances_are_ordered_by_visible_name_then_directory() {
    let temp = TempRoot::new();
    put(temp.path(), "zeta", &profile_bytes(ID_B, "Alpha"));
    put(temp.path(), "beta", &profile_bytes(ID_A, "Alpha"));
    put(temp.path(), "alpha", &profile_bytes(ID_C, "Zulu"));

    let found = discover(temp.path());

    assert!(found.issues.is_empty());
    let order: Vec<&str> = found
        .instances
        .iter()
        .map(|entry| entry.directory.file_name().unwrap().to_str().unwrap())
        .collect();
    assert_eq!(order, ["beta", "zeta", "alpha"]);
}
