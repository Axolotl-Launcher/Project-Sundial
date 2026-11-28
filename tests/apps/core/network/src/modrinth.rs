// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::is_project_image_url;

#[test]
fn only_accepts_project_data_from_the_https_cdn() {
    assert!(is_project_image_url(
        "https://cdn.modrinth.com/data/project/images/comparison.png"
    ));
    assert!(is_project_image_url(
        "https://cdn.modrinth.com/data/cached_images/badge.webp"
    ));
    for url in [
        "http://cdn.modrinth.com/data/image.png",
        "https://cdn.modrinth.com:8443/data/image.png",
        "https://user@cdn.modrinth.com/data/image.png",
        "https://cdn.modrinth.com.evil.example/data/image.png",
        "https://cdn.modrinth.com/other/image.png",
        "file:///data/image.png",
    ] {
        assert!(!is_project_image_url(url), "{url}");
    }
}
