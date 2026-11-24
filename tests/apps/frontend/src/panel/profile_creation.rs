// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::{AUTOMATIC, CatalogStatus, LoaderCatalog, LoaderVersion, loader_build_choices};
use axlauncher_ui::{SelectDelegate, SelectItem, SharedString};

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
    let choices = loader_build_choices(vec![LoaderVersion {
        version: "47.4.28".into(),
        stable: Some(true),
    }]);
    let automatic = choices.position(&SharedString::from(AUTOMATIC)).unwrap();
    assert_eq!(
        choices.item(automatic).unwrap().title().as_ref(),
        "Automatic"
    );
    let exact = choices.position(&SharedString::from("47.4.28")).unwrap();
    assert_eq!(
        choices.item(exact).unwrap().title().as_ref(),
        "47.4.28 · Stable"
    );
    assert_eq!(choices.item(exact).unwrap().value().as_ref(), "47.4.28");
}
