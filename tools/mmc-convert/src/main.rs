// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

#![forbid(unsafe_code)]

mod artifact;
mod base;
mod jars;
mod patches;

use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use axlauncher_core::install::loader;
use axlauncher_core::launch::{Target, chain, version};
use axlauncher_core::network::ReqwestNetwork;
use axlauncher_core::state::profile::{Game, Profile, StagedProfile};
use axlauncher_core::storage::{
    AtomicFile, DataDir, DataPath, GameAccess, GameDir, VersionPath, write_atomic,
};
use axlauncher_download_engine::Direct;
use axlauncher_install_runtime::prepare;
use axlauncher_modpack::MultiMcExport;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn main() -> ExitCode {
    match run() {
        Ok(directory) => {
            println!("Converted instance: {}", directory.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<PathBuf> {
    let mut args = std::env::args_os().skip(1);
    let (Some(input), Some(data), None) = (args.next(), args.next(), args.next()) else {
        return Err(
            "Usage: axlauncher-mmc-convert <multimc-export.zip> <axolotl-data-directory>".into(),
        );
    };
    let input = PathBuf::from(input);
    let data = DataDir::isolated(data)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(convert(&input, &data))
}

async fn convert(input: &Path, data: &DataDir) -> Result<PathBuf> {
    let mut export = MultiMcExport::open(input)?;
    let requirements = export.base_game()?;
    let patch_set = patches::read(&mut export)?;
    let staging = StagedProfile::new(data.path(DataPath::Profiles))?;
    export.write_game_files(|path, input| {
        let destination = staging.directory().join(path);
        let mut file = AtomicFile::new(&destination)?;
        io::copy(input, file.file())?;
        file.commit(&destination)
    })?;
    let game_root = GameDir::isolated(staging.directory());
    let lock = game_root.lock(GameAccess::Install)?;
    let target = Target::host();
    let network = ReqwestNetwork::desktop()?;
    let base_id = base::prepare(&network, data, &game_root, &requirements, &target).await?;
    let mut effective = chain::load_resolved(&game_root, &base_id)?;
    let mut game = Game::new(requirements.minecraft()).with_isolated_runtime();
    if let Some((kind, build)) = requirements.loader() {
        game = game.with_loader(kind.as_str(), Some(build.into()));
    }
    let profile = Profile::new(export.name(), Some(game));
    set_converted_id(&mut effective)?;
    patches::apply(patch_set, &mut export, data, &game_root, &mut effective)?;
    let checked = prepare::prepare(
        &network,
        &Direct(&network),
        &game_root,
        &effective,
        &target,
        0,
    )
    .await?;
    base::complete(checked)?;
    write_atomic(
        &game_root.version_path(&effective.id, VersionPath::Document),
        &serde_json::to_vec_pretty(&effective)?,
    )?;
    loader::check_files(&game_root, &effective.id, &target)?;
    drop(lock);
    Ok(staging.publish_installed(profile, &effective.id)?)
}

fn set_converted_id(version: &mut version::Version) -> Result<()> {
    const CONVERTED_ID: &str = "axolotl-converted";
    if version.id == CONVERTED_ID || version.jar_id() == CONVERTED_ID {
        return Err("Base version conflicts with the converted profile ID".into());
    }
    version.jar = Some(version.jar_id().to_owned());
    version.id = CONVERTED_ID.into();
    Ok(())
}
