// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use axlauncher_core::spi::{FetchError, LoaderError, LoaderKind, LoaderVersion};

use super::*;

fn offer(version: &str, stable: Option<bool>) -> LoaderVersion {
    LoaderVersion {
        version: version.into(),
        stable,
    }
}

#[test]
fn the_first_stable_entry_is_the_recommendation_the_providers_resolve() {
    let versions = [
        offer("0.19.6", Some(false)),
        offer("0.19.5", Some(true)),
        offer("0.18.0", Some(true)),
    ];

    assert_eq!(
        recommended(&versions).map(|offer| offer.version.as_str()),
        Some("0.19.5")
    );
    let items = version_items(&versions);
    assert!(items[0].recommended);
    assert_eq!(items[0].title().as_ref(), "0.19.5 (recommended)");
    assert!(!items[1].recommended);
    assert!(!items[2].recommended);
}

#[test]
fn the_recommendation_is_the_first_row_and_the_rest_keep_provider_order() {
    let versions = [
        offer("0.19.6", Some(false)),
        offer("0.19.5", Some(true)),
        offer("0.18.0", Some(true)),
    ];

    let titles: Vec<String> = version_items(&versions)
        .iter()
        .map(|item| item.title().to_string())
        .collect();

    assert_eq!(
        titles,
        vec!["0.19.5 (recommended)", "0.19.6", "0.18.0"],
        "the recommendation leads; the others stay newest first"
    );
}

#[test]
fn a_listing_with_no_stable_entry_recommends_its_first_entry() {
    let versions = [offer("0.19.6", Some(false)), offer("0.19.5", None)];

    assert_eq!(
        recommended(&versions).map(|offer| offer.version.as_str()),
        Some("0.19.6")
    );
    assert!(version_items(&versions)[0].recommended);
    assert!(recommended(&[]).is_none());
    assert!(version_items(&[]).is_empty());
}

#[test]
fn the_recommended_choice_asks_the_provider_for_its_own_pick() {
    let versions = [offer("0.19.6", Some(false)), offer("0.19.5", Some(true))];

    assert_eq!(requested_version(&versions, Some("0.19.5")), None);
    assert_eq!(
        requested_version(&versions, Some("0.19.6")),
        Some("0.19.6".into())
    );
    assert_eq!(requested_version(&versions, None), None);
    assert_eq!(requested_version(&[], Some("0.19.5")), None);
}

#[test]
fn vanilla_keeps_an_optional_game_version_and_needs_no_loader() {
    assert_eq!(game_for(None, "  ", &[], None).unwrap(), None);
    assert_eq!(
        game_for(None, " 1.21.11 ", &[], None).unwrap(),
        Some(Game::new("1.21.11"))
    );
}

#[test]
fn a_loader_needs_a_game_version_and_a_published_build() {
    let versions = [offer("0.19.6", Some(false)), offer("0.19.5", Some(true))];

    let missing_game = game_for(Some(LoaderKind::Fabric), "", &versions, Some("0.19.5"));
    assert_eq!(
        missing_game.unwrap_err(),
        "Enter the Minecraft version to install Fabric for."
    );

    let missing_choice = game_for(Some(LoaderKind::Fabric), "1.21.11", &versions, None);
    assert_eq!(
        missing_choice.unwrap_err(),
        "Choose a Fabric version for Minecraft 1.21.11."
    );

    let chosen = game_for(
        Some(LoaderKind::Fabric),
        " 1.21.11 ",
        &versions,
        Some("0.19.6"),
    )
    .unwrap()
    .expect("game");
    assert_eq!(chosen.minecraft, "1.21.11");
    let loader = chosen.loader.expect("loader");
    assert_eq!(loader.kind, "fabric");
    assert_eq!(loader.version.as_deref(), Some("0.19.6"));

    let recommended = game_for(
        Some(LoaderKind::Fabric),
        "1.21.11",
        &versions,
        Some("0.19.5"),
    )
    .unwrap()
    .expect("game");
    assert!(recommended.loader.expect("loader").version.is_none());
}

#[test]
fn a_kind_with_no_builds_reads_as_that_not_as_a_generic_failure() {
    let error = LoaderError::NoVersions {
        loader: LoaderKind::Forge,
        game_version: "1.5.2".into(),
    };

    assert_eq!(
        LoadState::from_error(&error),
        LoadState::Empty("Forge has no builds for Minecraft 1.5.2.".into())
    );
}

#[test]
fn an_unreachable_provider_reads_as_a_failure_naming_the_loader() {
    let error = LoaderError::Unavailable {
        loader: LoaderKind::Quilt,
        source: FetchError::Fatal("offline".into()),
    };

    let LoadState::Failed(message) = LoadState::from_error(&error) else {
        panic!("a transport failure is not an empty listing");
    };
    assert!(message.starts_with("Could not load Quilt versions:"));
}

#[test]
fn the_loader_choices_are_vanilla_plus_every_kind_with_a_provider() {
    let all = choices();
    let kinds: Vec<LoaderKind> = all.iter().filter_map(|choice| choice.loader()).collect();

    assert_eq!(
        kinds,
        vec![
            LoaderKind::Fabric,
            LoaderKind::LegacyFabric,
            LoaderKind::Quilt,
            LoaderKind::Forge,
            LoaderKind::NeoForge,
        ]
    );
    assert!(!kinds.contains(&LoaderKind::OptiFine));
    assert_eq!(all[0], LoaderChoice::Vanilla);
    assert_eq!(LoaderChoice::Vanilla.loader(), None);
}
