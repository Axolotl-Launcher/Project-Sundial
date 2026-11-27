// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use axlauncher_launch::version::{DataEntry, Processor};
use std::collections::BTreeMap;

fn version(data: BTreeMap<String, DataEntry>) -> Version {
    Version {
        id: "1.20.2-neoforge".into(),
        kind: "release".into(),
        main_class: "cpw.mods.bootstraplauncher.BootstrapLauncher".into(),
        libraries: Vec::new(),
        arguments: None,
        minecraft_arguments: None,
        asset_index: None,
        assets: None,
        java_version: None,
        inherits_from: None,
        jar: Some("1.20.2".into()),
        downloads: None,
        logging: None,
        processors: Vec::<Processor>::new(),
        data,
    }
}

fn entry(value: &str) -> DataEntry {
    DataEntry {
        client: value.into(),
        server: value.into(),
    }
}

#[test]
fn a_coordinate_maps_to_its_maven_path() {
    assert_eq!(
        axlauncher_launch::version::maven_path("net.neoforged:neoform:1.20.2@zip").expect("path"),
        Path::new("net/neoforged/neoform/1.20.2/neoform-1.20.2.zip")
    );
}

#[test]
fn a_bracketed_argument_becomes_a_library_path() {
    let libraries = Path::new("libs");
    let version = version(BTreeMap::new());

    let resolved = resolve_template(
        "[net.neoforged:neoform:1.20.2@zip]",
        libraries,
        &version,
        &[],
    )
    .expect("resolved");

    assert!(resolved.ends_with("neoform-1.20.2.zip"), "{resolved}");
}

#[test]
fn launcher_values_are_available_to_a_step() {
    let mut version = version(BTreeMap::new());
    version.jar = None;
    let values = launcher_data(
        &version,
        Path::new("data/meta"),
        Path::new("data/profiles/NeoForge"),
        "1.20.2",
        Path::new("data/meta/libraries"),
    );
    let value = |name| {
        values
            .iter()
            .find(|(key, _)| *key == name)
            .unwrap()
            .1
            .as_str()
    };

    assert_eq!(value(SIDE), "client");
    assert_eq!(value(MINECRAFT_VERSION), "1.20.2");
    assert_eq!(Path::new(value(ROOT)), Path::new("data/profiles/NeoForge"));
    assert_eq!(
        Path::new(value(LIBRARY_DIR)),
        Path::new("data/meta/libraries")
    );
    assert_eq!(
        Path::new(value(MINECRAFT_JAR)),
        Path::new("data/meta/versions/1.20.2-neoforge/1.20.2-neoforge.jar")
    );
}

#[test]
fn a_quoted_value_is_a_literal_not_a_path() {
    let version = version(BTreeMap::from([("VERSION".into(), entry("'1.20.2'"))]));

    let resolved =
        resolve_template("{VERSION}", Path::new("libs"), &version, &[]).expect("resolved");

    assert_eq!(resolved, "1.20.2");
}
