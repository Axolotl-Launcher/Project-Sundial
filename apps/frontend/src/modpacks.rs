// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::PathBuf;

use axlauncher_core::install::prepare::Progress as GameProgress;
use axlauncher_core::network::ReqwestNetwork;
use axlauncher_core::network::auth::AuthHttp;
use axlauncher_core::spi::Network;
use axlauncher_core::state::profile::{Game, InstallStatus, Profile, StagedProfile};
use axlauncher_core::storage::{AtomicFile, DataDir};
use axlauncher_modpack::{InstallPlan, OptionId, Pack, PlanOptions};
use futures::{StreamExt, TryStreamExt, stream};
use tokio::sync::watch;

use crate::game_install::{self, InstallOptions};

mod acquisition;
mod view;
use acquisition::{ManualFile, acquire};
pub(crate) use view::{ImportEvent, ModpackImport};

#[derive(Clone, Copy)]
pub(crate) enum Progress {
    Planning,
    Files { done: usize, total: usize },
    Game,
}

struct Selection<'a> {
    options: &'a BTreeSet<OptionId>,
    local: &'a BTreeMap<String, PathBuf>,
    policy: PlanOptions,
}

enum InstallFailure {
    Failed(String),
    Manual(Vec<ManualFile>),
}

impl From<String> for InstallFailure {
    fn from(error: String) -> Self {
        Self::Failed(error)
    }
}

fn install(
    data: &DataDir,
    pack: &Pack,
    selection: Selection<'_>,
    options: InstallOptions,
    progress: watch::Sender<Progress>,
) -> Result<PathBuf, InstallFailure> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    let api = AuthHttp::desktop()
        .map_err(|error| format!("Could not start the metadata client: {error:?}"))?;
    let plan = runtime
        .block_on(pack.plan(selection.options, selection.policy, &api))
        .map_err(|error| error.to_string())?;
    let staging = StagedProfile::new(&data.instances()).map_err(|error| error.to_string())?;
    let network = ReqwestNetwork::desktop_with_download_policy(
        plan.redirect_policy(),
        plan.download_api_key(),
    )
    .map_err(|error| error.to_string())?;
    let mut game = Game::new(plan.game().minecraft());
    if let Some((kind, version)) = plan.game().loader() {
        game = game.with_loader(kind.as_str(), Some(version.to_owned()));
    }
    runtime.block_on(install_files(
        &network,
        &staging,
        &plan,
        selection.local,
        options.concurrency,
        &progress,
    ))?;
    progress.send_replace(Progress::Game);
    let version = prepare_game(&runtime, data, &game, options)?;
    let mut profile = Profile::new(plan.name(), Some(game));
    profile.set_installed_version(version);
    profile.set_install_status(InstallStatus::Ready);
    Ok(staging
        .publish(&profile)
        .map_err(|error| error.to_string())?)
}

async fn install_files(
    network: &impl Network,
    staging: &StagedProfile,
    plan: &InstallPlan<'_>,
    local: &BTreeMap<String, PathBuf>,
    concurrency: usize,
    progress: &watch::Sender<Progress>,
) -> Result<(), InstallFailure> {
    let total = plan.downloads().len();
    progress.send_replace(Progress::Files { done: 0, total });
    let mut done = 0;
    let missing: Vec<_> = stream::iter(plan.downloads().iter().map(|file| {
        acquire(
            network,
            staging.directory(),
            file,
            local.get(file.path()).map(PathBuf::as_path),
        )
    }))
    .buffer_unordered(if concurrency == 0 {
        axlauncher_core::download_engine::DEFAULT_CONCURRENCY
    } else {
        concurrency
    })
    .try_filter_map(|result| {
        done += 1;
        if done == total || done.is_multiple_of((total / 100).max(1)) {
            progress.send_replace(Progress::Files { done, total });
        }
        futures::future::ok(result)
    })
    .try_collect()
    .await?;
    if !missing.is_empty() {
        return Err(InstallFailure::Manual(missing));
    }
    plan.write_bundled_files(|path, input| {
        let destination = staging.directory().join(path);
        let mut file = AtomicFile::new(&destination)?;
        io::copy(input, file.file())?;
        file.commit(&destination)
    })
    .map_err(|error| error.to_string().into())
}

fn prepare_game(
    runtime: &tokio::runtime::Runtime,
    data: &DataDir,
    game: &Game,
    options: InstallOptions,
) -> Result<String, String> {
    let (progress, _updates) = watch::channel(GameProgress::Version);
    if game.loader.is_some() {
        return game_install::install_loader(data.root(), &data.meta(), game, options, progress);
    }
    let prepared = runtime.block_on(game_install::download_vanilla(
        &data.meta(),
        &game.minecraft,
        options,
        &progress,
    ))?;
    if prepared.is_complete() {
        return Ok(game.minecraft.clone());
    }
    Err(game_install::download_failure(&prepared))
}
