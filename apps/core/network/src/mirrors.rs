// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axlauncher_spi::{DownloadMirror, FetchKind, ServiceHost};
use tokio::sync::Semaphore;
use url::Url;

const BMCLAPI_CONNECTIONS: usize = 16;
const MCIM_CONNECTIONS: usize = 4;
const ALIYUN_CONNECTIONS: usize = 16;
const CONNECT_COOLDOWN: Duration = Duration::from_secs(30);

static BMCLAPI_BUFFERED: Semaphore = Semaphore::const_new(BMCLAPI_CONNECTIONS);
static MCIM_BUFFERED: Semaphore = Semaphore::const_new(MCIM_CONNECTIONS);
static ALIYUN_BUFFERED: Semaphore = Semaphore::const_new(ALIYUN_CONNECTIONS);

/// The user's order for public sources. MCIM always follows its upstream.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MirrorPreference {
    #[default]
    Auto,
    OfficialOnly,
    MirrorPreferred,
}

/// Endpoints supplied by the application for the supported public mirrors.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MirrorHosts {
    pub bmclapi: Option<ServiceHost>,
    pub mcim: Option<ServiceHost>,
    pub aliyun: Option<ServiceHost>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mirror {
    Bmclapi,
    Mcim,
    Aliyun,
}

impl Mirror {
    fn host(self, hosts: &MirrorHosts) -> Option<&str> {
        match self {
            Self::Bmclapi => &hosts.bmclapi,
            Self::Mcim => &hosts.mcim,
            Self::Aliyun => &hosts.aliyun,
        }
        .as_ref()
        .map(ServiceHost::as_str)
    }

    fn for_host(host: &str, hosts: &MirrorHosts) -> Option<Self> {
        [Self::Bmclapi, Self::Mcim, Self::Aliyun]
            .into_iter()
            .find(|mirror| mirror.host(hosts) == Some(host))
    }
}

/// Source preferences and configured endpoints for downloads.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MirrorSettings {
    pub metadata: MirrorPreference,
    pub files: MirrorPreference,
    pub maven_central: MirrorPreference,
    pub hosts: MirrorHosts,
}

#[derive(Debug, Default)]
pub(super) struct Mirrors {
    settings: MirrorSettings,
    unavailable: Mutex<HashMap<String, Instant>>,
}

impl Mirrors {
    pub(super) fn new(settings: MirrorSettings) -> Self {
        Self {
            settings,
            unavailable: Mutex::default(),
        }
    }

    pub(super) fn alternate(&self, original: &str, kind: FetchKind) -> Option<DownloadMirror> {
        let original = Url::parse(original).ok()?;
        let upstream = original.host_str()?;
        let (mirror, path) = mirror_path(&original, upstream)?;
        let host = mirror.host(&self.settings.hosts)?;
        let preference = match (mirror, kind) {
            (Mirror::Aliyun, _) => self.settings.maven_central,
            (_, FetchKind::Metadata | FetchKind::UnverifiedMetadata) => self.settings.metadata,
            (_, FetchKind::File | FetchKind::UnverifiedFile) => self.settings.files,
        };
        if preference == MirrorPreference::OfficialOnly
            || (preference == MirrorPreference::Auto
                && matches!(
                    kind,
                    FetchKind::UnverifiedMetadata | FetchKind::UnverifiedFile
                ))
        {
            return None;
        }
        let preferred = mirror != Mirror::Mcim && self.prefer_mirror(host, upstream, preference);
        let mut url = Url::parse(&format!("https://{host}{path}")).ok()?;
        url.set_query(original.query());
        Some(DownloadMirror {
            url: url.into(),
            preferred,
        })
    }

    fn prefer_mirror(&self, mirror: &str, upstream: &str, preference: MirrorPreference) -> bool {
        let now = Instant::now();
        let mut unavailable = self.unavailable.lock().expect("mirror availability lock");
        if unavailable.is_empty() {
            return preference == MirrorPreference::MirrorPreferred;
        }
        unavailable.retain(|_, until| *until > now);
        !unavailable.contains_key(mirror)
            && (preference == MirrorPreference::MirrorPreferred
                || unavailable.contains_key(upstream))
    }

    pub(super) fn connect_failed(&self, url: &str) {
        let Ok(url) = Url::parse(url) else { return };
        let Some(host) = url.host_str() else { return };
        let mirror = Mirror::for_host(host, &self.settings.hosts)
            .or_else(|| mirror_path(&url, host).map(|(mirror, _)| mirror));
        if let Some(mirror) = mirror
            && mirror != Mirror::Mcim
            && mirror.host(&self.settings.hosts).is_some()
        {
            self.unavailable
                .lock()
                .expect("mirror availability lock")
                .insert(host.to_owned(), Instant::now() + CONNECT_COOLDOWN);
        }
    }

    pub(super) fn download_connection_limit(&self, host: &str) -> NonZeroUsize {
        match Mirror::for_host(host, &self.settings.hosts) {
            Some(Mirror::Bmclapi) => NonZeroUsize::new(BMCLAPI_CONNECTIONS).unwrap(),
            Some(Mirror::Mcim) => NonZeroUsize::new(MCIM_CONNECTIONS).unwrap(),
            Some(Mirror::Aliyun) => NonZeroUsize::new(ALIYUN_CONNECTIONS).unwrap(),
            None => NonZeroUsize::MAX,
        }
    }

    pub(super) fn buffered_quota(&self, host: &str) -> Option<&'static Semaphore> {
        match Mirror::for_host(host, &self.settings.hosts) {
            Some(Mirror::Bmclapi) => Some(&BMCLAPI_BUFFERED),
            Some(Mirror::Mcim) => Some(&MCIM_BUFFERED),
            Some(Mirror::Aliyun) => Some(&ALIYUN_BUFFERED),
            None => None,
        }
    }
}

fn mirror_path(url: &Url, host: &str) -> Option<(Mirror, String)> {
    if !matches!(url.scheme(), "http" | "https")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    let path = url.path();
    if url.query().is_none()
        && let Some(path) = bmclapi_path(host, path)
    {
        return Some((Mirror::Bmclapi, path));
    }
    if url.scheme() != "https" {
        return None;
    }
    if matches!(host, "repo.maven.apache.org" | "repo1.maven.org") && url.query().is_none() {
        let artifact = path
            .strip_prefix("/maven2/")
            .filter(|path| !path.is_empty())?;
        return Some((Mirror::Aliyun, format!("/repository/public/{artifact}")));
    }
    let path = match host {
        "api.modrinth.com" if path.starts_with("/v2/") => format!("/modrinth{path}"),
        "api.curseforge.com" if path.starts_with("/v1/") => format!("/curseforge{path}"),
        "cdn.modrinth.com" if path.starts_with("/data/") && url.query().is_none() => path.into(),
        "edge.forgecdn.net" if path.starts_with("/files/") && url.query().is_none() => path.into(),
        _ => return None,
    };
    Some((Mirror::Mcim, path))
}

fn bmclapi_path(host: &str, path: &str) -> Option<String> {
    let path = match host {
        "launchermeta.mojang.com"
        | "piston-meta.mojang.com"
        | "launcher.mojang.com"
        | "piston-data.mojang.com"
            if mojang_path(path) =>
        {
            path.into()
        }
        "resources.download.minecraft.net" => format!("/assets{path}"),
        "libraries.minecraft.net" | "maven.minecraftforge.net" | "maven.fabricmc.net" => {
            format!("/maven{path}")
        }
        "files.minecraftforge.net" if path.starts_with("/maven/") => path.into(),
        "files.minecraftforge.net" if path == "/net/minecraftforge/forge/promotions_slim.json" => {
            format!("/maven{path}")
        }
        "maven.neoforged.net" => {
            let releases = path.strip_prefix("/releases/net/neoforged/")?;
            format!("/maven/net/neoforged/{releases}")
        }
        "meta.fabricmc.net" if path.starts_with("/v2/versions/") => format!("/fabric-meta{path}"),
        "authlib-injector.yushi.moe" if path.starts_with("/artifact/") => {
            format!("/mirrors/authlib-injector{path}")
        }
        _ => return None,
    };
    Some(path)
}

fn mojang_path(path: &str) -> bool {
    matches!(
        path,
        "/mc/game/version_manifest.json" | "/mc/game/version_manifest_v2.json"
    ) || [
        "/v1/packages/",
        "/v1/objects/",
        "/v1/products/java-runtime/",
    ]
    .iter()
    .any(|prefix| path.starts_with(prefix))
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/network/src/mirrors.rs"]
mod tests;
