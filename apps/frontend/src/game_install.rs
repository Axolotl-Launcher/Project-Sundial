// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Worker-side preparation of the shared Minecraft files.

use std::path::Path;

use axlauncher_core::install::{
    loader as loader_install,
    prepare::{Prepared, Progress},
    runtime, vanilla,
};
use axlauncher_core::launch::version::{self, Version};
use axlauncher_core::launch::{Java, JavaMode, SystemJava, Target};
use axlauncher_core::loader::{ForgeApi, LoaderApi, NeoForgeApi};
use axlauncher_core::network::{MirrorPreference, MirrorSettings, ReqwestNetwork};
use axlauncher_core::spi::{LoaderKind, LoaderRequest};
use axlauncher_core::state::javas::{self, ChosenJava};
use axlauncher_core::state::profile::Game;
use axlauncher_core::state::{SettingsData, SourcePreference};

use crate::source_hosts::mirror_hosts;

pub(crate) struct InstallOptions {
    pub(crate) concurrency: usize,
    pub(crate) mirrors: MirrorSettings,
}

impl InstallOptions {
    pub(crate) fn from_settings(settings: Option<&SettingsData>) -> Self {
        let hosts = mirror_hosts();
        let Some(settings) = settings else {
            return Self {
                concurrency: 0,
                mirrors: MirrorSettings {
                    hosts,
                    ..MirrorSettings::default()
                },
            };
        };
        Self {
            concurrency: settings.max_concurrent_downloads as usize,
            mirrors: MirrorSettings {
                metadata: mirror_preference(settings.minecraft_metadata_source),
                files: mirror_preference(settings.minecraft_file_source),
                maven_central: mirror_preference(settings.maven_central_source),
                hosts,
            },
        }
    }
}

fn mirror_preference(preference: SourcePreference) -> MirrorPreference {
    match preference {
        SourcePreference::Auto => MirrorPreference::Auto,
        SourcePreference::OfficialOnly => MirrorPreference::OfficialOnly,
        SourcePreference::MirrorPreferred => MirrorPreference::MirrorPreferred,
    }
}

pub(crate) async fn download_vanilla(
    data_root: &Path,
    root: &Path,
    id: &str,
    options: InstallOptions,
    report: impl Fn(Progress),
) -> Result<Prepared, String> {
    let network =
        ReqwestNetwork::desktop_with_mirrors(options.mirrors).map_err(|error| error.to_string())?;
    let prepared = vanilla::install(
        &network,
        root,
        id,
        &Target::host(),
        options.concurrency,
        |stage| report_progress(&report, stage),
    )
    .await?;
    provide_java(&network, data_root, root, id, &report).await?;
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
    report: &impl Fn(Progress),
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

    report(Progress::Java);
    let installed = runtime::install(
        network,
        &axlauncher_core::storage::DataDir::new(data_root).java_versions(),
        major,
        |stage| report_progress(report, java_stage(stage)),
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

pub(crate) fn download_failure(prepared: &Prepared) -> String {
    format!(
        "Could not prepare {} file(s). {}",
        prepared.failures.len(),
        prepared.failures[0]
    )
}

pub(crate) fn progress_message(stage: Progress) -> String {
    match stage {
        Progress::Version => "Getting the version manifest".into(),
        Progress::Loader => "Resolving the mod loader".into(),
        Progress::CheckingFiles => "Checking local game files".into(),
        Progress::Java => "Getting the Java runtime".into(),
        Progress::Downloading { processed, total } => {
            format!("Processing downloads ({processed}/{total} files)")
        }
        Progress::Finishing => "Finishing installation".into(),
    }
}

pub(crate) fn install_loader(
    data_root: &Path,
    game_root: &Path,
    game: &Game,
    options: InstallOptions,
    report: impl Fn(Progress),
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
        data_root, game_root, kind, request, options, &report,
    ))
}

async fn download_loader(
    data_root: &Path,
    game_root: &Path,
    kind: LoaderKind,
    request: LoaderRequest,
    options: InstallOptions,
    report: &impl Fn(Progress),
) -> Result<String, String> {
    let network =
        ReqwestNetwork::desktop_with_mirrors(options.mirrors).map_err(|error| error.to_string())?;
    let concurrency = options.concurrency;

    let first = run_provider(
        kind,
        &network,
        &crate::java::managed(data_root),
        game_root,
        request.clone(),
        concurrency,
        report,
    )
    .await;

    let installed = match first {
        Err(loader_install::LoaderInstallError::Java { required }) => {
            let installed = runtime::install(
                &network,
                &axlauncher_core::storage::DataDir::new(data_root).java_versions(),
                required,
                |stage| report_progress(report, stage),
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
                report,
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
    report: &impl Fn(Progress),
) -> Result<loader_install::LoaderInstalled, loader_install::LoaderInstallError> {
    let api = match kind {
        LoaderKind::Fabric => LoaderApi::fabric(network.clone()),
        LoaderKind::LegacyFabric => LoaderApi::legacy_fabric(network.clone()),
        LoaderKind::Quilt => LoaderApi::quilt(network.clone()),
        LoaderKind::Forge => {
            let api = ForgeApi::new(network.clone());
            return install_with(network, java, &api, game_root, request, concurrency, report)
                .await;
        }
        LoaderKind::NeoForge => {
            let api = NeoForgeApi::new(network.clone());
            return install_with(network, java, &api, game_root, request, concurrency, report)
                .await;
        }
        other => {
            return Err(loader_install::LoaderInstallError::Vanilla(format!(
                "No provider for loader {other} is wired yet."
            )));
        }
    };
    install_with(network, java, &api, game_root, request, concurrency, report).await
}

async fn install_with(
    network: &ReqwestNetwork,
    java: &SystemJava,
    loader: &impl axlauncher_core::spi::ModLoader,
    game_root: &Path,
    request: LoaderRequest,
    concurrency: usize,
    report: &impl Fn(Progress),
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
    .install(|stage| report_progress(report, stage))
    .await
}

/// Reports a stage the panel can afford to rebuild for.
///
/// An install downloads thousands of assets and dozens of libraries, and a
/// runtime is several hundred files. Every tick rebuilds the panel, so the
/// per-file counts are collapsed to about a hundred while the named phases and
/// the final count always reach it.
fn report_progress(report: &impl Fn(Progress), stage: Progress) {
    if let Progress::Downloading { processed, total } = stage
        && processed != total
        && !processed.is_multiple_of((total / 100).max(1))
    {
        return;
    }
    report(stage);
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
#[path = "../../../tests/apps/frontend/src/game_install.rs"]
mod tests;
