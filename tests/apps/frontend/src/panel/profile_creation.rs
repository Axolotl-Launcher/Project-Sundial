// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::{
    AUTOMATIC, CatalogStatus, LoaderCatalog, LoaderKind, LoaderVersion, loader_build_choices,
    minecraft_picker_items,
};
use axlauncher_core::install::vanilla;
use axlauncher_core::spi::{FetchError, Network};
use axlauncher_ui::{SelectDelegate, SelectItem, SharedString};

struct MinecraftCatalogNetwork;

impl Network for MinecraftCatalogNetwork {
    async fn fetch(&self, _: &str) -> Result<Vec<u8>, FetchError> {
        Ok(br#"{
            "latest": {"release": "26.3"},
            "versions": [
                {"id": "26.4-rc-1", "type": "snapshot", "url": "https://example.invalid/rc", "sha1": ""},
                {"id": "26.3", "type": "release", "url": "https://example.invalid/26.3", "sha1": ""},
                {"id": "1.20.1", "type": "release", "url": "https://example.invalid/1.20.1", "sha1": ""}
            ]
        }"#
        .to_vec())
    }
}

#[test]
fn prerelease_labels_do_not_change_the_version_ids_saved_to_the_profile() {
    let catalog = futures::executor::block_on(vanilla::versions(&MinecraftCatalogNetwork)).unwrap();
    let (choices, _) = minecraft_picker_items(&catalog, "26.4-rc-1");
    let index = choices.position(&SharedString::from("26.4-rc-1")).unwrap();
    let choice = choices.item(index).unwrap();
    assert_eq!(choice.title().as_ref(), "26.4-rc-1 · Release candidate");
    assert_eq!(choice.value().as_ref(), "26.4-rc-1");
}

#[test]
fn a_catalog_response_uses_the_manual_game_before_the_latest_release() {
    let catalog = futures::executor::block_on(vanilla::versions(&MinecraftCatalogNetwork)).unwrap();
    for (entered, expected, listed) in [
        ("", "26.3", true),
        (" 1.20.1 ", "1.20.1", true),
        ("unknown", "unknown", false),
    ] {
        let (items, selected) = minecraft_picker_items(&catalog, entered);
        assert_eq!(selected.as_ref(), expected);
        assert_eq!(items.position(&selected).is_some(), listed);
    }
}

#[test]
fn a_late_loader_response_cannot_complete_a_new_selection() {
    let mut catalog = LoaderCatalog::default();
    let first = catalog.restart();
    catalog.status = CatalogStatus::Loading;
    assert!(catalog.accepts(first));

    let second = catalog.restart();
    catalog.status = CatalogStatus::Loading;
    assert!(!catalog.accepts(first));
    assert!(catalog.accepts(second));

    catalog.restart();
    assert!(!catalog.accepts(second));
}

#[test]
fn loader_labels_do_not_change_the_build_ids_saved_to_the_profile() {
    let choices = loader_build_choices(
        Some(LoaderKind::Forge),
        vec![LoaderVersion {
            version: "47.4.28".into(),
            stable: Some(true),
        }],
    );
    let automatic = choices.position(&SharedString::from(AUTOMATIC)).unwrap();
    assert_eq!(
        choices.item(automatic).unwrap().title().as_ref(),
        "Automatic"
    );
    let exact = choices.position(&SharedString::from("47.4.28")).unwrap();
    assert_eq!(
        choices.item(exact).unwrap().title().as_ref(),
        "47.4.28 · Recommended"
    );
    assert_eq!(choices.item(exact).unwrap().value().as_ref(), "47.4.28");
}
