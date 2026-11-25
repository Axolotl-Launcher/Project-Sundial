// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

#[test]
fn desktop_downloads_can_route_to_each_built_in_mirror() {
    let network = DownloadOptions::from_settings(None).network().unwrap();
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
