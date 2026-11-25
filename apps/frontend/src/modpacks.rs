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
    policy: PlanOptions,
}

enum InstallFailure {
    Failed(String),
    Manual(Box<PendingInstall>),
}

struct Installed {
    directory: PathBuf,
    root_data_packs: bool,
}

struct PendingInstall {
    plan: InstallPlan,
    staging: StagedProfile,
    files: Vec<ManualFile>,
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
) -> Result<Installed, InstallFailure> {
    let runtime = install_runtime()?;
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
    let files = runtime.block_on(install_files(
        &network,
        &staging,
        &plan,
        options.concurrency,
        &progress,
    ))?;
    if !files.is_empty() {
        return Err(InstallFailure::Manual(Box::new(PendingInstall {
            plan,
            staging,
            files,
        })));
    }
    publish(&runtime, data, &plan, staging, options, &progress)
}

fn install_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())
}

impl PendingInstall {
    fn files(&self) -> &[ManualFile] {
        &self.files
    }

    fn resume(
        mut self,
        data: &DataDir,
        local: &BTreeMap<String, PathBuf>,
        options: InstallOptions,
        progress: watch::Sender<Progress>,
    ) -> Result<Installed, InstallFailure> {
        self.files = acquisition::supply_manual_files(
            self.files,
            &self.plan,
            self.staging.directory(),
            local,
            &progress,
        );
        if !self.files.is_empty() {
            return Err(InstallFailure::Manual(Box::new(self)));
        }
        publish(
            &install_runtime()?,
            data,
            &self.plan,
            self.staging,
            options,
            &progress,
        )
    }
}

fn publish(
    runtime: &tokio::runtime::Runtime,
    data: &DataDir,
    plan: &InstallPlan,
    staging: StagedProfile,
    options: InstallOptions,
    progress: &watch::Sender<Progress>,
) -> Result<Installed, InstallFailure> {
    let mut game = Game::new(plan.game().minecraft());
    if let Some((kind, version)) = plan.game().loader() {
        game = game.with_loader(kind.as_str(), Some(version.to_owned()));
    }
    plan.write_bundled_files(|path, input| {
        let destination = staging.directory().join(path);
        let mut file = AtomicFile::new(&destination)?;
        io::copy(input, file.file())?;
        file.commit(&destination)
    })
    .map_err(|error| error.to_string())?;
    progress.send_replace(Progress::Game);
    let version = prepare_game(runtime, data, &game, options)?;
    let mut profile = Profile::new(plan.name(), Some(game));
    profile.set_installed_version(version);
    profile.set_install_status(InstallStatus::Ready);
    let directory = staging
        .publish(&profile)
        .map_err(|error| error.to_string())?;
    Ok(Installed {
        directory,
        root_data_packs: plan.has_root_data_packs(),
    })
}

async fn install_files(
    network: &impl Network,
    staging: &StagedProfile,
    plan: &InstallPlan,
    concurrency: usize,
    progress: &watch::Sender<Progress>,
) -> Result<Vec<ManualFile>, String> {
    let total = plan.downloads().len();
    progress.send_replace(Progress::Files { done: 0, total });
    let mut done = 0;
    stream::iter(
        plan.downloads()
            .iter()
            .map(|file| acquire(network, staging.directory(), file)),
    )
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
    .await
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

fn run_install(
    data: DataDir,
    pack: &Pack,
    selection: Selection<'_>,
    pending: Option<Box<PendingInstall>>,
    local: &BTreeMap<String, PathBuf>,
    options: InstallOptions,
    progress: watch::Sender<Progress>,
) -> Result<Installed, InstallFailure> {
    match pending {
        Some(pending) => pending.resume(&data, local, options, progress),
        None => install(&data, pack, selection, options, progress),
    }
}
