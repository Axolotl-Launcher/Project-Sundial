// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use axlauncher_core::network::MirrorHosts;
use axlauncher_core::spi::ServiceHost;

pub(crate) fn mirror_hosts() -> MirrorHosts {
    MirrorHosts {
        bmclapi: Some(service_host("bmclapi2.bangbang93.com")),
        mcim: Some(service_host("mod.mcimirror.top")),
        aliyun: Some(service_host("maven.aliyun.com")),
    }
}

pub(crate) fn fallen_proxy_host() -> ServiceHost {
    service_host("msp.fallenbreath.me")
}

fn service_host(host: &str) -> ServiceHost {
    ServiceHost::parse(host).expect("a built-in service hostname")
}
