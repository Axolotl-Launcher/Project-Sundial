// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Transient desktop installation state for the shared Minecraft files.

use std::path::{Path, PathBuf};

use axlauncher_core::install::{
    prepare::{Prepared, Progress},
    runtime, vanilla,
};
use axlauncher_core::launch::version::{self, Version};
use axlauncher_core::launch::{Java, JavaMode, Target};
use axlauncher_core::loader::SUPPORTED_KINDS;
use axlauncher_core::network::{MirrorPreference, MirrorSettings, ReqwestNetwork};
use axlauncher_core::spi::LoaderKind;
use axlauncher_core::state::javas::{self, ChosenJava};
use axlauncher_core::state::profile::Game;
use axlauncher_core::state::{SettingsData, SourcePreference};
use axlauncher_ui::{AppContext, AsyncApp, Context, WeakEntity};
use tokio::sync::watch;

use crate::RootView;
use crate::source_hosts::mirror_hosts;

mod loader;

pub(crate) use loader::start_loader;

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
    options: InstallOptions,
    cx: &mut Context<'_, RootView>,
) {
    let (progress, mut updates) = watch::channel(Progress::Version);
    let version = id.clone();
    let work_root = root.clone();
    let task =
        cx.background_spawn(
            async move { install(&data_root, &work_root, &version, options, progress) },
        );
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
    options: InstallOptions,
    progress: watch::Sender<Progress>,
) -> Installation {
    // Reqwest needs Tokio's I/O driver. The caller runs this on a GPUI worker.
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| error.to_string())
        .and_then(|runtime| runtime.block_on(download(data_root, root, id, options, &progress)));

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
    options: InstallOptions,
    progress: &watch::Sender<Progress>,
) -> Result<Prepared, String> {
    let network =
        ReqwestNetwork::desktop_with_mirrors(options.mirrors).map_err(|error| error.to_string())?;
    let prepared = vanilla::install(
        &network,
        root,
        id,
        &Target::host(),
        options.concurrency,
        |stage| {
            if let Progress::Downloading { processed, total } = stage
                && processed != total
                && !processed.is_multiple_of((total / 100).max(1))
            {
                return;
            }
            progress.send_replace(stage);
        },
    )
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
    is_vanilla(kind) || LoaderKind::parse(kind).is_some_and(|kind| SUPPORTED_KINDS.contains(&kind))
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

/// A runtime this build installed becomes the recorded answer for its major,
/// unless the user already chose one.
///
/// `chosen` is the user's intent; a runtime an install fetched only fills a
/// major nobody answered, and replacing a choice the user made would silently
/// override their configuration.
fn record_runtime(data_root: &Path, installed: &runtime::InstalledRuntime) {
    let Ok(loaded) = javas::load(data_root) else {
        return;
    };
    let mut document = loaded.javas;
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
