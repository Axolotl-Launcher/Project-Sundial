// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Transient desktop installation state for the shared Minecraft files.

use std::path::{Path, PathBuf};

use axlauncher_core::install::{
    loader as loader_install,
    prepare::{Prepared, Progress},
    runtime, vanilla,
};
use axlauncher_core::launch::version::{self, Version};
use axlauncher_core::launch::{Java, JavaMode, SystemJava, Target};
use axlauncher_core::loader::{ForgeApi, LoaderApi, NeoForgeApi};
use axlauncher_core::network::ReqwestNetwork;
use axlauncher_core::spi::{LoaderKind, LoaderRequest};
use axlauncher_core::state::javas::{self, ChosenJava};
use axlauncher_core::state::profile::Game;
use axlauncher_ui::{AppContext, AsyncApp, Context, WeakEntity};
use tokio::sync::watch;

use crate::RootView;

pub(crate) enum Installation {
    Running(Progress),
    Complete,
    Failed(String),
}

impl Installation {
    pub(crate) fn message(&self) -> String {
        match self {
            Self::Running(Progress::Version) => "Getting the version manifest".into(),
            Self::Running(Progress::Loader) => "Resolving the mod loader".into(),
            Self::Running(Progress::CheckingFiles) => "Checking local game files".into(),
            Self::Running(Progress::Java) => "Getting the Java runtime".into(),
            Self::Running(Progress::Downloading { processed, total }) => {
                format!("Processing downloads ({processed}/{total} files)")
            }
            Self::Running(Progress::Finishing) => "Finishing installation".into(),
            Self::Complete => "Game files ready".into(),
            Self::Failed(message) => message.clone(),
        }
    }
}

/// Whether a profile's loader names no loader, whatever its spelling.
///
/// A hand-edited or imported marker can spell the kind with other case or
/// surrounding space. Every action must agree on which profiles that describes,
/// or a profile is accepted in one place and refused in another.
pub(crate) fn is_vanilla(kind: &str) -> bool {
    kind.trim().eq_ignore_ascii_case("vanilla")
}

/// An original game version can be prepared without a loader installer.
pub(crate) fn vanilla_version(game: &Game) -> Option<&str> {
    if game
        .loader
        .as_ref()
        .is_some_and(|loader| !is_vanilla(&loader.kind))
    {
        return None;
    }
    let id = game.version_id.as_deref().unwrap_or(&game.minecraft);
    (id == game.minecraft).then_some(id)
}

pub(crate) fn start(
    data_root: PathBuf,
    root: PathBuf,
    id: String,
    concurrency: usize,
    cx: &mut Context<'_, RootView>,
) {
    let (progress, mut updates) = watch::channel(Progress::Version);
    let version = id.clone();
    let work_root = root.clone();
    let task = cx.background_spawn(async move {
        install(&data_root, &work_root, &version, concurrency, progress)
    });
    cx.spawn(async move |view, cx| {
        display_progress(&mut updates, &view, cx, &root, &id).await;
        let result = task.await;
        let _ = view.update(cx, |view, cx| view.set_installation(&root, &id, result, cx));
    })
    .detach();
}

async fn display_progress(
    updates: &mut watch::Receiver<Progress>,
    view: &WeakEntity<RootView>,
    cx: &mut AsyncApp,
    root: &Path,
    id: &str,
) {
    while updates.changed().await.is_ok() {
        let progress = *updates.borrow_and_update();
        let updated = view.update(cx, |view, cx| {
            view.set_installation(root, id, Installation::Running(progress), cx)
        });
        if updated.is_err() {
            return;
        }
    }
}

fn install(
    data_root: &Path,
    root: &Path,
    id: &str,
    concurrency: usize,
    progress: watch::Sender<Progress>,
) -> Installation {
    // Reqwest needs Tokio's I/O driver. The caller runs this on a GPUI worker.
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())
        .and_then(|runtime| {
            runtime.block_on(download(data_root, root, id, concurrency, &progress))
        });

    match result {
        Ok(prepared) if prepared.is_complete() => Installation::Complete,
        Ok(prepared) => Installation::Failed(download_failure(&prepared)),
        Err(error) => Installation::Failed(error),
    }
}

async fn download(
    data_root: &Path,
    root: &Path,
    id: &str,
    concurrency: usize,
    progress: &watch::Sender<Progress>,
) -> Result<Prepared, String> {
    let network = ReqwestNetwork::desktop().map_err(|error| error.to_string())?;
    let prepared = vanilla::install(&network, root, id, &Target::host(), concurrency, |stage| {
        if let Progress::Downloading { processed, total } = stage
            && processed != total
            && !processed.is_multiple_of((total / 100).max(1))
        {
            return;
        }
        progress.send_replace(stage);
    })
    .await?;
    provide_java(&network, data_root, root, id, progress).await?;
    Ok(prepared)
}

/// The Java major an installed version asks for when no runtime provides it.
///
/// A manifest before 1.17 states no `javaVersion`, and such a version runs on
/// whatever Java the machine already has. A stated major with no matching
/// runtime is the one case an install has to fetch for: launching the version
/// otherwise fails before the game starts, with a message asking the user to
/// configure `JAVA_HOME` by hand.
fn missing_java(version: &Version, available: impl FnOnce(u32) -> bool) -> Option<u32> {
    let required = version.java_version.as_ref()?.major;
    (!available(required)).then_some(required)
}

/// Fetches the runtime the installed version names, unless one already answers
/// for its major.
///
/// The game files are already on disk when this runs, so a runtime that cannot
/// be installed fails the whole install: a profile without its runtime is not
/// launchable, and the panel offers the install again.
async fn provide_java(
    network: &ReqwestNetwork,
    data_root: &Path,
    root: &Path,
    id: &str,
    progress: &watch::Sender<Progress>,
) -> Result<(), String> {
    let document = version::load(&root.join("versions"), id)
        .map_err(|error| format!("Cannot read the installed Minecraft {id} manifest: {error}"))?;
    let required = missing_java(&document, |major| {
        crate::java::managed(data_root)
            .executable_for(major, JavaMode::Windowed)
            .is_some()
    });
    let Some(major) = required else {
        return Ok(());
    };

    report_progress(progress, Progress::Java);
    let installed = runtime::install(
        network,
        &axlauncher_core::storage::DataDir::new(data_root).java_versions(),
        major,
        |stage| report_progress(progress, java_stage(stage)),
    )
    .await
    .map_err(|error| {
        format!(
            "Minecraft {id} needs Java {major}, and its runtime could not be installed: {error}"
        )
    })?;
    record_runtime(data_root, &installed);
    Ok(())
}

/// A runtime's own stages, as the panel should name them.
///
/// The runtime fetch drives the same pipeline as a game install and reports the
/// same phases, so its manifest and completion phases would otherwise appear as
/// the game's. Its per-file counts stay: they describe the same downloads.
fn java_stage(stage: Progress) -> Progress {
    match stage {
        Progress::Downloading { .. } => stage,
        _ => Progress::Java,
    }
}

fn download_failure(prepared: &Prepared) -> String {
    format!(
        "Could not prepare {} file(s). {}",
        prepared.failures.len(),
        prepared.failures[0]
    )
}

/// Whether this build has a provider for a profile's loader.
///
/// A kind the UI can show but not install must not offer an install that would
/// fail at the end of a download. The name is read through the SPI's own parser,
/// so a spelling the port accepts is not refused here.
pub(crate) fn supported_loader(kind: &str) -> bool {
    is_vanilla(kind)
        || matches!(
            LoaderKind::parse(kind),
            Some(LoaderKind::Fabric | LoaderKind::Quilt | LoaderKind::Forge | LoaderKind::NeoForge)
        )
}

/// Starts installing a loader profile.
///
/// Progress is reported under `key`, the identity the row looks the install up
/// by, because a loader profile has no version id until this install produces
/// one. The id is returned on completion, and the caller records it on the
/// profile.
pub(crate) fn start_loader(
    data_root: PathBuf,
    game_root: PathBuf,
    directory: PathBuf,
    game: Game,
    key: String,
    concurrency: usize,
    cx: &mut Context<'_, RootView>,
) {
    let (progress, mut updates) = watch::channel(Progress::Version);
    let display_key = key.clone();
    let display_root = game_root.clone();
    let work_data = data_root.clone();
    let task = cx.background_spawn(async move {
        install_loader(&work_data, &game_root, &game, concurrency, progress)
    });
    cx.spawn(async move |view, cx| {
        display_progress(&mut updates, &view, cx, &display_root, &display_key).await;
        let outcome = task.await;
        let _ = view.update(cx, |view, cx| {
            view.finish_loader_install(&data_root, &directory, &key, outcome, cx)
        });
    })
    .detach();
}

fn install_loader(
    data_root: &Path,
    game_root: &Path,
    game: &Game,
    concurrency: usize,
    progress: watch::Sender<Progress>,
) -> Result<String, String> {
    let kind = game
        .loader
        .as_ref()
        .and_then(|loader| LoaderKind::parse(&loader.kind))
        .ok_or_else(|| "This profile names no loader to install.".to_string())?;
    let request = LoaderRequest {
        game_version: game.minecraft.clone(),
        loader_version: game
            .loader
            .as_ref()
            .and_then(|loader| loader.version.clone()),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())?;
    runtime.block_on(download_loader(
        data_root,
        game_root,
        kind,
        request,
        concurrency,
        progress,
    ))
}

async fn download_loader(
    data_root: &Path,
    game_root: &Path,
    kind: LoaderKind,
    request: LoaderRequest,
    concurrency: usize,
    progress: watch::Sender<Progress>,
) -> Result<String, String> {
    let network = ReqwestNetwork::desktop().map_err(|error| error.to_string())?;

    let first = run_provider(
        kind,
        &network,
        &crate::java::managed(data_root),
        game_root,
        request.clone(),
        concurrency,
        &progress,
    )
    .await;

    let installed = match first {
        Err(loader_install::LoaderInstallError::Java { required }) => {
            let installed = runtime::install(
                &network,
                &axlauncher_core::storage::DataDir::new(data_root).java_versions(),
                required,
                |stage| report_progress(&progress, stage),
            )
            .await
            .map_err(|error| error.to_string())?;
            record_runtime(data_root, &installed);
            run_provider(
                kind,
                &network,
                &crate::java::managed(data_root),
                game_root,
                request,
                concurrency,
                &progress,
            )
            .await
            .map_err(|error| error.to_string())?
        }
        other => other.map_err(|error| error.to_string())?,
    };

    if installed.prepared.is_complete() {
        Ok(installed.version_id)
    } else {
        Err(download_failure(&installed.prepared))
    }
}

/// Each provider is its own type, so the match is over concrete values rather
/// than a trait object: the port is generic over the network provider.
async fn run_provider(
    kind: LoaderKind,
    network: &ReqwestNetwork,
    java: &SystemJava,
    game_root: &Path,
    request: LoaderRequest,
    concurrency: usize,
    progress: &watch::Sender<Progress>,
) -> Result<loader_install::LoaderInstalled, loader_install::LoaderInstallError> {
    match kind {
        LoaderKind::Fabric => {
            let api = LoaderApi::fabric(network.clone());
            install_with(
                network,
                java,
                &api,
                game_root,
                request,
                concurrency,
                progress,
            )
            .await
        }
        LoaderKind::Quilt => {
            let api = LoaderApi::quilt(network.clone());
            install_with(
                network,
                java,
                &api,
                game_root,
                request,
                concurrency,
                progress,
            )
            .await
        }
        LoaderKind::Forge => {
            let api = ForgeApi::new(network.clone());
            install_with(
                network,
                java,
                &api,
                game_root,
                request,
                concurrency,
                progress,
            )
            .await
        }
        LoaderKind::NeoForge => {
            let api = NeoForgeApi::new(network.clone());
            install_with(
                network,
                java,
                &api,
                game_root,
                request,
                concurrency,
                progress,
            )
            .await
        }
        other => Err(loader_install::LoaderInstallError::Vanilla(format!(
            "No provider for loader {other} is wired yet."
        ))),
    }
}

async fn install_with(
    network: &ReqwestNetwork,
    java: &SystemJava,
    loader: &impl axlauncher_core::spi::ModLoader,
    game_root: &Path,
    request: LoaderRequest,
    concurrency: usize,
    progress: &watch::Sender<Progress>,
) -> Result<loader_install::LoaderInstalled, loader_install::LoaderInstallError> {
    loader_install::InstallRequest {
        network,
        loader,
        java,
        game_root,
        request,
        target: &Target::host(),
        concurrency,
    }
    .install(|stage| report_progress(progress, stage))
    .await
}

/// Reports a stage the panel can afford to rebuild for.
///
/// An install downloads thousands of assets and dozens of libraries, and a
/// runtime is several hundred files. Every tick rebuilds the panel, so the
/// per-file counts are collapsed to about a hundred while the named phases and
/// the final count always reach it.
fn report_progress(progress: &watch::Sender<Progress>, stage: Progress) {
    if let Progress::Downloading { processed, total } = stage
        && processed != total
        && !processed.is_multiple_of((total / 100).max(1))
    {
        return;
    }
    let _ = progress.send(stage);
}

/// A runtime the launcher installed becomes the user's choice for its major,
/// so a later launch reuses it without discovery.
fn record_runtime(data_root: &Path, installed: &runtime::InstalledRuntime) {
    let Ok(loaded) = javas::load(data_root) else {
        return;
    };
    let mut document = loaded.javas;
    // `chosen` is the user's intent; a runtime this install fetched is only
    // what provides a still-unanswered major. Replacing a choice the user made
    // would silently override their configuration.
    if document.chosen_for(installed.major).is_none() {
        document.choose(ChosenJava {
            major_version: installed.major,
            full_version: installed.full_version.clone(),
            architecture: std::env::consts::ARCH.to_string(),
            path: installed.java.display().to_string(),
        });
    }
    let _ = javas::save(data_root, &document);
}

#[cfg(test)]
#[path = "../../../tests/apps/frontend/src/installations.rs"]
mod tests;
