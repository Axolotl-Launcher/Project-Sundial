// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::fs;
use std::path::Path;

use super::{ResourceKind, scan_resources};
use crate::test_support::TempRoot;

fn put(root: &Path, relative: &str, bytes: &[u8]) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    fs::write(path, bytes).expect("fixture");
}

fn inventory(root: &Path) -> Vec<(String, ResourceKind, Option<bool>)> {
    scan_resources(root)
        .expect("scan")
        .into_iter()
        .map(|resource| (resource.relative_path, resource.kind, resource.enabled))
        .collect()
}

#[test]
fn an_empty_game_directory_has_no_resources() {
    let temp = TempRoot::new();

    let resources = scan_resources(temp.path()).expect("scan");

    assert!(resources.is_empty());
}

#[test]
fn mods_and_packs_keep_their_kind_and_enabled_state() {
    let temp = TempRoot::new();
    put(temp.path(), "mods/sodium.jar", b"mod");
    put(temp.path(), "mods/old.jar.disabled", b"mod");
    put(temp.path(), "mods/notes.txt", b"ignored");
    put(temp.path(), "resourcepacks/pack.zip", b"pack");
    put(temp.path(), "shaderpacks/shiny.zip.disabled", b"shader");
    put(temp.path(), "datapacks/data.ZIP", b"data");

    let kind =
        |name: &str, kind: ResourceKind, enabled: bool| (name.to_owned(), kind, Some(enabled));
    assert_eq!(
        inventory(temp.path()),
        [
            kind("datapacks/data.ZIP", ResourceKind::DataPack, true),
            kind("mods/old.jar.disabled", ResourceKind::Mod, false),
            kind("mods/sodium.jar", ResourceKind::Mod, true),
            kind("resourcepacks/pack.zip", ResourceKind::ResourcePack, true),
            kind(
                "shaderpacks/shiny.zip.disabled",
                ResourceKind::ShaderPack,
                false
            ),
        ]
    );
}

#[test]
fn schematics_recurse_and_need_a_known_suffix() {
    let temp = TempRoot::new();
    put(temp.path(), "schematics/house.litematic", b"schematic");
    put(temp.path(), "schematics/nested/castle.schem", b"schematic");
    put(temp.path(), "schematics/readme.txt", b"ignored");

    let inventory = inventory(temp.path());

    assert_eq!(inventory.len(), 2);
    assert!(inventory.contains(&(
        "schematics/house.litematic".to_owned(),
        ResourceKind::Schematic,
        Some(true)
    )));
    assert!(inventory.contains(&(
        "schematics/nested/castle.schem".to_owned(),
        ResourceKind::Schematic,
        Some(true)
    )));
}

#[test]
fn a_world_is_recognised_by_its_level_file() {
    let temp = TempRoot::new();
    put(temp.path(), "saves/Overworld/level.dat", b"level");
    fs::create_dir_all(temp.path().join("saves").join("Empty")).expect("directory");
    fs::create_dir_all(temp.path().join("saves").join("NotAWorld")).expect("directory");
    fs::write(
        temp.path().join("saves").join("NotAWorld").join("icon.png"),
        b"icon",
    )
    .expect("icon");

    let inventory = inventory(temp.path());

    assert_eq!(
        inventory,
        [("saves/Overworld".to_owned(), ResourceKind::World, None)]
    );
}

#[test]
fn screenshots_are_png_files_without_an_enabled_state() {
    let temp = TempRoot::new();
    put(temp.path(), "screenshots/2026-01-01_12.00.00.png", b"image");
    put(temp.path(), "screenshots/notes.txt", b"ignored");

    let inventory = inventory(temp.path());

    assert_eq!(
        inventory,
        [(
            "screenshots/2026-01-01_12.00.00.png".to_owned(),
            ResourceKind::Screenshot,
            None
        )]
    );
}

#[test]
fn the_inventory_reports_each_file_size_and_sorts_by_path() {
    let temp = TempRoot::new();
    put(temp.path(), "mods/zeta.jar", b"12345");
    put(temp.path(), "mods/alpha.jar", b"1");
    put(temp.path(), "screenshots/a.png", b"123");

    let resources = scan_resources(temp.path()).expect("scan");

    let paths: Vec<&str> = resources
        .iter()
        .map(|resource| resource.relative_path.as_str())
        .collect();
    assert_eq!(
        paths,
        ["mods/alpha.jar", "mods/zeta.jar", "screenshots/a.png"]
    );
    assert_eq!(resources[0].size_bytes, Some(1));
    assert_eq!(resources[1].size_bytes, Some(5));
    assert_eq!(resources[2].size_bytes, Some(3));
}

#[test]
fn datapacks_inside_a_world_are_listed_beneath_it() {
    let temp = TempRoot::new();
    put(temp.path(), "saves/Overworld/level.dat", b"level");
    put(
        temp.path(),
        "saves/Overworld/datapacks/terrain.zip",
        b"datapack",
    );

    let inventory = inventory(temp.path());

    assert!(inventory.contains(&("saves/Overworld".to_owned(), ResourceKind::World, None)));
    assert!(inventory.contains(&(
        "saves/Overworld/datapacks/terrain.zip".to_owned(),
        ResourceKind::DataPack,
        Some(true)
    )));
}

#[test]
fn a_root_that_is_not_a_directory_is_refused() {
    let temp = TempRoot::new();
    put(temp.path(), "mods/sodium.jar", b"mod");

    let error = scan_resources(&temp.path().join("mods").join("sodium.jar")).expect_err("refused");

    assert_eq!(error.kind(), std::io::ErrorKind::NotADirectory);
}

#[test]
fn a_missing_root_is_an_error_rather_than_an_empty_inventory() {
    let temp = TempRoot::new();

    let error = scan_resources(&temp.path().join("absent")).expect_err("refused");

    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
}
