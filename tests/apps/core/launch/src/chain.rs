// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use crate::version::{Argument, Arguments};
use std::collections::BTreeMap;

fn minimal(id: &str) -> Version {
    Version {
        id: id.into(),
        kind: "release".into(),
        main_class: "net.minecraft.client.main.Main".into(),
        libraries: Vec::new(),
        arguments: None,
        minecraft_arguments: None,
        asset_index: None,
        assets: None,
        java_version: None,
        inherits_from: None,
        jar: None,
        downloads: None,
        logging: None,
        processors: Vec::new(),
        data: BTreeMap::default(),
    }
}

fn plain(argument: &str) -> Argument {
    Argument::Plain(argument.into())
}

#[test]
fn a_field_the_child_omits_is_the_parents() {
    let mut parent = minimal("1.20.1");
    parent.assets = Some("5".into());

    let child = minimal("1.20.1-forge");

    let merged = merge(parent, child);
    assert_eq!(merged.assets.as_deref(), Some("5"));
    assert_eq!(merged.id, "1.20.1-forge");
}

#[test]
fn a_field_the_child_states_replaces_the_parents() {
    let mut parent = minimal("1.20.1");
    parent.main_class = "net.minecraft.client.main.Main".into();

    let mut child = minimal("1.20.1-forge");
    child.main_class = "cpw.mods.bootstraplauncher.BootstrapLauncher".into();

    let merged = merge(parent, child);
    assert_eq!(
        merged.main_class,
        "cpw.mods.bootstraplauncher.BootstrapLauncher"
    );
}

#[test]
fn libraries_keep_the_parent_first_and_the_child_after() {
    let mut parent = minimal("1.20.1");
    parent.libraries = vec![crate::version::Library {
        name: "org.example:vanilla:1".into(),
        downloads: None,
        natives: None,
        rules: None,
        url: None,
        include_in_classpath: true,
    }];
    let mut child = minimal("1.20.1-forge");
    child.libraries = vec![crate::version::Library {
        name: "org.example:loader:1".into(),
        downloads: None,
        natives: None,
        rules: None,
        url: None,
        include_in_classpath: true,
    }];

    let merged = merge(parent, child);
    let names: Vec<&str> = merged
        .libraries
        .iter()
        .map(|library| library.name.as_str())
        .collect();
    assert_eq!(names, ["org.example:vanilla:1", "org.example:loader:1"]);
}

#[test]
fn arguments_are_appended_rather_than_replaced() {
    let mut parent = minimal("1.20.1");
    parent.arguments = Some(Arguments {
        jvm: vec![plain("-cp")],
        game: vec![plain("--username")],
        default_user_jvm: Vec::new(),
    });
    let mut child = minimal("1.20.1-forge");
    child.arguments = Some(Arguments {
        jvm: vec![plain("-Dloader=1")],
        game: vec![plain("--fml.neoForgeVersion")],
        default_user_jvm: Vec::new(),
    });

    let merged = merge(parent, child);
    let arguments = merged.arguments.expect("arguments");
    assert_eq!(arguments.jvm.len(), 2);
    assert_eq!(arguments.game.len(), 2);
}

#[test]
fn a_resolved_document_no_longer_names_a_parent() {
    let mut parent = minimal("1.20.1");
    parent.jar = None;
    let mut child = minimal("1.20.1-forge");
    child.inherits_from = Some("1.20.1".into());
    child.jar = None;

    let merged = merge(parent, child);
    assert!(merged.inherits_from.is_none());
}

#[test]
fn the_jar_a_child_names_wins_over_the_parents() {
    let mut parent = minimal("1.20.1");
    parent.jar = Some("1.20.1".into());
    let mut child = minimal("1.20.1-forge");
    child.jar = Some("1.20.1-forge".into());

    let merged = merge(parent, child);
    assert_eq!(merged.jar.as_deref(), Some("1.20.1-forge"));
}
