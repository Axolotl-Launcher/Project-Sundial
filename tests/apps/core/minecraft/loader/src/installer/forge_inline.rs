// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::io::{Cursor, Write};

use axlauncher_spi::{LoaderError, LoaderKind};
use serde_json::{Value, json};

use super::parse;

const UNIVERSAL: &str = "net.minecraftforge:minecraftforge:9.11.1.1345";
const UNIVERSAL_PATH: &str =
    "net/minecraftforge/minecraftforge/9.11.1.1345/minecraftforge-9.11.1.1345.jar";

fn installer(coordinate: &str, parent: Option<&str>) -> Vec<u8> {
    installer_from(coordinate, parent, "http://files.minecraftforge.net/maven/")
}

fn installer_from(coordinate: &str, parent: Option<&str>, repository: &str) -> Vec<u8> {
    let mut profile = json!({
        "install": {
            "minecraft": "1.6.4",
            "filePath": "/forge-universal.jar",
            "path": coordinate,
        },
        "versionInfo": {
            "id": "1.6.4-Forge9.11.1.1345",
            "mainClass": "net.minecraft.launchwrapper.Launch",
            "minecraftArguments": "--session ${auth_session} --tweakClass cpw.mods.fml.common.launcher.FMLTweaker",
            "libraries": [
                {"name": coordinate, "url": "http://files.minecraftforge.net/maven/"},
                {"name": "net.minecraft:launchwrapper:1.8"},
                {"name": "org.ow2.asm:asm-all:4.1"},
                {"name": "jline:jline:2.13", "url": "http://files.minecraftforge.net/maven/"},
                {"name": "net.minecraftforge:example:1", "url": repository, "checksums": ["a9993e364706816aba3e25717850c26c9cd0d89d", "43ad1705f06e8b0d18583bd70a2f721ae9ec93a8", "invalid"]},
                {"name": "org.lwjgl.lwjgl:lwjgl-platform:2.9.0", "natives": {"windows": "natives-windows-${arch}"}},
            ],
        },
    });
    if let Some(parent) = parent {
        profile["versionInfo"]["inheritsFrom"] = json!(parent);
    }
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    writer
        .start_file("install_profile.json", options)
        .expect("profile entry");
    writer
        .write_all(profile.to_string().as_bytes())
        .expect("write profile");
    writer
        .start_file("forge-universal.jar", options)
        .expect("universal entry");
    writer.write_all(b"universal").expect("write universal");
    writer.finish().expect("installer").into_inner()
}

#[test]
fn an_inline_forge_profile_inherits_vanilla_and_carries_the_universal_jar() {
    let profile = parse(LoaderKind::Forge, &installer(UNIVERSAL, None), "1.6.4").expect("profile");

    assert_eq!(profile.version_id, "1.6.4-Forge9.11.1.1345");
    assert_eq!(profile.files.len(), 1);
    assert_eq!(profile.files[0].path, format!("libraries/{UNIVERSAL_PATH}"));
    assert_eq!(profile.files[0].bytes, b"universal");
    let document: Value = serde_json::from_slice(&profile.document).expect("document");
    assert_eq!(document["inheritsFrom"], "1.6.4");
    assert_eq!(document["mainClass"], "net.minecraft.launchwrapper.Launch");
    assert_eq!(
        document["libraries"][0]["downloads"]["artifact"]["path"],
        UNIVERSAL_PATH
    );
    assert_eq!(document["libraries"][0]["downloads"]["artifact"]["url"], "");
}

#[test]
fn an_inline_forge_profile_resolves_legacy_dependencies_and_natives() {
    let profile = parse(LoaderKind::Forge, &installer(UNIVERSAL, None), "1.6.4").expect("profile");
    let document: Value = serde_json::from_slice(&profile.document).expect("document");
    let libraries = document["libraries"].as_array().expect("libraries");
    let url = |index: usize| &libraries[index]["downloads"]["artifact"]["url"];

    assert_eq!(
        url(1),
        "https://libraries.minecraft.net/net/minecraft/launchwrapper/1.8/launchwrapper-1.8.jar"
    );
    assert_eq!(
        url(2),
        "https://repo.maven.apache.org/maven2/org/ow2/asm/asm-all/4.1/asm-all-4.1.jar"
    );
    assert_eq!(
        url(3),
        "https://repo.maven.apache.org/maven2/jline/jline/2.13/jline-2.13.jar"
    );
    assert_eq!(
        url(4),
        "https://maven.minecraftforge.net/net/minecraftforge/example/1/example-1.jar"
    );
    assert_eq!(
        libraries[4]["downloads"]["artifact"]["sha1_alternatives"],
        json!([
            "a9993e364706816aba3e25717850c26c9cd0d89d",
            "43ad1705f06e8b0d18583bd70a2f721ae9ec93a8"
        ])
    );
    let classifiers = &libraries[5]["downloads"]["classifiers"];
    assert_eq!(
        classifiers["natives-windows-64"]["path"],
        "org/lwjgl/lwjgl/lwjgl-platform/2.9.0/lwjgl-platform-2.9.0-natives-windows-64.jar"
    );
    assert!(classifiers["natives-windows-32"]["url"].is_string());
    assert!(libraries[5]["downloads"].get("artifact").is_none());
}

#[test]
fn legacy_forge_refuses_unmapped_plain_http_repositories() {
    let bytes = installer_from(UNIVERSAL, None, "http://example.invalid/maven");
    let error = parse(LoaderKind::Forge, &bytes, "1.6.4").unwrap_err();
    assert!(error.to_string().contains("non-HTTPS"), "{error}");
}

#[test]
fn an_inline_forge_profile_rejects_mismatched_parents_and_unsafe_coordinates() {
    for (coordinate, parent, game) in [
        (UNIVERSAL, Some("1.6.2"), "1.6.4"),
        (UNIVERSAL, None, "1.7.10"),
        ("net.minecraftforge:../forge:1", None, "1.6.4"),
    ] {
        let error =
            parse(LoaderKind::Forge, &installer(coordinate, parent), game).expect_err("malformed");
        assert!(matches!(error, LoaderError::Malformed { .. }), "{error}");
    }
}
