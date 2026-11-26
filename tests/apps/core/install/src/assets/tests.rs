// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use std::collections::BTreeMap;

use axlauncher_launch::version::{Artifact, LibraryDownloads, OsRule, Rule};

use crate::test_tempfile as tempfile;

/// SHA-1 of `abc`, as `sha1sum` reports it.
const ABC_SHA1: &str = "a9993e364706816aba3e25717850c26c9cd0d89d";
const ABC: &[u8] = b"abc";

fn target() -> Target {
    Target {
        os: "windows".into(),
        arch: "x86_64".into(),
    }
}

fn artifact(path: &str) -> Artifact {
    Artifact {
        path: path.into(),
        sha1: Some(ABC_SHA1.into()),
        sha1_alternatives: Vec::new(),
        size: Some(ABC.len() as u64),
        url: Some(format!("https://example.invalid/{path}")),
    }
}

#[test]
fn a_local_only_jar_is_required_for_launch_but_is_not_downloaded() {
    let root = tempfile::tempdir().unwrap();
    let library: Library = serde_json::from_value(serde_json::json!({
        "name": "example:embedded:1",
        "downloads": {"artifact": {"path": "example/embedded.jar", "url": "", "size": 3}}
    }))
    .unwrap();
    let libraries = [library];
    assert!(missing_libraries(&GameDir::standalone(root.path()), &libraries, &target()).is_empty());
    assert_eq!(
        missing_library_files(
            &GameDir::standalone(root.path()),
            &libraries,
            &target(),
            true
        )
        .len(),
        1
    );
    write_file(root.path(), "libraries/example/embedded.jar", b"a");
    assert_eq!(
        missing_library_files(
            &GameDir::standalone(root.path()),
            &libraries,
            &target(),
            true
        )
        .len(),
        1
    );
    write_file(root.path(), "libraries/example/embedded.jar", ABC);
    assert!(
        missing_library_files(
            &GameDir::standalone(root.path()),
            &libraries,
            &target(),
            true
        )
        .is_empty()
    );
}

fn library(artifact: Artifact, rules: Option<Vec<Rule>>) -> Library {
    Library {
        name: "org.example:example:1.0".into(),
        downloads: Some(LibraryDownloads {
            artifact: Some(artifact),
            classifiers: None,
        }),
        natives: None,
        rules,
        url: None,
        include_in_classpath: true,
    }
}

fn write_file(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

#[test]
fn missing_libraries_reports_an_absent_artifact() {
    let root = tempfile::tempdir().unwrap();
    let library = library(artifact("org/example/a.jar"), None);

    let missing = missing_libraries(&GameDir::standalone(root.path()), &[library], &target());

    assert_eq!(missing.len(), 1);
    assert_eq!(
        missing[0].path,
        root.path().join("libraries/org/example/a.jar")
    );
    assert_eq!(missing[0].url, "https://example.invalid/org/example/a.jar");
    assert_eq!(missing[0].sha1.as_deref(), Some(ABC_SHA1));
    assert_eq!(missing[0].size, Some(ABC.len() as u64));
}

#[test]
fn missing_classifiers_go_to_the_hash_cache() {
    use axlauncher_storage::{DataDir, Directory};

    let data = tempfile::tempdir().unwrap();
    let mut paths = DataDir::isolated(data.path()).unwrap();
    let cache = data.path().join("custom-cache");
    paths
        .set_directory(Directory::AppCache, Some(&cache))
        .unwrap();
    let game_root = paths.game();
    let mut library = library(artifact("org/example/example-1.0.jar"), None);
    library.natives = Some(BTreeMap::from([(
        "windows".into(),
        "natives-windows".into(),
    )]));
    let classifier = artifact("org/example/example-1.0-natives-windows.jar");
    library.downloads.as_mut().unwrap().classifiers =
        Some(BTreeMap::from([("natives-windows".into(), classifier)]));

    let files = missing_libraries(&game_root, &[library], &target());

    assert_eq!(files.len(), 2);
    assert_eq!(
        files[1].path,
        cache
            .join("minecraft-natives")
            .join(format!("{ABC_SHA1}.jar"))
    );
}

#[test]
fn missing_libraries_reports_an_unverified_artifact() {
    let root = tempfile::tempdir().unwrap();
    let mut unverified = artifact("org/example/a.jar");
    unverified.sha1 = None;
    let library = library(unverified, None);

    let missing = missing_libraries(&GameDir::standalone(root.path()), &[library], &target());

    assert_eq!(missing.len(), 1);
    assert_eq!(missing[0].sha1, None);
}

#[test]
fn missing_libraries_skips_a_correct_file() {
    let root = tempfile::tempdir().unwrap();
    write_file(root.path(), "libraries/org/example/a.jar", ABC);
    let library = library(artifact("org/example/a.jar"), None);

    assert!(missing_libraries(&GameDir::standalone(root.path()), &[library], &target()).is_empty());
}

#[test]
fn missing_libraries_reports_a_wrong_size() {
    let root = tempfile::tempdir().unwrap();
    write_file(root.path(), "libraries/org/example/a.jar", b"four");
    let library = library(artifact("org/example/a.jar"), None);

    let missing = missing_libraries(&GameDir::standalone(root.path()), &[library], &target());

    assert_eq!(missing.len(), 1);
    assert_eq!(
        missing[0].path,
        root.path().join("libraries/org/example/a.jar")
    );
}

#[test]
fn a_library_the_processors_produce_is_not_scheduled_for_download() {
    // A loader's patched client jar declares its path and hash but no URL: its
    // processors write it, and planning a fetch for it fails every install.
    let root = tempfile::tempdir().unwrap();
    let mut produced = artifact("net/minecraftforge/forge/patch-client.jar");
    produced.url = Some(String::new());
    let library = library(produced, None);

    assert!(missing_libraries(&GameDir::standalone(root.path()), &[library], &target()).is_empty());
}

#[test]
fn a_library_path_that_leaves_the_game_root_is_dropped() {
    let root = tempfile::tempdir().unwrap();
    let library = library(artifact("../../escaped.jar"), None);

    assert!(missing_libraries(&GameDir::standalone(root.path()), &[library], &target()).is_empty());
}

#[test]
fn missing_libraries_ignores_a_library_excluded_for_the_target() {
    let root = tempfile::tempdir().unwrap();
    let only_macos = vec![Rule {
        action: "allow".into(),
        features: std::collections::BTreeMap::new(),
        os: Some(OsRule {
            name: Some("osx".into()),
            arch: None,
        }),
    }];
    let library = library(artifact("org/example/a.jar"), Some(only_macos));

    assert!(missing_libraries(&GameDir::standalone(root.path()), &[library], &target()).is_empty());
}

#[test]
fn asset_index_parses_the_objects_map() {
    let json = br#"{"objects":{"minecraft/sounds/a.ogg":{"hash":"0123","size":7}}}"#;

    let document = AssetIndexDocument::parse(json).unwrap();

    let object = &document.objects["minecraft/sounds/a.ogg"];
    assert_eq!(object.hash, "0123");
    assert_eq!(object.size, 7);
}

#[test]
fn missing_assets_uses_the_two_character_fan_out() {
    let root = tempfile::tempdir().unwrap();
    let hash = "0123456789abcdef0123456789abcdef01234567";
    let index = AssetIndexDocument {
        objects: BTreeMap::from([(
            "minecraft/sounds/a.ogg".to_owned(),
            AssetObject {
                hash: hash.into(),
                size: ABC.len() as u64,
            },
        )]),
        ..AssetIndexDocument::default()
    };

    let missing = missing_assets(&GameDir::standalone(root.path()), &index);

    assert_eq!(missing.len(), 1);
    assert_eq!(
        missing[0].path,
        root.path().join(format!("assets/objects/01/{hash}"))
    );
    assert_eq!(
        missing[0].url,
        format!("https://resources.download.minecraft.net/01/{hash}")
    );
    assert_eq!(missing[0].sha1.as_deref(), Some(hash));
    assert_eq!(missing[0].size, Some(ABC.len() as u64));
}

#[test]
fn missing_assets_skips_a_correct_object() {
    let root = tempfile::tempdir().unwrap();
    let hash = "0123456789abcdef0123456789abcdef01234567";
    write_file(root.path(), &format!("assets/objects/01/{hash}"), ABC);
    let index = AssetIndexDocument {
        objects: BTreeMap::from([(
            "minecraft/sounds/a.ogg".to_owned(),
            AssetObject {
                hash: hash.into(),
                size: ABC.len() as u64,
            },
        )]),
        ..AssetIndexDocument::default()
    };

    assert!(missing_assets(&GameDir::standalone(root.path()), &index).is_empty());
}

#[test]
fn a_virtual_index_copies_objects_into_its_flat_tree() {
    let root = tempfile::tempdir().unwrap();
    let hash = "0123456789abcdef0123456789abcdef01234567";
    write_file(root.path(), &format!("assets/objects/01/{hash}"), ABC);
    let index = AssetIndexDocument {
        objects: BTreeMap::from([(
            "minecraft/sounds/a.ogg".to_owned(),
            AssetObject {
                hash: hash.into(),
                size: ABC.len() as u64,
            },
        )]),
        virtual_map: true,
        ..AssetIndexDocument::default()
    };

    let failures = materialise_flat(&GameDir::standalone(root.path()), "legacy", &index);

    assert!(failures.is_empty(), "{failures:?}");
    let flat = root
        .path()
        .join("assets/virtual/legacy/minecraft/sounds/a.ogg");
    assert_eq!(fs::read(&flat).expect("flat copy"), ABC);
}

#[test]
fn a_map_to_resources_index_copies_into_resources() {
    let root = tempfile::tempdir().unwrap();
    let hash = "0123456789abcdef0123456789abcdef01234567";
    write_file(root.path(), &format!("assets/objects/01/{hash}"), ABC);
    let index = AssetIndexDocument {
        objects: BTreeMap::from([(
            "sound/a.ogg".to_owned(),
            AssetObject {
                hash: hash.into(),
                size: ABC.len() as u64,
            },
        )]),
        map_to_resources: true,
        ..AssetIndexDocument::default()
    };

    materialise_flat(&GameDir::standalone(root.path()), "pre-1.6", &index);

    let flat = root.path().join("resources").join("sound").join("a.ogg");
    assert_eq!(fs::read(&flat).expect("flat copy"), ABC);
}

#[test]
fn a_modern_index_asks_for_no_flat_tree() {
    let root = tempfile::tempdir().unwrap();
    let index = AssetIndexDocument::default();

    assert!(materialise_flat(&GameDir::standalone(root.path()), "34", &index).is_empty());
    assert!(!root.path().join("assets/virtual").exists());
    assert!(!root.path().join("resources").exists());
}

#[test]
fn a_flat_tree_is_not_rebuilt_when_it_is_current() {
    let root = tempfile::tempdir().unwrap();
    let hash = "0123456789abcdef0123456789abcdef01234567";
    write_file(root.path(), &format!("assets/objects/01/{hash}"), ABC);
    write_file(root.path(), "assets/virtual/legacy/a.ogg", ABC);
    let index = AssetIndexDocument {
        objects: BTreeMap::from([(
            "a.ogg".to_owned(),
            AssetObject {
                hash: hash.into(),
                size: ABC.len() as u64,
            },
        )]),
        virtual_map: true,
        ..AssetIndexDocument::default()
    };

    materialise_flat(&GameDir::standalone(root.path()), "legacy", &index);

    // The destination is untouched: it already had the right size, so a
    // second pass did not need to read the object at all.
    let flat = root.path().join("assets/virtual/legacy/a.ogg");
    assert_eq!(fs::read(&flat).expect("flat copy"), ABC);
}
