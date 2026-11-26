//! Download requests contain configuration and file descriptors, never file bodies.

use std::path::PathBuf;

use axlauncher_network::{MirrorSettings, ReqwestNetwork};
use axlauncher_spi::downloads::{DownloadJob, ExtraDigest, PackOrigins, TransferStats};
use serde::{Deserialize, Serialize};

mod client;
pub use client::{Downloads, stats};
pub(crate) use client::{close, configure, wait_closed};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Route {
    Mirrors(MirrorSettings),
    Pack {
        origins: PackOrigins,
        key: Option<String>,
    },
}

impl Default for Route {
    fn default() -> Self {
        Self::Mirrors(MirrorSettings::default())
    }
}

impl Route {
    pub fn network(&self) -> Result<ReqwestNetwork, String> {
        let result = match self {
            Self::Mirrors(settings) => ReqwestNetwork::desktop_with_mirrors(settings.clone()),
            Self::Pack { origins, key } => {
                let allows: fn(&url::Url) -> bool = match origins {
                    PackOrigins::Modrinth => |url| PackOrigins::Modrinth.allows(url, true),
                    PackOrigins::CurseForge => |url| PackOrigins::CurseForge.allows(url, true),
                    PackOrigins::None => |_| false,
                };
                let key = match origins {
                    PackOrigins::CurseForge => key
                        .as_deref()
                        .map(|key| (PackOrigins::AUTHENTICATED_CDN, key)),
                    _ => None,
                };
                ReqwestNetwork::desktop_with_download_policy(allows, key)
            }
        };
        result.map_err(|error| error.to_string())
    }
}

#[derive(Serialize, Deserialize)]
pub struct Transfer {
    route: Route,
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
        Ok(Self {
            route,
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

    pub fn route(&self) -> &Route {
        &self.route
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
