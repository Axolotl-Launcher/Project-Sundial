// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::fs::{self, File};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use axlauncher_core::install::loader;
use axlauncher_core::launch::{Target, plan};
use axlauncher_core::state::profile::{Game, Profile, StagedProfile, discover};
use axlauncher_core::storage::{instance_runtime, write_atomic};
use serde_json::json;
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

use super::*;

fn jar(value: &str) -> Vec<u8> {
    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    archive
        .start_file("example/Game.class", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(value.as_bytes()).unwrap();
    archive.finish().unwrap().into_inner()
}

fn export(path: &Path) {
    let mut archive = ZipWriter::new(File::create(path).unwrap());
    let manifest = json!({"formatVersion":1,"components":[
        {"uid":"net.minecraft","version":"1.20.1"}, {"uid":"example.original"}, {"uid":"example.final"}
    ]});
    let first = json!({"uid":"example.original","mainClass":"example.First",
        "mainJar":{"name":"example:replacement:1","MMC-hint":"local"},
        "libraries":[{"name":"example:library:1","MMC-hint":"local"}],
        "jarMods":[{"name":"example:first:1","MMC-hint":"local"}],
        "requires":[{"uid":"net.minecraft","equals":"1.20.1"}]
    });
    let second = json!({"uid":"example.final","mainClass":"example.Game",
        "jarMods":[{"name":"example:second:1","MMC-hint":"local"}]
    });
    let entries = [
        ("instance.cfg", b"name=Custom fixture".to_vec()),
        ("mmc-pack.json", manifest.to_string().into_bytes()),
        (
            "patches/example.original.json",
            first.to_string().into_bytes(),
        ),
        (
            "patches/example.final.json",
            second.to_string().into_bytes(),
        ),
        ("libraries/replacement-1.jar", jar("main")),
        ("libraries/library-1.jar", jar("library")),
        ("jarmods/first-1.jar", jar("first")),
        ("jarmods/second-1.jar", jar("second")),
    ];
    for (name, bytes) in entries {
        archive
            .start_file(name, SimpleFileOptions::default())
            .unwrap();
        archive.write_all(&bytes).unwrap();
    }
    archive.finish().unwrap();
}

#[test]
fn active_patches_produce_an_isolated_client_and_local_classpath_from_a_real_zip() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("pack.zip");
    export(&path);
    let instances = temporary.path().join("profiles");
    let staging = StagedProfile::new(&instances).unwrap();
    let root = instance_runtime(staging.directory());
    fs::create_dir(&root).unwrap();
    let mut export = MultiMcExport::open(&path).unwrap();
    assert_eq!(export.base_game().unwrap().minecraft(), "1.20.1");
    let patches = read(&mut export).unwrap();
    let mut version: Version =
        serde_json::from_value(json!({"id":"axolotl-converted","jar":"1.20.1"})).unwrap();
    apply(patches, &mut export, &root, &mut version).unwrap();
    assert_eq!(version.jar_id(), version.id);
    assert_eq!(version.main_class, "example.Game");
    assert_eq!(version.libraries[0].name, "example:library:1");
    assert!(
        root.join("libraries/example/library/1/library-1.jar")
            .is_file()
    );
    let client = root.join("versions/axolotl-converted/axolotl-converted.jar");
    let mut archive = ZipArchive::new(File::open(client).unwrap()).unwrap();
    let mut value = String::new();
    archive
        .by_name("example/Game.class")
        .unwrap()
        .read_to_string(&mut value)
        .unwrap();
    assert_eq!(value, "second");
    assert!(!root.join("versions/1.20.1/1.20.1.jar").exists());
    drop(archive);
    assert!(discover(&instances).instances.is_empty());

    let (directory, root) = publish(staging, &version);
    assert_eq!(discover(&instances).instances.len(), 1);
    assert_launch_layout(&directory, &root, &version);
}

fn publish(staging: StagedProfile, version: &Version) -> (PathBuf, PathBuf) {
    let root = instance_runtime(staging.directory());
    write_atomic(
        &loader::document_path(&root, &version.id),
        &serde_json::to_vec(version).unwrap(),
    )
    .unwrap();
    let mut profile = Profile::new(
        "Converted",
        Some(Game::new("1.20.1").with_isolated_runtime()),
    );
    profile.set_installed_version(version.id.clone());
    let directory = staging.publish(&profile).unwrap();
    let root = instance_runtime(&directory);
    loader::check_files(&root, &version.id, &Target::host()).unwrap();
    (directory, root)
}

fn assert_launch_layout(directory: &Path, root: &Path, version: &Version) {
    let layout = plan::Layout {
        game_root: root.into(),
        game_directory: directory.into(),
        natives_directory: root.join("natives"),
        launcher_name: "Axolotl".into(),
        launcher_version: "test".into(),
    };
    let command = plan::build(
        Path::new("java"),
        version,
        &layout,
        &plan::Session::offline("Player"),
        &Target::host(),
    )
    .unwrap();
    let library = root
        .join("libraries/example/library/1/library-1.jar")
        .to_string_lossy()
        .replace('\\', "/");
    assert!(
        command
            .args
            .iter()
            .any(|arg| arg.replace('\\', "/").contains(&library))
    );
    assert_eq!(command.working_directory, directory);
}
