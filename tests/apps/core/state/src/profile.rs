// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use crate::test_support::TempRoot;

const ID: &str = "b09d9f2d-6351-4602-8f53-74a4dc645837";
const MINIMAL: &[u8] = br#"{
    "schema_version": 1,
    "id": "b09d9f2d-6351-4602-8f53-74a4dc645837",
    "instance": {"name": "Survival"}
}"#;

fn put(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    fs::write(path, bytes).expect("fixture");
}

#[test]
fn blank_instances_round_trip_without_downloaded_content() {
    let temp = TempRoot::new();
    let instances = temp.path().join("instances");
    let blank = create_blank(&instances, "  空白实例  ", None).unwrap();
    let vanilla = create_blank(&instances, "空白实例", Some(" 1.21.11 ")).unwrap();

    assert_ne!(blank, vanilla);
    assert_eq!(fs::read_dir(&blank).unwrap().count(), 1);
    let blank = read(&blank).unwrap().unwrap();
    assert_eq!(blank.instance.name, "空白实例");
    assert_eq!(blank.install.status, InstallStatus::NotInstalled);
    assert!(blank.game.is_none());
    assert!(blank.content.files.is_empty());
    let version = read(&vanilla).unwrap().unwrap().game.unwrap();
    assert_eq!(version.minecraft, "1.21.11");
    assert!(version.loader.is_none());
    assert_eq!(discover(&instances).instances.len(), 2);
}

#[test]
fn invalid_input_never_creates_an_instance_directory() {
    let temp = TempRoot::new();
    let instances = temp.path().join("instances");
    assert!(matches!(
        create_blank(&instances, " \t ", None),
        Err(CreateError::InvalidName)
    ));
    assert!(matches!(
        create_blank(&instances, "Example", Some("../outside")),
        Err(CreateError::InvalidVersion)
    ));
    assert!(!instances.exists());
}

#[test]
fn discovery_uses_only_direct_profile_markers_and_retains_good_instances() {
    let temp = TempRoot::new();
    let root = temp.path();
    put(root, "valid/.axlprofile", MINIMAL);
    put(root, "broken/.axlprofile", b"{");
    put(root, "future/.axlprofile", br#"{"schema_version": 2}"#);
    put(root, "nested/child/.axlprofile", MINIMAL);
    put(
        root,
        "old/instance.json",
        br#"{"schema_version":1,"name":"Old"}"#,
    );

    let found = discover(root);

    assert_eq!(found.instances.len(), 1);
    assert_eq!(found.instances[0].directory, root.join("valid"));
    assert_eq!(found.instances[0].profile.id, ID);
    assert_eq!(found.issues.len(), 2);
    assert!(
        found
            .issues
            .iter()
            .any(|error| matches!(error, DiscoveryError::UnsupportedSchema { found: 2, .. }))
    );
    assert_eq!(fs::read(root.join("valid/.axlprofile")).unwrap(), MINIMAL);
    assert!(root.join("old/instance.json").is_file());
}

#[test]
fn moving_a_profile_keeps_its_id_and_copying_it_reports_both_paths() {
    let temp = TempRoot::new();
    let root = temp.path();
    put(root, "before/.axlprofile", MINIMAL);
    fs::rename(root.join("before"), root.join("after")).expect("rename");
    put(root, "copy/.axlprofile", MINIMAL);

    let found = discover(root);

    assert_eq!(found.instances.len(), 2);
    assert!(found.instances.iter().all(|entry| entry.profile.id == ID));
    assert_eq!(found.instances[0].directory, root.join("after"));
    assert_eq!(found.instances[1].directory, root.join("copy"));
    assert_eq!(found.issues.len(), 1);
    assert!(found.issues[0].to_string().contains("profile ID also used"));
}

#[test]
fn profile_metadata_includes_playtime_and_download_information() {
    let temp = TempRoot::new();
    let mut body: serde_json::Value = serde_json::from_slice(MINIMAL).unwrap();
    body["game"] = serde_json::json!({"minecraft":"26.2","version_id":"26.2-0.19.5","loader":{"kind":"fabric","version":"0.19.5"}});
    body["install"] = serde_json::json!({"status":"ready"});
    body["playtime"] = serde_json::json!({"total_seconds":18342,"session_count":12});
    body["content"] = serde_json::json!({"files":{"mods/example.jar":{
        "size_bytes":3, "hashes":{"sha1":"a9993e364706816aba3e25717850c26c9cd0d89d"},
        "source":{"provider":"modrinth","project_id":"project","version_id":"version"},
        "download_cache":{"url":"https://cdn.example.org/example.jar"}
    }}});
    put(temp.path(), FILE_NAME, &serde_json::to_vec(&body).unwrap());

    let profile = read(temp.path()).unwrap().unwrap();
    assert_eq!(profile.install.status, InstallStatus::Ready);
    assert_eq!(profile.playtime.total_seconds, 18342);
    assert_eq!(
        profile.game.unwrap().version_id.as_deref(),
        Some("26.2-0.19.5")
    );
    let file = &profile.content.files["mods/example.jar"];
    assert_eq!(file.source.as_ref().unwrap().provider, "modrinth");
    assert_eq!(
        file.download_cache.as_ref().unwrap().url,
        "https://cdn.example.org/example.jar"
    );
}

#[test]
fn content_paths_cannot_escape_the_profile_or_record_a_disabled_suffix() {
    let temp = TempRoot::new();
    for path in [
        "../example.jar",
        "C:/example.jar",
        "mods/example.jar.disabled",
    ] {
        let mut body: serde_json::Value = serde_json::from_slice(MINIMAL).unwrap();
        body["content"] = serde_json::json!({"files":{(path):{"size_bytes":3,"hashes":{}}}});
        put(temp.path(), FILE_NAME, &serde_json::to_vec(&body).unwrap());
        assert!(
            matches!(
                read(temp.path()),
                Err(DiscoveryError::InvalidManifest { .. })
            ),
            "{path}"
        );
    }
}

#[test]
fn hand_edited_duplicate_content_keys_are_reported() {
    let temp = TempRoot::new();
    let header = std::str::from_utf8(MINIMAL).unwrap().trim_end_matches('}');
    let file = r#"{"size_bytes":3,"hashes":{}}"#;
    let body =
        format!(r#"{header},"content":{{"files":{{"mods/a.jar":{file},"mods/a.jar":{file}}}}}}}"#);
    put(temp.path(), FILE_NAME, body.as_bytes());

    let error = read(temp.path()).expect_err("duplicate key");
    assert!(matches!(error, DiscoveryError::Parse { .. }));
    assert!(error.to_string().contains("duplicate content path"));
}

#[test]
fn local_resources_follow_old_directory_rules_and_the_actual_file_name() {
    let temp = TempRoot::new();
    let root = temp.path();
    for path in [
        "mods/a.jar",
        "mods/nested/ignored.jar",
        "resourcepacks/pack.ZIP",
        "resourcepacks/folder/pack.mcmeta",
        "shaderpacks/pack.zip",
        "shaderpacks/pack.zip.txt",
        "datapacks/pack.zip.disabled",
        "saves/World/datapacks/a.zip",
        "saves/World/level.dat",
        "schematics/nested/a.NBT",
        "screenshots/game.png",
        "plugins/ignored.jar",
        "options.txt",
    ] {
        put(root, path, b"data");
    }
    let initial = scan_resources(root).unwrap();
    assert_eq!(initial.len(), 8);
    assert!(
        initial
            .iter()
            .any(|file| file.relative_path == "mods/a.jar" && file.enabled == Some(true))
    );

    fs::rename(root.join("mods/a.jar"), root.join("mods/a.jar.disabled")).unwrap();
    put(root, "mods/new.jar", b"new");
    let rescanned = scan_resources(root).unwrap();

    assert_eq!(rescanned.len(), 9);
    assert!(
        rescanned
            .iter()
            .any(|file| file.relative_path == "mods/a.jar.disabled" && file.enabled == Some(false))
    );
    assert!(
        rescanned
            .iter()
            .any(|file| file.relative_path == "mods/new.jar" && file.enabled == Some(true))
    );
    assert!(
        rescanned
            .iter()
            .any(|file| file.relative_path == "saves/World" && file.kind == ResourceKind::World)
    );
    assert!(!root.join(FILE_NAME).exists());
}

#[test]
fn a_new_profile_round_trips_through_save_and_read() {
    let temp = TempRoot::new();
    let directory = temp.path().join("survival");
    let mut profile = Profile::new("Survival", Some(Game::new("1.21.11")));
    profile.set_installed_version("fabric-loader-0.20.1-1.21.11");
    profile.set_install_status(InstallStatus::Ready);

    save(&directory, &profile).expect("save");
    let loaded = read(&directory).expect("read").expect("present");

    assert_eq!(loaded, profile);
    assert!(loaded.id.chars().all(|c| c.is_ascii_hexdigit() || c == '-'));
}

#[test]
fn a_loader_selection_is_written_with_the_game() {
    let temp = TempRoot::new();
    let directory = temp.path().join("fabric");
    let game = Game::new("1.21.11").with_loader("fabric", Some("0.20.1".into()));
    let profile = Profile::new("Fabric", Some(game));

    save(&directory, &profile).expect("save");
    let loaded = read(&directory).expect("read").expect("present");
    let game = loaded.game.expect("game");
    let loader = game.loader.expect("loader");

    assert_eq!(loader.kind, "fabric");
    assert_eq!(loader.version.as_deref(), Some("0.20.1"));
}

#[test]
fn keys_this_build_does_not_know_survive_a_rewrite() {
    let temp = TempRoot::new();
    let directory = temp.path().join("survival");
    fs::create_dir_all(&directory).expect("directory");
    let original = format!(
        r#"{{"schema_version":1,"id":"{ID}","instance":{{"name":"Survival","accent":"green"}},"future":{{"nested":true}}}}"#
    );
    put(&directory, ".axlprofile", original.as_bytes());

    let mut profile = read(&directory).expect("read").expect("present");
    profile.set_install_status(InstallStatus::Ready);
    save(&directory, &profile).expect("save");

    let written: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.join(".axlprofile")).expect("bytes"))
            .expect("json");
    assert_eq!(written["future"]["nested"], true);
    assert_eq!(written["instance"]["accent"], "green");
    assert_eq!(written["install"]["status"], "ready");
}

#[test]
fn another_schema_is_refused_and_left_alone() {
    let temp = TempRoot::new();
    let directory = temp.path().join("future");
    put(&directory, ".axlprofile", br#"{"schema_version": 2}"#);

    let error = save(&directory, &Profile::new("Future", None)).expect_err("refused");

    assert!(matches!(
        error,
        ProfileWriteError::RefusedSchema { found: 2, .. }
    ));
    assert_eq!(
        fs::read(directory.join(".axlprofile")).expect("bytes"),
        br#"{"schema_version": 2}"#
    );
}

#[test]
fn an_unreadable_marker_is_refused_rather_than_replaced() {
    let temp = TempRoot::new();
    let directory = temp.path().join("broken");
    put(&directory, ".axlprofile", b"{ not a document");

    let error = save(&directory, &Profile::new("Broken", None)).expect_err("refused");

    assert!(matches!(error, ProfileWriteError::RefusedMarker { .. }));
    assert_eq!(
        fs::read(directory.join(".axlprofile")).expect("bytes"),
        b"{ not a document"
    );
}

#[test]
fn a_profile_that_breaks_the_document_rules_is_refused() {
    let temp = TempRoot::new();
    let directory = temp.path().join("bad");

    let error = save(&directory, &Profile::new("   ", None)).expect_err("invalid");

    assert!(matches!(error, ProfileWriteError::Invalid { .. }));
    assert!(!directory.join(".axlprofile").exists());
}
