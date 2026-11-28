// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Prepares and publishes an instance from a validated modpack plan.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::thread;

use axlauncher_core::install::prepare::Progress as GameProgress;
use axlauncher_core::state::profile::Game;
use axlauncher_core::storage::DataDir;
use axlauncher_modpack::{ModpackPlugin, OptionId, Pack, PlanOptions};

use axlauncher_core::desktop::{
    curseforge::DesktopCurseForge,
    installation::{self, InstallTarget},
    sources::DownloadOptions,
};

mod acquisition;
pub(super) use acquisition::ManualFile;
use acquisition::StagedPack;

pub(super) enum Outcome {
    Installed {
        paths: DataDir,
        directory: PathBuf,
        root_data_packs: bool,
    },
    Pending {
        install: Box<PendingInstall>,
        files: Vec<ManualFile>,
        error: Option<String>,
    },
}

pub(super) struct PendingInstall {
    data: DataDir,
    pack: StagedPack,
    game: Game,
    version: Option<String>,
}

#[derive(Clone, Copy)]
pub(super) enum ProgressUpdate {
    Pack(PackState),
    Game(GameState),
}

#[derive(Clone, Copy, Default)]
pub(super) enum PackState {
    #[default]
    Planning,
    Downloading {
        processed: usize,
        total: usize,
    },
    Supplying,
    ManualFiles,
    Copying,
    Ready,
    Failed,
    Publishing,
}

#[derive(Clone, Copy, Default)]
pub(super) enum GameState {
    #[default]
    Pending,
    Preparing(GameProgress),
    Ready,
    Failed,
}

pub(super) fn inspect(path: &Path) -> Result<Pack, String> {
    ModpackPlugin::builtin()
        .inspect(path)
        .map_err(|error| error.to_string())
}

pub(super) fn install(
    data: DataDir,
    pack: &Pack,
    selected: &BTreeSet<OptionId>,
    policy: PlanOptions,
    options: DownloadOptions,
    report: impl Fn(ProgressUpdate) + Sync,
) -> Result<Outcome, String> {
    report(ProgressUpdate::Pack(PackState::Planning));
    let plan = install_runtime()?
        .block_on(pack.plan(selected, policy, &DesktopCurseForge))
        .map_err(|error| error.to_string())?;
    let mut game = Game::new(plan.game().minecraft());
    if let Some((kind, version)) = plan.game().loader() {
        game = game.with_loader(kind.as_str(), Some(version.to_owned()));
    }
    let staged = StagedPack::new(plan, &data)?;
    Box::new(PendingInstall {
        data,
        pack: staged,
        game,
        version: None,
    })
    .resume(&BTreeMap::new(), options, report)
}

impl PendingInstall {
    pub(in crate::modpacks) fn resume(
        mut self: Box<Self>,
        local: &BTreeMap<String, PathBuf>,
        options: DownloadOptions,
        report: impl Fn(ProgressUpdate) + Sync,
    ) -> Result<Outcome, String> {
        let (pack_result, game_error) = self.prepare(local, options, &report);
        if self.pack.is_ready()
            && let Some(version) = self.version.take()
        {
            report(ProgressUpdate::Pack(PackState::Publishing));
            let root_data_packs = self.pack.has_root_data_packs();
            let directory = self.pack.publish(self.game, version)?;
            return Ok(Outcome::Installed {
                paths: self.data,
                directory,
                root_data_packs,
            });
        }
        let error = match (pack_result.err(), game_error) {
            (Some(files), Some(game)) => Some(format!("{files}\n{game}")),
            (files, game) => files.or(game),
        };
        Ok(Outcome::Pending {
            files: self.pack.manual_files(),
            install: self,
            error,
        })
    }

    fn prepare(
        &mut self,
        local: &BTreeMap<String, PathBuf>,
        options: DownloadOptions,
        report: &(impl Fn(ProgressUpdate) + Sync),
    ) -> (Result<(), String>, Option<String>) {
        let report_pack = |state| report(ProgressUpdate::Pack(state));
        if self.version.is_some() {
            report(ProgressUpdate::Game(GameState::Ready));
            return (
                self.pack
                    .prepare(local, options.concurrency(), &report_pack),
                None,
            );
        }
        let (files, game) = prepare_together(self, local, options, report);
        match game {
            Ok(version) => {
                self.version = Some(version);
                (files, None)
            }
            Err(error) => (files, Some(format!("Could not prepare Minecraft. {error}"))),
        }
    }
}

fn prepare_together(
    install: &mut PendingInstall,
    local: &BTreeMap<String, PathBuf>,
    options: DownloadOptions,
    report: &(impl Fn(ProgressUpdate) + Sync),
) -> (Result<(), String>, Result<String, String>) {
    let concurrency = options.concurrency();
    let (data, game) = (&install.data, &install.game);
    thread::scope(|scope| {
        // Java processors may block while running. The game worker stays
        // independent and is joined before the install can publish or pause.
        let worker = thread::Builder::new()
            .name("modpack-game".into())
            .spawn_scoped(scope, move || prepare_game(data, game, options, report));
        let files = install.pack.prepare(local, concurrency, &|state| {
            report(ProgressUpdate::Pack(state))
        });
        let game = worker
            .map_err(|error| error.to_string())
            .and_then(|worker| {
                worker
                    .join()
                    .map_err(|_| "Minecraft preparation stopped unexpectedly.".to_owned())?
            });
        (files, game)
    })
}

fn install_runtime() -> Result<tokio::runtime::Runtime, String> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())
}

fn prepare_game(
    data: &DataDir,
    game: &Game,
    options: DownloadOptions,
    report: &impl Fn(ProgressUpdate),
) -> Result<String, String> {
    let result = download_game(data, game, options, |stage| {
        report(ProgressUpdate::Game(GameState::Preparing(stage)));
    });
    let state = if result.is_ok() {
        GameState::Ready
    } else {
        GameState::Failed
    };
    report(ProgressUpdate::Game(state));
    result
}

fn download_game(
    data: &DataDir,
    game: &Game,
    options: DownloadOptions,
    report: impl Fn(GameProgress),
) -> Result<String, String> {
    let target = InstallTarget::for_game(game).ok_or("No supported installer for this game.")?;
    installation::prepare(data, target, options, report)
}

#[cfg(test)]
#[path = "../../../../tests/apps/frontend/src/modpacks/service.rs"]
mod tests;
