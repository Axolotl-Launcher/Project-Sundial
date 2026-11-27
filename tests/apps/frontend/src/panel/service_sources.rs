// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use axlauncher_core::state::load_settings;

use super::*;
use crate::test_tempfile;

#[test]
fn preferences_are_saved_independently_and_reloaded_for_new_downloads() {
    let root = test_tempfile::tempdir().unwrap();
    update_settings(root.path(), |settings| {
        settings.max_concurrent_downloads = 24
    })
    .unwrap();
    SourceField::Metadata
        .save(root.path(), SourcePreference::MirrorPreferred)
        .unwrap();
    SourceField::Files
        .save(root.path(), SourcePreference::OfficialOnly)
        .unwrap();
    SourceField::MavenCentral
        .save(root.path(), SourcePreference::MirrorPreferred)
        .unwrap();

    let saved = load_settings(root.path()).unwrap().data;
    assert_eq!(
        saved.minecraft_metadata_source,
        SourcePreference::MirrorPreferred
    );
    assert_eq!(saved.minecraft_file_source, SourcePreference::OfficialOnly);
    assert_eq!(
        saved.maven_central_source,
        SourcePreference::MirrorPreferred
    );
    assert_eq!(saved.max_concurrent_downloads, 24);
}
