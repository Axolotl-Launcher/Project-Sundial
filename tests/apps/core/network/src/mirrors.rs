// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

const BMCLAPI: &str = "bmclapi.example.invalid";
const MCIM: &str = "mcim.example.invalid";
const ALIYUN: &str = "aliyun.example.invalid";

fn configured() -> MirrorSettings {
    MirrorSettings {
        hosts: MirrorHosts {
            bmclapi: ServiceHost::parse(BMCLAPI),
            mcim: ServiceHost::parse(MCIM),
            aliyun: ServiceHost::parse(ALIYUN),
        },
        ..MirrorSettings::default()
    }
}

#[test]
fn aliyun_maps_both_maven_central_origins_and_uses_its_own_preference() {
    let path = "org/example/library/1.0/name%20with%20spaces.jar";
    for host in ["repo.maven.apache.org", "repo1.maven.org"] {
        let original = format!("https://{host}/maven2/{path}");
        let expected = format!("https://{ALIYUN}/repository/public/{path}");
        assert_eq!(
            Mirrors::new(configured()).alternate(&original, FetchKind::File),
            Some(DownloadMirror {
                url: expected.clone(),
                preferred: false
            }),
        );
        let mirrors = Mirrors::new(MirrorSettings {
            metadata: MirrorPreference::OfficialOnly,
            files: MirrorPreference::OfficialOnly,
            maven_central: MirrorPreference::MirrorPreferred,
            ..configured()
        });
        let preferred = Some(DownloadMirror {
            url: expected,
            preferred: true,
        });
        assert_eq!(mirrors.alternate(&original, FetchKind::Metadata), preferred);
        assert_eq!(mirrors.alternate(&original, FetchKind::File), preferred);
    }
}

#[test]
fn aliyun_can_be_disabled_while_bmclapi_is_preferred() {
    let mirrors = Mirrors::new(MirrorSettings {
        metadata: MirrorPreference::MirrorPreferred,
        files: MirrorPreference::MirrorPreferred,
        maven_central: MirrorPreference::OfficialOnly,
        ..configured()
    });
    let central = "https://repo.maven.apache.org/maven2/org/example/library.jar";
    let minecraft = "https://libraries.minecraft.net/org/example/library.jar";
    assert_eq!(mirrors.alternate(central, FetchKind::File), None);
    assert_eq!(mirrors.alternate(central, FetchKind::Metadata), None);
    assert!(
        mirrors
            .alternate(minecraft, FetchKind::File)
            .unwrap()
            .preferred
    );
}

#[test]
fn aliyun_only_rewrites_public_maven_central_artifact_paths() {
    let mirrors = Mirrors::new(configured());
    for url in [
        "https://repo.maven.apache.org/maven2/",
        "https://repo.maven.apache.org/maven20/org/example.jar",
        "https://repo1.maven.org/maven2/org/example.jar?token=private",
        "https://repo1.maven.org.evil.example/maven2/org/example.jar",
        "http://repo1.maven.org/maven2/org/example.jar",
    ] {
        assert_eq!(mirrors.alternate(url, FetchKind::File), None, "{url}");
    }
}

#[test]
fn bmclapi_maps_game_assets_and_java_runtime() {
    assert_bmclapi_routes(&[
        (
            "piston-meta.mojang.com/mc/game/version_manifest_v2.json",
            "/mc/game/version_manifest_v2.json",
        ),
        (
            "piston-meta.mojang.com/v1/packages/hash/version.json",
            "/v1/packages/hash/version.json",
        ),
        (
            "piston-data.mojang.com/v1/objects/hash/client.jar",
            "/v1/objects/hash/client.jar",
        ),
        (
            "resources.download.minecraft.net/ab/abcdef",
            "/assets/ab/abcdef",
        ),
        (
            "libraries.minecraft.net/org/example/one.jar",
            "/maven/org/example/one.jar",
        ),
        (
            "launchermeta.mojang.com/v1/products/java-runtime/hash/all.json",
            "/v1/products/java-runtime/hash/all.json",
        ),
        (
            "authlib-injector.yushi.moe/artifact/56/agent.jar",
            "/mirrors/authlib-injector/artifact/56/agent.jar",
        ),
    ]);
}

#[test]
fn bmclapi_maps_supported_loader_paths() {
    assert_bmclapi_routes(&[
        (
            "maven.minecraftforge.net/net/minecraftforge/forge/installer.jar",
            "/maven/net/minecraftforge/forge/installer.jar",
        ),
        (
            "files.minecraftforge.net/maven/net/minecraftforge/forge/installer.jar",
            "/maven/net/minecraftforge/forge/installer.jar",
        ),
        (
            "files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json",
            "/maven/net/minecraftforge/forge/promotions_slim.json",
        ),
        (
            "maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml",
            "/maven/net/neoforged/neoforge/maven-metadata.xml",
        ),
        (
            "meta.fabricmc.net/v2/versions/loader/1.21",
            "/fabric-meta/v2/versions/loader/1.21",
        ),
        (
            "maven.fabricmc.net/net/fabricmc/fabric-loader/loader.jar",
            "/maven/net/fabricmc/fabric-loader/loader.jar",
        ),
    ]);
}

fn assert_bmclapi_routes(routes: &[(&str, &str)]) {
    let mirrors = Mirrors::new(configured());
    for (original, expected) in routes {
        let route = mirrors
            .alternate(&format!("https://{original}"), FetchKind::File)
            .unwrap();
        assert_eq!(route.url, format!("https://{BMCLAPI}{expected}"));
        assert!(!route.preferred);
    }
}

#[test]
fn mcim_preserves_api_queries_and_escaped_file_paths() {
    let routes = [
        (
            "api.modrinth.com/v2/project/example/version?loaders=%5B%22fabric%22%5D",
            "/modrinth/v2/project/example/version?loaders=%5B%22fabric%22%5D",
        ),
        (
            "api.curseforge.com/v1/mods/238222/files?pageSize=20",
            "/curseforge/v1/mods/238222/files?pageSize=20",
        ),
        (
            "cdn.modrinth.com/data/project/versions/id/name%20with%20spaces.jar",
            "/data/project/versions/id/name%20with%20spaces.jar",
        ),
        ("edge.forgecdn.net/files/1/2/mod.jar", "/files/1/2/mod.jar"),
    ];
    let mirrors = Mirrors::new(MirrorSettings {
        metadata: MirrorPreference::MirrorPreferred,
        files: MirrorPreference::MirrorPreferred,
        ..configured()
    });
    for (original, expected) in routes {
        let route = mirrors
            .alternate(&format!("https://{original}"), FetchKind::File)
            .unwrap();
        assert_eq!(route.url, format!("https://{MCIM}{expected}"));
        assert!(!route.preferred);
    }
}

#[test]
fn unmatched_or_credential_bearing_file_urls_remain_direct() {
    let mirrors = Mirrors::new(configured());
    for url in [
        "https://mediafilez.forgecdn.net/files/1/2/mod.jar",
        "https://api.modrinth.com/v3/project/a",
        "https://api.curseforge.com/v10/mods/1",
        "https://cdn.modrinth.com/data/a/b.jar?signature=secret",
        "https://name:password@cdn.modrinth.com/data/file.jar",
        "https://cdn.modrinth.com:8080/data/file.jar",
        "https://meta.legacyfabric.net/v2/versions/loader/1.8.9",
        "https://meta.quiltmc.org/v3/versions/loader/1.21",
        "https://launchermeta.mojang.com/unrelated",
        "https://mcim.example.invalid/data/mod.jar",
        "https://cdn.modrinth.com.example.invalid/data/mod.jar",
    ] {
        assert_eq!(mirrors.alternate(url, FetchKind::File), None, "{url}");
    }
}

#[test]
fn metadata_and_files_use_independent_preferences() {
    let mirrors = Mirrors::new(MirrorSettings {
        metadata: MirrorPreference::MirrorPreferred,
        files: MirrorPreference::OfficialOnly,
        ..configured()
    });
    let url = "https://maven.minecraftforge.net/net/minecraftforge/forge/example.jar";
    assert!(
        mirrors
            .alternate(url, FetchKind::Metadata)
            .unwrap()
            .preferred
    );
    assert_eq!(mirrors.alternate(url, FetchKind::File), None);
    let mcim = "https://cdn.modrinth.com/data/example.jar";
    assert_eq!(mirrors.alternate(mcim, FetchKind::File), None);
}

#[test]
fn connect_failure_changes_only_matching_bmclapi_routes_until_the_cooldown_expires() {
    let mirrors = Mirrors::new(configured());
    let asset = "https://resources.download.minecraft.net/ab/abcdef";
    let jar = "https://libraries.minecraft.net/org/example/one.jar";
    mirrors.connect_failed(asset);
    let route = mirrors.alternate(asset, FetchKind::File).unwrap();
    assert!(route.preferred);
    assert!(!mirrors.alternate(jar, FetchKind::File).unwrap().preferred);
    mirrors.connect_failed(&route.url);
    assert!(!mirrors.alternate(asset, FetchKind::File).unwrap().preferred);

    let mut unavailable = mirrors.unavailable.lock().unwrap();
    let elapsed = Instant::now() - CONNECT_COOLDOWN;
    unavailable.values_mut().for_each(|until| *until = elapsed);
    drop(unavailable);
    assert!(!mirrors.alternate(asset, FetchKind::File).unwrap().preferred);
}

#[tokio::test]
async fn automatic_routing_keeps_unsigned_installation_sources_official_on_failure() {
    for (url, kind) in [
        (
            "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
            FetchKind::UnverifiedMetadata,
        ),
        (
            "https://launchermeta.mojang.com/v1/products/java-runtime/hash/all.json",
            FetchKind::UnverifiedMetadata,
        ),
        (
            "https://maven.minecraftforge.net/net/minecraftforge/forge/installer.jar",
            FetchKind::UnverifiedFile,
        ),
        (
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/installer.jar",
            FetchKind::UnverifiedFile,
        ),
        (
            "https://repo.maven.apache.org/maven2/org/example/library.jar",
            FetchKind::UnverifiedFile,
        ),
    ] {
        assert_no_automatic_fallback(url, kind).await;

        let preferred = Mirrors::new(MirrorSettings {
            metadata: MirrorPreference::MirrorPreferred,
            files: MirrorPreference::MirrorPreferred,
            maven_central: MirrorPreference::MirrorPreferred,
            ..configured()
        });
        assert!(preferred.alternate(url, kind).unwrap().preferred);
    }
}

async fn assert_no_automatic_fallback(url: &str, kind: FetchKind) {
    let auto = Mirrors::new(configured());
    auto.connect_failed(url);
    let alternate = auto.alternate(url, kind);
    let result = super::super::fetch_alternate(url, alternate, async |source| {
        assert_eq!(source, url, "unexpected fallback for {url}");
        Err::<(), _>("official source unavailable")
    })
    .await;
    assert_eq!(result, Err("official source unavailable"));
}

#[test]
fn unset_mirrors_never_add_a_source_and_only_configured_hosts_have_limits() {
    let upstream = "https://resources.download.minecraft.net/ab/abcdef";
    let mirrors = Mirrors::new(MirrorSettings {
        files: MirrorPreference::MirrorPreferred,
        ..MirrorSettings::default()
    });
    assert_eq!(mirrors.alternate(upstream, FetchKind::File), None);
    assert_eq!(mirrors.download_connection_limit(MCIM), NonZeroUsize::MAX);
    assert!(mirrors.buffered_quota(MCIM).is_none());

    let mirrors = Mirrors::new(configured());
    assert_eq!(
        mirrors.download_connection_limit(MCIM).get(),
        MCIM_CONNECTIONS
    );
    assert_eq!(
        mirrors.buffered_quota(MCIM).unwrap().available_permits(),
        MCIM_CONNECTIONS
    );
}

#[test]
fn supplied_hosts_cannot_include_credentials_or_other_url_components() {
    let host = ServiceHost::parse("Mirror.Example.Invalid").unwrap();
    assert_eq!(host.as_str(), "mirror.example.invalid");
    for value in [
        "mirror.example.invalid/",
        "mirror.example.invalid:443",
        "https://mirror.example.invalid",
        "user:password@mirror.example.invalid",
        "mirror.example.invalid?key=value",
        "mirror..invalid",
        " mirror.example.invalid",
    ] {
        assert!(ServiceHost::parse(value).is_none(), "{value}");
    }
}
