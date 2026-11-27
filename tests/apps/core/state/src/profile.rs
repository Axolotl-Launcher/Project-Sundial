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
fn profile_creation_uses_one_managed_directory_per_profile() {
    let temp = TempRoot::new();
    let instances = temp.path().join("instances");
    let vanilla = create(&instances, "  Survival  ", Game::new(" 1.21.11 ")).unwrap();
    let modded = create(
        &instances,
        "Survival",
        Game::new("1.21.11").with_loader("Fabric", Some("0.18.0".into())),
    )
    .unwrap();

    assert_ne!(vanilla, modded);
    assert_eq!(fs::read_dir(&vanilla).unwrap().count(), 1);
    let profile = read(&vanilla).unwrap().unwrap();
    assert_eq!(vanilla, instances.join(&profile.id));
    assert_eq!(profile.instance.name, "Survival");
    assert_eq!(profile.install.status, InstallStatus::NotInstalled);
    assert!(profile.content.files.is_empty());
    assert_eq!(profile.game.unwrap().effective_version(), Some("1.21.11"));
    let game = read(&modded).unwrap().unwrap().game.unwrap();
    assert_eq!(game.loader.unwrap().version.as_deref(), Some("0.18.0"));
    assert_eq!(discover(&instances).instances.len(), 2);
}

#[test]
fn a_historical_catalog_version_keeps_its_spaces_in_the_profile() {
    let temp = TempRoot::new();
    let id = "1.14 Pre-Release 1";
    let directory = create(temp.path(), "Pre-release", Game::new(id)).unwrap();
    let profile = read(&directory).unwrap().unwrap();
    assert_eq!(profile.game().unwrap().effective_version(), Some(id));
}

#[test]
fn a_memory_override_roundtrips_and_following_global_removes_it() {
    let temp = TempRoot::new();
    let directory = create(temp.path(), "Survival", Game::new("1.21.11")).unwrap();
    let profile = read(&directory).unwrap().unwrap();
    let heap = MaximumHeap::new(6144).unwrap();
    let start = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        scope.spawn(|| {
            start.wait();
            set_memory(&directory, profile.id(), Some(heap)).unwrap();
        });
        scope.spawn(|| {
            start.wait();
            mark_installed(&directory, "1.21.11").unwrap();
        });
    });
    let saved = read(&directory).unwrap().unwrap();
    assert_eq!(saved.memory(), Some(heap));
    assert_eq!(saved.install.status, InstallStatus::Ready);
    let raw = fs::read(Document::Profile.path(&directory)).unwrap();
    let document: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(document["memory"], serde_json::json!({"maximum_mib": 6144}));
    assert!(set_memory(&directory, "other-id", None).is_err());
    assert_eq!(read(&directory).unwrap().unwrap().memory(), Some(heap));
    set_memory(&directory, profile.id(), None).unwrap();
    let raw = fs::read(Document::Profile.path(&directory)).unwrap();
    let document: serde_json::Value = serde_json::from_slice(&raw).unwrap();
    assert!(document.get("memory").is_none());
    assert_eq!(read(&directory).unwrap().unwrap().memory(), None);
}

#[test]
fn an_invalid_heap_override_does_not_hide_the_instance() {
    let temp = TempRoot::new();
    let mut body: serde_json::Value = serde_json::from_slice(MINIMAL).unwrap();
    body["memory"] = serde_json::json!({"maximum_mib": 1});
    let bytes = serde_json::to_vec(&body).unwrap();
    put(temp.path(), "instance/.axlprofile", &bytes);

    let discovered = discover(temp.path());
    assert!(discovered.issues.is_empty());
    assert_eq!(discovered.instances.len(), 1);
    let instance = &discovered.instances[0];
    assert_eq!(instance.profile.id(), ID);
    assert_eq!(instance.profile.memory(), None);
    assert_eq!(
        fs::read(Document::Profile.path(&instance.directory)).unwrap(),
        bytes
    );
}

#[test]
fn invalid_input_never_creates_an_instance_directory() {
    let temp = TempRoot::new();
    let instances = temp.path().join("instances");
    for (name, version) in [
        (" \t ", "1.21.11"),
        ("Example", "../outside"),
        ("Example", "1.21?invalid"),
        ("Example", "NUL.json"),
        ("Example", " "),
    ] {
        assert!(matches!(
            create(&instances, name, Game::new(version)),
            Err(ProfileWriteError::Invalid { .. })
        ));
    }
    assert!(!instances.exists());
}

#[test]
fn effective_version_uses_the_recorded_id_or_a_vanilla_fallback() {
    let vanilla = Game::new("1.21.11").with_loader(" vAnIlLa ", None);
    assert!(vanilla.is_vanilla());
    assert_eq!(vanilla.effective_version(), Some("1.21.11"));
    let mut modded = Game::new("1.21.11").with_loader("fabric", None);
    assert!(!modded.is_vanilla());
    assert_eq!(modded.effective_version(), None);
    modded.version_id = Some("fabric-custom".into());
    assert_eq!(modded.effective_version(), Some("fabric-custom"));
}

#[test]
fn state_updates_only_the_install_result_on_the_current_profile() {
    let temp = TempRoot::new();
    let directory = create(temp.path(), "Survival", Game::new("1.21.11")).unwrap();
    let mut edited = read(&directory).unwrap().unwrap();
    edited.instance.name = "Edited while installing".into();
    save(&directory, &edited).unwrap();

    mark_installed(&directory, "1.21.11").unwrap();
    let ready = read(&directory).unwrap().unwrap();
    assert_eq!(ready.instance.name, "Edited while installing");
    assert_eq!(ready.install.status, InstallStatus::Ready);
    assert_eq!(ready.game.unwrap().version_id.as_deref(), Some("1.21.11"));

    mark_failed(&directory).unwrap();
    let failed = read(&directory).unwrap().unwrap();
    assert_eq!(failed.install.status, InstallStatus::Failed);
    assert_eq!(failed.game.unwrap().version_id.as_deref(), Some("1.21.11"));
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
    put(
        temp.path(),
        ".axlprofile",
        &serde_json::to_vec(&body).unwrap(),
    );

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
        put(
            temp.path(),
            ".axlprofile",
            &serde_json::to_vec(&body).unwrap(),
        );
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
    put(temp.path(), ".axlprofile", body.as_bytes());

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
    assert!(!root.join(".axlprofile").exists());
}

#[test]
fn a_new_profile_round_trips_through_save_and_read() {
    let temp = TempRoot::new();
    let directory = temp.path().join("survival");
    let profile = Profile::new("Survival", Some(Game::new("1.21.11")));

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

    mark_failed(&directory).expect("record failure");

    let written: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.join(".axlprofile")).expect("bytes"))
            .expect("json");
    assert_eq!(written["future"]["nested"], true);
    assert_eq!(written["instance"]["accent"], "green");
    assert_eq!(written["install"]["status"], "failed");
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
