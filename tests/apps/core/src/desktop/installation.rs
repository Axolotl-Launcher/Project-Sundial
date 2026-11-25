// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

#[test]
fn installation_keys_follow_the_profile_not_its_resolved_version() {
    let game = Game::new("1.21.11");
    assert_eq!(install_key(&game).as_deref(), Some("1.21.11"));
    assert_eq!(
        install_key(&game.clone().with_loader(" VANILLA ", None)).as_deref(),
        Some("1.21.11")
    );
    assert_eq!(
        install_key(
            &game
                .clone()
                .with_loader(" FaBrIc ", Some(" 0.18.0 ".into()))
        )
        .as_deref(),
        Some("1.21.11:fabric:0.18.0")
    );
    assert_eq!(
        install_key(&game.clone().with_loader("fabric", Some(" ".into()))).as_deref(),
        Some("1.21.11:fabric")
    );
    assert!(install_key(&game.clone().with_loader("optifine", None)).is_none());
    assert!(install_key(&game.with_loader("unknown", None)).is_none());
}

#[test]
fn a_loader_is_complete_only_after_its_version_is_recorded() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = profile::create(temporary.path(), "Fixture", Game::new("1.21.11")).unwrap();
    let version = "fabric-loader-example";
    assert_eq!(
        persist_loader_result(&directory, Ok(version.to_owned())).unwrap(),
        version
    );
    assert_eq!(
        profile::read(&directory)
            .unwrap()
            .unwrap()
            .game()
            .unwrap()
            .effective_version(),
        Some(version)
    );

    let missing = temporary.path().join("removed");
    assert!(persist_loader_result(&missing, Ok(version.to_owned())).is_err());
    assert!(!missing.exists());
}
