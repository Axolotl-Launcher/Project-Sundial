// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use axlauncher_core::network::MirrorPreference;
use axlauncher_core::state::load_settings;

use super::*;
use crate::installations::InstallOptions;
use crate::panel::download_debug::test_tempfile;

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
    let options = InstallOptions::from_settings(Some(&saved));
    assert_eq!(options.mirrors.metadata, MirrorPreference::MirrorPreferred);
    assert_eq!(options.mirrors.files, MirrorPreference::OfficialOnly);
    assert_eq!(
        options.mirrors.maven_central,
        MirrorPreference::MirrorPreferred
    );
    assert_eq!(options.concurrency, 24);
}
