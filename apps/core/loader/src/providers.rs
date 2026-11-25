// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use axlauncher_spi::{
    LoaderError, LoaderKind, LoaderProfile, LoaderRequest, LoaderVersion, ModLoader, Network,
};

use crate::{LoaderApi, forge::ForgeApi, neoforge::NeoForgeApi};

/// The loader kinds with a backend in this build, in picker order.
pub fn supported_kinds() -> impl Iterator<Item = LoaderKind> {
    LoaderKind::ALL
        .into_iter()
        .filter(|kind| *kind != LoaderKind::OptiFine)
}

/// One provider for both catalog lookup and installation.
pub fn provider<N: Network>(network: N, kind: LoaderKind) -> Result<impl ModLoader, LoaderError> {
    let provider = match kind {
        LoaderKind::Fabric => Provider::Metadata(LoaderApi::fabric(network)),
        LoaderKind::LegacyFabric => Provider::Metadata(LoaderApi::legacy_fabric(network)),
        LoaderKind::Quilt => Provider::Metadata(LoaderApi::quilt(network)),
        LoaderKind::Forge => Provider::Forge(ForgeApi::new(network)),
        LoaderKind::NeoForge => Provider::NeoForge(NeoForgeApi::new(network)),
        LoaderKind::OptiFine => return Err(LoaderError::Unsupported { loader: kind }),
    };
    Ok(provider)
}

enum Provider<N: Network> {
    Metadata(LoaderApi<N>),
    Forge(ForgeApi<N>),
    NeoForge(NeoForgeApi<N>),
}

impl<N: Network> ModLoader for Provider<N> {
    fn kind(&self) -> LoaderKind {
        match self {
            Self::Metadata(api) => api.kind(),
            Self::Forge(api) => api.kind(),
            Self::NeoForge(api) => api.kind(),
        }
    }

    async fn versions(&self, game_version: &str) -> Result<Vec<LoaderVersion>, LoaderError> {
        match self {
            Self::Metadata(api) => api.versions(game_version).await,
            Self::Forge(api) => api.versions(game_version).await,
            Self::NeoForge(api) => api.versions(game_version).await,
        }
    }

    async fn profile(&self, request: LoaderRequest) -> Result<LoaderProfile, LoaderError> {
        match self {
            Self::Metadata(api) => api.profile(request).await,
            Self::Forge(api) => api.profile(request).await,
            Self::NeoForge(api) => api.profile(request).await,
        }
    }
}
