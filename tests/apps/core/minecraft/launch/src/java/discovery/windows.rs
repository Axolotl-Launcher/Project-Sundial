// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use crate::TestTempDir;

use super::*;

#[test]
fn exploration_follows_runtime_and_version_names_to_the_depth_limit() {
    let root = TestTempDir::new();
    let found = fake_java(
        root.path()
            .join("Minecraft/runtime/amd64/21.0.11/Temurin/1/bin"),
    );
    fake_java(
        root.path()
            .join("Minecraft/runtime/amd64/21.0.11/Temurin/1/2/bin"),
    );
    fake_java(root.path().join("Documents/runtime/bin"));
    fake_java(root.path().join("NetEase Java/Temurin/bin"));

    assert_eq!(explore([root.path().to_owned()]), [found]);
}

#[test]
fn fixed_drive_seeds_use_installation_keywords() {
    assert!(is_installation_root(Path::new(r"D:\Eclipse Temurin")));
    assert!(is_installation_root(Path::new(r"D:\Java")));
    assert!(!is_installation_root(Path::new(r"D:\Runtime")));
    assert!(may_descend("runtime"));
    assert!(may_descend("21.0.11_4"));
    assert!(!may_descend("java8path"));
}

#[test]
fn program_files_searches_unrecognized_vendors_without_crossing_its_depth_limit() {
    let root = TestTempDir::new();
    let programs = dunce::canonicalize(root.path())
        .unwrap()
        .join("Program Files");
    let java = fake_java(programs.join("Acme/Java/jdk-21/bin"));
    fake_java(programs.join("Acme/Java/Deep/jdk-25/bin"));
    fake_java(programs.join("WpSystem/Java/jdk-25/bin"));
    let mut found = Vec::new();

    scan_program_files(&[programs], &mut found);

    assert_eq!(found, [java]);
}

#[test]
fn the_store_layout_accepts_runtime_components_on_non_wpsystem_paths() {
    let root = TestTempDir::new();
    let found = fake_java(
        root.path()
            .join("java-runtime-epsilon/windows-x64/runtime/bin"),
    );
    fake_java(root.path().join("unrelated/windows-x64/runtime/bin"));
    let excluded = root.path().join("WpSystem/runtime");
    fake_java(excluded.join("java-runtime-epsilon/windows-x64/runtime/bin"));

    assert_eq!(store_executables(root.path()), [found]);
    assert!(store_executables(&excluded).is_empty());
}

fn fake_java(bin: PathBuf) -> PathBuf {
    fs::create_dir_all(&bin).unwrap();
    fs::write(
        bin.parent().unwrap().join("release"),
        "JAVA_VERSION=\"21\"\n",
    )
    .unwrap();
    let path = bin.join("java.exe");
    fs::write(&path, b"fixture").unwrap();
    dunce::canonicalize(path).unwrap()
}
