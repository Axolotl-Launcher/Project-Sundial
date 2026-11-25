// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::sync::OnceLock;

use axlauncher_core::network::MirrorHosts;
use axlauncher_core::spi::ServiceHost;

pub(crate) struct SourceHosts {
    pub(crate) mirrors: MirrorHosts,
    pub(crate) fallen_proxy: Option<ServiceHost>,
}

pub(crate) fn source_hosts() -> &'static SourceHosts {
    static HOSTS: OnceLock<SourceHosts> = OnceLock::new();
    HOSTS.get_or_init(|| SourceHosts {
        mirrors: MirrorHosts {
            bmclapi: host_from_env("AXLAUNCHER_BMCLAPI_HOST"),
            mcim: host_from_env("AXLAUNCHER_MCIM_HOST"),
            aliyun: host_from_env("AXLAUNCHER_ALIYUN_MAVEN_HOST"),
        },
        fallen_proxy: host_from_env("AXLAUNCHER_FALLEN_PROXY_DOMAIN"),
    })
}

fn host_from_env(name: &str) -> Option<ServiceHost> {
    ServiceHost::parse(&std::env::var(name).ok()?)
}
