// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::{BTreeMap, BTreeSet};

use axlauncher_spi::LoaderKind;
use serde_json::{Value, json};

use crate::MultiMcExport;

use super::{
    InstallPlan, ModpackPlugin, PackError, PackSource, archive_with_marker, local_plan, tempfile,
};

fn components(extra: impl IntoIterator<Item = Value>) -> Value {
    let mut result = vec![json!({"uid": "net.minecraft", "version": "1.20.1", "important": true})];
    result.extend(extra);
    json!({"formatVersion": 1, "components": result})
}

fn bundled(plan: &InstallPlan) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    plan.write_bundled_files(|path, reader| {
        let mut content = String::new();
        reader.read_to_string(&mut content)?;
        files.insert(path.to_owned(), content);
        Ok(())
    })
    .unwrap();
    files
}

#[test]
fn a_wrapped_multimc_pack_uses_the_common_plan_for_game_files() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("export.zip");
    let manifest = components([
        json!({"uid": "net.fabricmc.fabric-loader", "version": "0.17.0"}),
        json!({"uid": "net.fabricmc.intermediary", "version": "1.20.1", "dependencyOnly": true}),
        json!({"uid": "org.lwjgl3", "version": "3.3.1", "dependencyOnly": true}),
        json!({"uid": "example.disabled", "disabled": true}),
    ]);
    archive_with_marker(
        &path,
        "Wrapped/mmc-pack.json",
        &manifest,
        &[
            (
                "Wrapped/instance.cfg",
                "name=Example \\#1 # comment\r\nPreLaunchCommand=ignored\r\nJavaPath=ignored",
            ),
            ("Wrapped/.minecraft/mods/mod.jar.disabled", "bundled mod"),
            ("Wrapped/.minecraft/saves/World/level.dat", "world data"),
            (
                "Wrapped/minecraft/saves/Other/level.dat",
                "ignore the other game directory",
            ),
            ("Wrapped/patches/example.disabled.json", "ignored patch"),
            ("Wrapped/pack.png", "instance metadata"),
        ],
    );
    let pack = ModpackPlugin::builtin().inspect(&path).unwrap();
    assert_eq!(pack.source(), PackSource::MultiMc);
    assert_eq!(pack.name(), "Example #1");
    assert_eq!(pack.game().minecraft(), "1.20.1");
    assert_eq!(pack.game().loader(), Some((LoaderKind::Fabric, "0.17.0")));
    let plan = local_plan(&pack, &BTreeSet::new());
    assert!(plan.downloads().is_empty());
    assert_eq!(
        bundled(&plan),
        BTreeMap::from([
            ("mods/mod.jar.disabled".into(), "bundled mod".into()),
            ("saves/World/level.dat".into(), "world data".into()),
        ])
    );
}

#[test]
fn root_exports_accept_the_minecraft_directory_and_supported_loaders() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("export.zip");
    for (uid, kind) in [
        ("net.minecraftforge", LoaderKind::Forge),
        ("net.neoforged", LoaderKind::NeoForge),
        ("org.quiltmc.quilt-loader", LoaderKind::Quilt),
    ] {
        archive_with_marker(
            &path,
            "mmc-pack.json",
            &components([json!({"uid": uid, "version": "0.25.0"})]),
            &[
                ("instance.cfg", "InstanceType=OneSix"),
                ("minecraft/config/example.txt", "config"),
            ],
        );
        let pack = ModpackPlugin::builtin().inspect(&path).unwrap();
        assert_eq!(pack.game().loader(), Some((kind, "0.25.0")));
        let plan = local_plan(&pack, &BTreeSet::new());
        assert_eq!(
            bundled(&plan),
            BTreeMap::from([("config/example.txt".into(), "config".into())])
        );
    }
}

#[test]
fn active_mmc_patches_and_local_launch_artifacts_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("export.zip");
    for artifact in [
        "patches/net.minecraft.json",
        "libraries/custom.jar",
        "jarmods/custom.jar",
    ] {
        archive_with_marker(
            &path,
            "mmc-pack.json",
            &components([]),
            &[
                ("instance.cfg", "name=Pack"),
                (artifact, "local launch data"),
            ],
        );
        let result = ModpackPlugin::builtin().inspect(&path);
        assert!(
            matches!(result, Err(PackError::Unsupported(message)) if message.contains(artifact))
        );
    }
}

#[test]
fn excessive_combined_patch_size_is_rejected_before_invoking_the_reader() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("export.zip");
    let manifest = components(["one", "two", "three"].map(|uid| json!({"uid": uid})));
    let patch = format!("{{}}{}", " ".repeat(6 * 1024 * 1024));
    archive_with_marker(
        &path,
        "mmc-pack.json",
        &manifest,
        &[
            ("instance.cfg", "name=Large patches"),
            ("patches/one.json", &patch),
            ("patches/two.json", &patch),
            ("patches/three.json", &patch),
        ],
    );
    let mut export = MultiMcExport::open(&path).unwrap();
    let result = export.read_patches(|_, _| panic!("patch budget must be checked first"));
    assert!(
        matches!(result, Err(PackError::Invalid(message)) if message.contains("total size limit"))
    );
}

#[test]
fn unsupported_loaders_and_multiple_roots_do_not_leave_a_plan() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("export.zip");
    let manifest = components([json!({"uid": "example.unknown", "version": "1.0"})]);
    archive_with_marker(
        &path,
        "mmc-pack.json",
        &manifest,
        &[("instance.cfg", "name=Pack")],
    );
    assert!(
        matches!(ModpackPlugin::builtin().inspect(&path), Err(PackError::Unsupported(message)) if message.contains("example.unknown"))
    );

    let stock = components([]);
    archive_with_marker(
        &path,
        "mmc-pack.json",
        &stock,
        &[
            ("instance.cfg", "name=Pack"),
            ("Other/instance.cfg", "name=Other"),
            ("Other/mmc-pack.json", &stock.to_string()),
        ],
    );
    assert!(matches!(
        ModpackPlugin::builtin().inspect(&path),
        Err(PackError::AmbiguousFormat)
    ));
}

#[test]
fn unrelated_nested_manifests_and_reserved_output_paths_are_handled() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("export.zip");
    archive_with_marker(
        &path,
        "mmc-pack.json",
        &components([]),
        &[
            ("instance.cfg", "name=Pack"),
            (".minecraft/mods/Nested/mmc-pack.json", "non-manifest data"),
            (".minecraft/mods/Nested/instance.cfg", "non-instance data"),
        ],
    );
    let pack = ModpackPlugin::builtin().inspect(&path).unwrap();
    assert_eq!(pack.bundled_count(), 2);

    archive_with_marker(
        &path,
        "mmc-pack.json",
        &components([]),
        &[
            ("instance.cfg", "name=Pack"),
            (".minecraft/.axlprofile", "foreign marker"),
        ],
    );
    assert!(matches!(
        ModpackPlugin::builtin().inspect(&path),
        Err(PackError::Invalid(_))
    ));
}
