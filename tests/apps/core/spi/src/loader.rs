// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

#[test]
fn every_kind_round_trips_through_its_spelling() {
    for kind in LoaderKind::ALL {
        assert_eq!(LoaderKind::parse(kind.as_str()), Some(kind));
    }
}

#[test]
fn a_spelling_is_read_regardless_of_case_and_space() {
    assert_eq!(LoaderKind::parse("  NeoForge "), Some(LoaderKind::NeoForge));
    assert_eq!(LoaderKind::parse("FABRIC"), Some(LoaderKind::Fabric));
}

#[test]
fn an_unknown_spelling_is_absent_rather_than_a_guess() {
    assert_eq!(LoaderKind::parse("liteloader"), None);
    assert_eq!(LoaderKind::parse(""), None);
}

#[test]
fn a_kind_reads_as_its_stored_spelling() {
    assert_eq!(LoaderKind::OptiFine.to_string(), "optifine");
}
