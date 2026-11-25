// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use axlauncher_core::network::{MirrorPreference, ReqwestNetwork};
use axlauncher_core::spi::Network;
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

#[test]
fn desktop_downloads_can_route_to_each_built_in_mirror() {
    let options = InstallOptions::from_settings(None);
    let network = ReqwestNetwork::desktop_with_mirrors(options.mirrors).unwrap();
    for (official, expected) in [
        (
            "https://libraries.minecraft.net/test/artifact.jar",
            "https://bmclapi2.bangbang93.com/maven/test/artifact.jar",
        ),
        (
            "https://cdn.modrinth.com/data/test/artifact.jar",
            "https://mod.mcimirror.top/data/test/artifact.jar",
        ),
        (
            "https://repo.maven.apache.org/maven2/test/artifact.jar",
            "https://maven.aliyun.com/repository/public/test/artifact.jar",
        ),
    ] {
        let mirror = network.download_mirror(official, true).unwrap();
        assert_eq!(mirror.url, expected);
        assert!(!mirror.preferred);
    }
}
