// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use crate::test_support::TempRoot;

fn write_manifest(root: &Path, id: &str, manifest: &str) {
    let directory = root.join(id);
    fs::create_dir_all(&directory).expect("instance directory");
    fs::write(directory.join(MANIFEST), manifest).expect("manifest");
}

#[test]
fn a_missing_root_is_an_empty_list() {
    let temp = TempRoot::new();
    let missing = temp.path().join("does-not-exist");
    assert_eq!(discover(&missing).expect("discovery"), Vec::new());
}

#[test]
fn reads_a_manifest_and_uses_the_directory_as_id() {
    let temp = TempRoot::new();
    write_manifest(
        temp.path(),
        "survival",
        r#"{"schema_version":1,"name":"Survival","game_version":"1.21","loader":"fabric"}"#,
    );

    assert_eq!(
        discover(temp.path()).expect("discovery"),
        vec![{
            let mut instance = Instance::new("survival", "Survival");
            instance.game_version = Some("1.21".into());
            instance.loader = Some("fabric".into());
            instance
        }]
    );
}

#[test]
fn a_directory_without_a_manifest_is_not_an_instance() {
    let temp = TempRoot::new();
    fs::create_dir_all(temp.path().join("scratch")).expect("directory");
    assert_eq!(discover(temp.path()).expect("discovery"), Vec::new());
}

#[test]
fn sorts_by_name_then_id() {
    let temp = TempRoot::new();
    write_manifest(temp.path(), "b", r#"{"schema_version":1,"name":"Alpha"}"#);
    write_manifest(temp.path(), "c", r#"{"schema_version":1,"name":"Alpha"}"#);
    write_manifest(temp.path(), "a", r#"{"schema_version":1,"name":"Zulu"}"#);

    let ids: Vec<String> = discover(temp.path())
        .expect("discovery")
        .into_iter()
        .map(|instance| instance.id)
        .collect();
    assert_eq!(ids, ["b", "c", "a"]);
}

#[test]
fn a_malformed_manifest_is_reported_with_its_path() {
    let temp = TempRoot::new();
    write_manifest(temp.path(), "broken", "{ not json }");

    let error = discover(temp.path()).expect_err("must fail");
    assert!(matches!(error, DiscoveryError::Parse { .. }));
    assert!(error.to_string().contains("broken"));
}

#[test]
fn a_newer_schema_is_rejected_rather_than_misread() {
    let temp = TempRoot::new();
    write_manifest(
        temp.path(),
        "future",
        r#"{"schema_version":2,"name":"Future"}"#,
    );

    let error = discover(temp.path()).expect_err("must fail");
    assert!(matches!(
        error,
        DiscoveryError::UnsupportedSchema { found: 2, .. }
    ));
}

#[test]
fn a_manifest_without_a_name_is_rejected() {
    let temp = TempRoot::new();
    write_manifest(temp.path(), "nameless", r#"{"schema_version":1}"#);
    assert!(matches!(
        discover(temp.path()).expect_err("must fail"),
        DiscoveryError::Parse { .. }
    ));
}

fn sample(id: &str, name: &str) -> Instance {
    let mut instance = Instance::new(id, name);
    instance.game_version = Some("1.21".into());
    instance.loader = Some("fabric".into());
    instance
}

#[test]
fn create_writes_a_manifest_that_discovery_finds() {
    let temp = TempRoot::new();

    create(temp.path(), &sample("survival", "Survival")).expect("create");

    assert_eq!(
        discover(temp.path()).expect("discovery"),
        vec![sample("survival", "Survival")]
    );
}

#[test]
fn create_trims_the_name_and_blank_optional_fields() {
    let temp = TempRoot::new();
    let mut instance = sample("padded", "  Padded  ");
    instance.loader = Some("   ".into());

    create(temp.path(), &instance).expect("create");

    let found = discover(temp.path()).expect("discovery");
    assert_eq!(found[0].name, "Padded");
    assert_eq!(found[0].loader, None);
}

#[test]
fn create_refuses_an_existing_directory() {
    let temp = TempRoot::new();
    fs::create_dir(temp.path().join("taken")).expect("directory");

    let error = create(temp.path(), &sample("taken", "Taken")).expect_err("must refuse");

    assert!(matches!(error, InstanceError::AlreadyExists { .. }));
}

#[test]
fn ids_that_could_leave_the_root_are_rejected() {
    let temp = TempRoot::new();

    for id in ["", "..", "a/b", "a\\b", "UPPER", "with space"] {
        let error = create(temp.path(), &sample(id, "Bad")).expect_err(id);
        assert!(matches!(error, InstanceError::InvalidId { .. }), "{id}");
    }
}

#[test]
fn an_empty_name_is_rejected() {
    let temp = TempRoot::new();

    let error = create(temp.path(), &sample("ok", "   ")).expect_err("must refuse");

    assert!(matches!(error, InstanceError::InvalidName { .. }));
}

#[test]
fn save_rewrites_the_manifest() {
    let temp = TempRoot::new();
    create(temp.path(), &sample("survival", "Survival")).expect("create");
    let mut renamed = sample("survival", "Renamed");
    renamed.loader = None;

    save(temp.path(), &renamed).expect("save");

    assert_eq!(discover(temp.path()).expect("discovery"), vec![renamed]);
}

#[test]
fn save_refuses_a_missing_instance() {
    let temp = TempRoot::new();

    let error = save(temp.path(), &sample("absent", "Absent")).expect_err("must fail");

    assert!(matches!(error, InstanceError::NotFound { .. }));
}

#[test]
fn save_refuses_another_schema_and_leaves_it_alone() {
    let temp = TempRoot::new();
    let directory = temp.path().join("future");
    fs::create_dir_all(&directory).expect("directory");
    let manifest = directory.join(MANIFEST);
    fs::write(&manifest, r#"{"schema_version":2,"name":"Future"}"#).expect("seed");
    let before = fs::read(&manifest).expect("before");

    let error = save(temp.path(), &sample("future", "Renamed")).expect_err("must refuse");

    assert!(matches!(
        error,
        InstanceError::RefusedSchema { found: 2, .. }
    ));
    assert_eq!(fs::read(&manifest).expect("after"), before);
}

#[test]
fn save_refuses_an_unreadable_manifest_and_leaves_it_alone() {
    let temp = TempRoot::new();
    let directory = temp.path().join("broken");
    fs::create_dir_all(&directory).expect("directory");
    let manifest = directory.join(MANIFEST);
    fs::write(&manifest, "{ broken").expect("seed");
    let before = fs::read(&manifest).expect("before");

    let error = save(temp.path(), &sample("broken", "Broken")).expect_err("must refuse");

    assert!(matches!(error, InstanceError::RefusedUnreadable { .. }));
    assert_eq!(fs::read(&manifest).expect("after"), before);
}

#[test]
fn a_manifest_from_the_earlier_build_still_parses() {
    let temp = TempRoot::new();
    write_manifest(
        temp.path(),
        "old",
        r#"{"schema_version":1,"name":"Old","game_version":"1.20.1"}"#,
    );

    let found = discover(temp.path()).expect("discovery");
    let instance = &found[0];

    assert_eq!(instance.name, "Old");
    assert_eq!(instance.install_stage, InstallStage::NotInstalled);
    assert_eq!(instance.created, 0);
    assert!(instance.link.is_none());
}

#[test]
fn the_kept_fields_round_trip_through_a_save() {
    let temp = TempRoot::new();
    let mut instance = sample("kept", "Kept");
    instance.created = 1_700_000_000_000;
    instance.modified = 1_700_000_001_000;
    instance.last_played = Some(1_700_000_002_000);
    instance.submitted_time_played = 3_600_000;
    instance.recent_time_played = 60_000;
    instance.install_stage = InstallStage::Installed;
    instance.icon_path = Some("icon.png".into());
    instance.link = Some(InstanceLink {
        kind: "modrinth_project".into(),
        project_id: Some("abc".into()),
        version_id: None,
    });

    create(temp.path(), &instance).expect("create");
    let found = discover(temp.path()).expect("discovery");

    assert_eq!(found, vec![instance]);
}

#[test]
fn a_ready_instance_is_told_apart_from_a_half_installed_one() {
    assert!(InstallStage::Installed.is_ready());
    assert!(InstallStage::PackInstalled.is_ready());
    assert!(!InstallStage::MinecraftInstalling.is_ready());
    assert!(!InstallStage::NotInstalled.is_ready());
}
