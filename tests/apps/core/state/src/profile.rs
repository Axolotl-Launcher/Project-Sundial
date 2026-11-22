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
