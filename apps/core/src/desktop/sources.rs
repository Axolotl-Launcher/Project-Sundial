// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Desktop provider configuration from the stored source preferences.

use axlauncher_network::{MirrorHosts, MirrorPreference, MirrorSettings, ReqwestNetwork};
use axlauncher_network_spi::{Network, ServiceHost};
use axlauncher_state::{SettingsData, SourcePreference};
use axlauncher_storage::DataDir;

use super::downloads::{DownloadRoute, Downloads};

#[derive(Clone)]
pub struct DownloadOptions {
    concurrency: usize,
    mirrors: MirrorSettings,
}

impl DownloadOptions {
    pub fn from_settings(settings: Option<&SettingsData>) -> Self {
        let mut mirrors = MirrorSettings {
            hosts: MirrorHosts {
                bmclapi: Some(service_host("bmclapi2.bangbang93.com")),
                mcim: Some(service_host("mod.mcimirror.top")),
                aliyun: Some(service_host("maven.aliyun.com")),
            },
            ..MirrorSettings::default()
        };
        let concurrency = settings.map_or(0, |settings| {
            mirrors.metadata = mirror_preference(settings.minecraft_metadata_source);
            mirrors.files = mirror_preference(settings.minecraft_file_source);
            mirrors.maven_central = mirror_preference(settings.maven_central_source);
            settings.max_concurrent_downloads as usize
        });
        Self {
            concurrency,
            mirrors,
        }
    }

    pub fn concurrency(&self) -> usize {
        self.concurrency
    }

    pub(super) fn mirrors(&self) -> MirrorSettings {
        self.mirrors.clone()
    }

    pub(super) fn network(&self) -> Result<impl Network + Clone + use<>, String> {
        ReqwestNetwork::desktop_with_mirrors(self.mirrors.clone())
            .map_err(|error| error.to_string())
    }

    pub fn downloader(&self, data: &DataDir) -> Downloads {
        Downloads::new(data.clone(), DownloadRoute::Mirrors(self.mirrors.clone()))
    }
}

/// The configured third-party sign-in proxy, or a direct connection.
pub fn fallen_proxy(settings: Option<&SettingsData>) -> Option<ServiceHost> {
    settings
        .filter(|settings| settings.use_fallen_proxy)
        .map(|_| service_host("msp.fallenbreath.me"))
}

fn mirror_preference(preference: SourcePreference) -> MirrorPreference {
    match preference {
        SourcePreference::Auto => MirrorPreference::Auto,
        SourcePreference::OfficialOnly => MirrorPreference::OfficialOnly,
        SourcePreference::MirrorPreferred => MirrorPreference::MirrorPreferred,
    }
}

fn service_host(host: &str) -> ServiceHost {
    ServiceHost::parse(host).expect("a built-in service hostname")
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/src/desktop/sources.rs"]
mod tests;
