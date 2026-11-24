// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use axlauncher_core::spi::LoaderKind;

use super::*;

#[test]
fn every_offered_loader_has_a_provider_and_optifine_is_not_offered() {
    assert_eq!(
        KINDS,
        [
            LoaderKind::Fabric,
            LoaderKind::Quilt,
            LoaderKind::Forge,
            LoaderKind::NeoForge,
        ]
    );
    assert!(!KINDS.contains(&LoaderKind::OptiFine));
}

#[test]
fn the_kinds_the_form_offers_are_the_kinds_an_install_can_serve() {
    for kind in KINDS {
        assert!(
            crate::installations::supported_loader(kind.as_str()),
            "{kind} is offered but not installable"
        );
    }
}
