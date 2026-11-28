//! Download requests contain configuration and file descriptors, never file bodies.

use std::path::PathBuf;

use axlauncher_download_spi::{DownloadJob, ExtraDigest, PackOrigins, TransferStats};
use axlauncher_network::{MirrorSettings, ReqwestNetwork};
use axlauncher_network_spi::auth::ApiKey;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

mod client;
pub use client::{Downloads, stats};
pub(crate) use client::{close, configure, wait_closed};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Route {
    Mirrors(MirrorSettings),
    Pack(PackOrigins),
}

impl Default for Route {
    fn default() -> Self {
        Self::Mirrors(MirrorSettings::default())
    }
}

/// The private worker request carries credentials separately from the host route.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferRoute {
    route: Route,
    key: Option<WireKey>,
}

impl TransferRoute {
    fn new(route: Route, key: Option<&ApiKey>) -> Self {
        let key = if matches!(route, Route::Pack(PackOrigins::CurseForge)) {
            key.cloned().map(WireKey)
        } else {
            None
        };
        Self { route, key }
    }

    pub fn network(&self) -> Result<ReqwestNetwork, String> {
        let result = match &self.route {
            Route::Mirrors(settings) => ReqwestNetwork::desktop_with_mirrors(settings.clone()),
            Route::Pack(origins) => {
                let allows: fn(&url::Url) -> bool = match origins {
                    PackOrigins::Modrinth => |url| PackOrigins::Modrinth.allows(url, true),
                    PackOrigins::CurseForge => |url| PackOrigins::CurseForge.allows(url, true),
                    PackOrigins::None => |_| false,
                };
                let key = match origins {
                    PackOrigins::CurseForge => self
                        .key
                        .as_ref()
                        .map(|key| (PackOrigins::AUTHENTICATED_CDN, &key.0)),
                    _ => None,
                };
                ReqwestNetwork::desktop_with_download_policy(allows, key)
            }
        };
        result.map_err(|error| error.to_string())
    }
}

#[derive(Clone, PartialEq, Eq)]
struct WireKey(ApiKey);

impl Serialize for WireKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0.expose_secret())
    }
}

impl<'de> Deserialize<'de> for WireKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        ApiKey::new(&value)
            .map(Self)
            .ok_or_else(|| serde::de::Error::custom("invalid download credential"))
    }
}

#[derive(Serialize, Deserialize)]
pub struct Transfer {
    route: TransferRoute,
    file: File,
}

/// A download job descriptor without transport configuration or credentials.
#[derive(Serialize, Deserialize)]
pub struct File {
    sources: Vec<String>,
    #[serde(with = "crate::path")]
    destination: PathBuf,
    sha1: Option<String>,
    sha1_alternatives: Vec<String>,
    size: Option<u64>,
    batch: bool,
    digest: Option<ExtraDigest>,
}

impl Transfer {
    pub fn new(
        route: Route,
        job: DownloadJob<'_>,
        digest: Option<ExtraDigest>,
    ) -> std::io::Result<Self> {
        Ok(Self::with_file(route, File::new(job, digest)?))
    }

    fn new_with_key(
        route: Route,
        job: DownloadJob<'_>,
        digest: Option<ExtraDigest>,
        key: Option<&ApiKey>,
    ) -> std::io::Result<Self> {
        Ok(Self {
            route: TransferRoute::new(route, key),
            file: File::new(job, digest)?,
        })
    }

    pub(super) fn with_file(route: Route, file: File) -> Self {
        Self {
            route: TransferRoute::new(route, None),
            file,
        }
    }

    pub fn route(&self) -> &TransferRoute {
        &self.route
    }

    pub fn file(&self) -> &File {
        &self.file
    }
}

impl File {
    pub fn new(job: DownloadJob<'_>, digest: Option<ExtraDigest>) -> std::io::Result<Self> {
        Ok(Self {
            sources: job
                .sources
                .iter()
                .map(|source| (*source).to_owned())
                .collect(),
            destination: std::path::absolute(job.destination)?,
            sha1: job.sha1.map(str::to_owned),
            sha1_alternatives: job.sha1_alternatives.to_vec(),
            size: job.size,
            batch: job.batch,
            digest,
        })
    }

    pub fn sources(&self) -> Vec<&str> {
        self.sources.iter().map(String::as_str).collect()
    }

    pub fn job<'a>(&'a self, sources: &'a [&'a str]) -> DownloadJob<'a> {
        DownloadJob {
            sources,
            destination: &self.destination,
            sha1: self.sha1.as_deref(),
            sha1_alternatives: &self.sha1_alternatives,
            size: self.size,
            batch: self.batch,
        }
    }

    pub fn digest(&self) -> Option<ExtraDigest> {
        self.digest
    }
}

pub type Reply = TransferStats;

#[cfg(test)]
#[path = "../../../../tests/apps/core/lib/src/downloads.rs"]
mod tests;
