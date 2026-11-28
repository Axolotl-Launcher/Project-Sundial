// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use url::Url;

use super::allows_image;

#[test]
fn accepts_web_images_without_an_origin_allowlist() {
    for url in [
        "https://cdn.modrinth.com/data/project/images/comparison.png",
        "https://cdn.modrinth.com/data/cached_images/badge.webp",
        "https://media.forgecdn.net/attachments/546/809/6.gif",
        "https://github.com/user-attachments/assets/8a2dcbad-57dd-4184-ac4b-a6df311ec3be",
        "https://github-production-user-asset-6210df.s3.amazonaws.com/123/image.png?signature=value",
        "https://images.example.org/illustration.png",
        "http://images.example.org:8080/illustration.png",
    ] {
        assert!(allows_image(&Url::parse(url).expect("valid URL")), "{url}");
    }
}

#[test]
fn rejects_non_web_urls_and_embedded_credentials() {
    for url in [
        "https://user@images.example.org/illustration.png",
        "https://user:password@images.example.org/illustration.png",
        "file:///data/image.png",
        "ftp://images.example.org/illustration.png",
    ] {
        assert!(!allows_image(&Url::parse(url).expect("valid URL")), "{url}");
    }
}
