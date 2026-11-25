// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

#[test]
fn fallen_proxy_preserves_minecraft_methods_paths_and_queries() {
    for (path, body) in [
        ("/launcher/login", Some(Body::Json("{}".into()))),
        ("/entitlements/license?requestId=test", None),
        ("/minecraft/profile", None),
    ] {
        let original = format!("https://api.minecraftservices.com{path}");
        let mut request = Request::new(&original, "test").with_minecraft_service();
        request.body = body;
        assert_eq!(
            fallen_service_url(&request).unwrap().as_str(),
            format!("https://services.msp.fallenbreath.me{path}")
        );
    }
}

#[test]
fn fallen_proxy_only_accepts_marked_official_minecraft_service_requests() {
    let unmarked = Request::new(
        "https://api.minecraftservices.com/minecraft/profile",
        "test",
    );
    assert!(fallen_service_url(&unmarked).is_none());
    for url in [
        "https://login.microsoftonline.com/consumers/oauth2/v2.0/token",
        "https://xsts.auth.xboxlive.com/xsts/authorize",
        "https://third-party.example/minecraft/profile",
        "https://api.minecraftservices.com:8443/minecraft/profile",
        "https://user:password@api.minecraftservices.com/minecraft/profile",
        "http://api.minecraftservices.com/minecraft/profile",
        "https://api.minecraftservices.com/minecraft/profile/skins",
        "https://api.minecraftservices.com/launcher/login",
    ] {
        let request = Request::new(url, "test").with_minecraft_service();
        assert!(fallen_service_url(&request).is_none(), "{url}");
    }
}
