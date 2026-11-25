// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::path::Path;

use axlauncher_core::install::{loader, prepare::Prepared, runtime, vanilla};
use axlauncher_core::launch::{SystemJava, Target};
use axlauncher_core::loader::{ForgeApi, LoaderApi, NeoForgeApi};
use axlauncher_core::network::ReqwestNetwork;
use axlauncher_core::spi::{LoaderKind, LoaderRequest, ModLoader};
use axlauncher_core::storage::DataDir;
use axlauncher_modpack::GameRequirements;

use crate::Result;

pub(super) async fn prepare(
    network: &ReqwestNetwork,
    data: &DataDir,
    root: &Path,
    game: &GameRequirements,
    target: &Target,
) -> Result<String> {
    let minecraft = game.minecraft();
    let Some((kind, build)) = game.loader() else {
        complete(vanilla::install(network, root, minecraft, target, 0, |_| {}).await?)?;
        return Ok(minecraft.into());
    };
    let request = LoaderRequest {
        game_version: minecraft.into(),
        loader_version: Some(build.into()),
    };
    let base = LoaderInstall {
        network,
        data,
        root,
        target,
    };
    match kind {
        LoaderKind::Forge => base.run(&ForgeApi::new(network.clone()), request).await,
        LoaderKind::NeoForge => base.run(&NeoForgeApi::new(network.clone()), request).await,
        LoaderKind::Fabric => base.run(&LoaderApi::fabric(network.clone()), request).await,
        LoaderKind::Quilt => base.run(&LoaderApi::quilt(network.clone()), request).await,
        _ => Err(format!("Unsupported mod loader {kind}").into()),
    }
}

pub(super) fn complete(result: Prepared) -> Result<()> {
    if let Some(error) = result.failures.first() {
        return Err(error.clone().into());
    }
    Ok(())
}

struct LoaderInstall<'a> {
    network: &'a ReqwestNetwork,
    data: &'a DataDir,
    root: &'a Path,
    target: &'a Target,
}

impl LoaderInstall<'_> {
    async fn run(&self, provider: &impl ModLoader, request: LoaderRequest) -> Result<String> {
        let java =
            SystemJava::from_environment().with_runtime_roots(vec![self.data.java_versions()]);
        let install = loader::InstallRequest {
            network: self.network,
            loader: provider,
            java: &java,
            game_root: self.root,
            request,
            target: self.target,
            concurrency: 0,
        };
        let report = |_| {};
        let result = match install.install(report).await {
            Err(loader::LoaderInstallError::Java { required }) => {
                runtime::install(self.network, &self.data.java_versions(), required, report)
                    .await?;
                install.install(report).await?
            }
            other => other?,
        };
        complete(result.prepared)?;
        Ok(result.version_id)
    }
}
