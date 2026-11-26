// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! One call that carries a version document on disk to a started game.
//!
//! The steps are fixed and nothing else happens here: no download, no login,
//! no update. Each step reports its own failure, so a caller can tell a missing
//! document from a missing Java or a missing client jar.

use std::path::{Path, PathBuf};

use axlauncher_spi::{Java, JavaMode, LaunchCommand, LaunchError, Launcher};
use axlauncher_storage::{GameDir, instance_options};

use crate::chain::{self, ChainError};
use crate::jar::JarReader;
use crate::language::GameLanguage;
use crate::natives::{self, NativeError};
use crate::plan::{self, Layout, PlanError, Session};
use crate::rules::Target;
use crate::version::{self, Version, VersionError};

/// The launcher name the game is told about.
const LAUNCHER_NAME: &str = "Axolotl Launcher";

/// The launcher version the game is told about.
const LAUNCHER_VERSION: &str = env!("CARGO_PKG_VERSION");
const LEGACY_JAVA: u32 = 8;

/// Everything a launch needs from its caller.
#[derive(Debug, Clone)]
pub struct LaunchRequest {
    /// The directory holding `versions/`, `libraries/` and `assets/`.
    pub game_root: GameDir,
    /// The instance's own directory, which the game writes its files into.
    pub instance_directory: PathBuf,
    /// The version to launch, as its directory under `versions/` names it.
    pub version_id: String,
    /// The user's Java executable, when they configured one.
    pub java_preference: Option<PathBuf>,
    /// Who the game will think is playing.
    pub player_name: String,
    /// The initial language and the original Minecraft version it applies to.
    pub game_language: Option<GameLanguage>,
}

/// What a completed launch produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOutcome {
    /// The command the launcher was given, with account tokens redacted.
    pub command: LaunchCommand,
    /// How many native files were unpacked.
    pub natives: usize,
}

/// A resolved version with the parent declared by its source and matching Java.
pub struct VersionInspection {
    version: Version,
    parent: Option<String>,
    java: Option<PathBuf>,
}

impl VersionInspection {
    pub fn version(&self) -> &Version {
        &self.version
    }

    pub fn parent(&self) -> Option<&str> {
        self.parent.as_deref()
    }

    pub fn java(&self) -> Option<&Path> {
        self.java.as_deref()
    }
}

/// Why a launch stopped before the game was running.
#[derive(Debug)]
pub enum FlowError {
    /// The version document could not be read or parsed.
    VersionDocument {
        /// The read or parse failure, which names the document.
        source: VersionError,
    },
    /// The version inherits from something that cannot be resolved.
    Chain {
        /// The cycle or depth failure behind it.
        source: ChainError,
    },
    /// No Java runtime was found.
    NoJava,
    /// No runtime of the major version the game needs was found.
    NoMatchingJava {
        /// The major version the version document names.
        required: u32,
    },
    /// The caller could not read the configured Java selection.
    JavaConfiguration { source: std::io::Error },
    /// The native jars could not be unpacked.
    Natives {
        /// The directory the natives were to be written into.
        directory: PathBuf,
        /// The jar or filesystem failure behind it.
        source: NativeError,
    },
    /// The launch command could not be assembled.
    Plan {
        /// The plan failure, which names the file it needed.
        source: PlanError,
    },
    /// The instance options could not be prepared before launch.
    GameOptions {
        path: PathBuf,
        source: std::io::Error,
    },
    /// The launcher could not start the game.
    Launch {
        /// The executable that was to be started.
        program: PathBuf,
        /// What the launcher reported.
        source: LaunchError,
    },
}

impl std::fmt::Display for FlowError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::VersionDocument { source } => {
                write!(formatter, "cannot load the version document: {source}")
            }
            Self::Chain { source } => {
                write!(formatter, "cannot resolve the version chain: {source}")
            }
            Self::NoJava => write!(formatter, "no Java found on this system"),
            Self::NoMatchingJava { required } => {
                write!(formatter, "no Java {required} runtime found on this system")
            }
            Self::JavaConfiguration { source } => {
                write!(formatter, "cannot load the Java configuration: {source}")
            }
            Self::Natives { directory, source } => write!(
                formatter,
                "cannot unpack natives into {}: {source}",
                directory.display()
            ),
            Self::Plan { source } => {
                write!(formatter, "cannot assemble the launch command: {source}")
            }
            Self::GameOptions { path, source } => {
                write!(
                    formatter,
                    "cannot set the game language in {}: {source}",
                    path.display()
                )
            }
            Self::Launch { program, source } => {
                write!(formatter, "{} {source}", program.display())
            }
        }
    }
}

impl std::error::Error for FlowError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::VersionDocument { source } => Some(source),
            Self::Chain { source } => Some(source),
            Self::Natives { source, .. } => Some(source),
            Self::Plan { source } => Some(source),
            Self::GameOptions { source, .. } | Self::JavaConfiguration { source } => Some(source),
            Self::Launch { source, .. } => Some(source),
            Self::NoJava | Self::NoMatchingJava { .. } => None,
        }
    }
}

/// Takes a version document on disk to a game the launcher has started.
///
/// The steps run in a fixed order: the host target, the version document, the
/// Java executable, the natives, the command, and finally the launcher port.
pub async fn launch(
    java: &impl Java,
    launcher: &impl Launcher,
    request: LaunchRequest,
) -> Result<LaunchOutcome, FlowError> {
    let session = Session::offline(request.player_name.clone());
    launch_with_session(java, launcher, request, &session).await
}

/// Starts Minecraft with a session already resolved by the caller.
pub async fn launch_with_session(
    java: &impl Java,
    launcher: &impl Launcher,
    request: LaunchRequest,
    session: &Session,
) -> Result<LaunchOutcome, FlowError> {
    let target = Target::host();
    let (version, _) = load_version(&request)?;
    let executable = java_executable(java, &version, &request)?;
    let natives = unpack_natives(&request, &version, &target)?;
    let mut command = assemble(&executable, &version, &request, session, &target)?;
    prepare_game_language(&request)?;

    launcher
        .launch_redacting(command.clone(), session.console_secret())
        .await
        .map_err(|source| FlowError::Launch {
            program: executable,
            source,
        })?;

    session.redact_command(&mut command);
    Ok(LaunchOutcome { command, natives })
}

fn prepare_game_language(request: &LaunchRequest) -> Result<(), FlowError> {
    let Some(language) = &request.game_language else {
        return Ok(());
    };
    let time = version::release_time(&request.game_root.versions(), language.minecraft())
        .map_err(|source| FlowError::VersionDocument { source })?;
    let path = instance_options(&request.instance_directory);
    language
        .apply(time.as_deref(), &path)
        .map_err(|source| FlowError::GameOptions { path, source })
}

/// Builds the command without extracting natives or launching the game.
pub fn preview(java: &impl Java, request: &LaunchRequest) -> Result<LaunchCommand, FlowError> {
    let session = Session::offline(request.player_name.clone());
    preview_with_session(java, request, &session)
}

/// Builds a diagnostic command with any Minecraft access token redacted.
pub fn preview_with_session(
    java: &impl Java,
    request: &LaunchRequest,
    session: &Session,
) -> Result<LaunchCommand, FlowError> {
    let (version, _) = load_version(request)?;
    let executable = java_executable(java, &version, request)?;
    assemble(
        &executable,
        &version,
        request,
        &session.redacted(),
        &Target::host(),
    )
}

/// Inspects the local version even when its matching Java runtime is unavailable.
pub fn inspect_version(
    java: &impl Java,
    request: &LaunchRequest,
) -> Result<VersionInspection, FlowError> {
    let (version, parent) = load_version(request)?;
    let java = java_executable(java, &version, request).ok();
    Ok(VersionInspection {
        version,
        parent,
        java,
    })
}

/// Loads and resolves the local version document for this request.
///
/// A loader profile is resolved against the version it inherits from first: on
/// its own it names no asset index, no client jar and no vanilla libraries, so
/// launching it unresolved would start the game with nothing to run.
fn load_version(request: &LaunchRequest) -> Result<(Version, Option<String>), FlowError> {
    let root = request.game_root.versions();
    let document = version::load(&root, &request.version_id)
        .map_err(|source| FlowError::VersionDocument { source })?;
    let parent = document.inherits_from.clone();
    let version = chain::resolve(&root, document).map_err(|source| match source {
        ChainError::Document { source } => FlowError::VersionDocument { source },
        other => FlowError::Chain { source: other },
    })?;
    Ok((version, parent))
}

/// The Java executable the game runs on, if this system offers one.
///
/// The version document states the major version its classes were built for. A
/// document with flat arguments and no Java requirement prefers Java 8 when it
/// is installed. An available explicit choice takes precedence regardless of
/// the manifest's requirement.
fn java_executable(
    java: &impl Java,
    version: &Version,
    request: &LaunchRequest,
) -> Result<PathBuf, FlowError> {
    if let Some(path) = &request.java_preference
        && let Some(executable) = java.executable_at(path, None, JavaMode::Windowed)
    {
        return Ok(executable);
    }
    if let Some(required) = version.java_version.as_ref().map(|java| java.major) {
        return java
            .executable_for(required, JavaMode::Windowed)
            .ok_or(FlowError::NoMatchingJava { required });
    }
    if version.minecraft_arguments.is_some()
        && let Some(legacy) = java.executable_for(LEGACY_JAVA, JavaMode::Windowed)
    {
        return Ok(legacy);
    }
    java.executable(JavaMode::Windowed).ok_or(FlowError::NoJava)
}

/// The shared directory for this version's unpacked natives.
fn natives_directory(request: &LaunchRequest) -> PathBuf {
    request.game_root.version_natives(&request.version_id)
}

/// The number of native files unpacked into the instance's natives directory.
fn unpack_natives(
    request: &LaunchRequest,
    version: &Version,
    target: &Target,
) -> Result<usize, FlowError> {
    let directory = natives_directory(request);
    match natives::extract_version(
        &JarReader,
        &request.game_root,
        &request.version_id,
        &version.libraries,
        target,
    ) {
        Ok(extracted) => Ok(extracted.files.len()),
        Err(source) => Err(FlowError::Natives { directory, source }),
    }
}

/// The command that runs `version` on `executable`, as `request` describes it.
fn assemble(
    executable: &Path,
    version: &Version,
    request: &LaunchRequest,
    session: &Session,
    target: &Target,
) -> Result<LaunchCommand, FlowError> {
    let layout = Layout {
        game_root: request.game_root.clone(),
        game_directory: request.instance_directory.clone(),
        natives_directory: natives_directory(request),
        launcher_name: LAUNCHER_NAME.to_owned(),
        launcher_version: LAUNCHER_VERSION.to_owned(),
    };
    plan::build(executable, version, &layout, session, target)
        .map_err(|source| FlowError::Plan { source })
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/launch/src/flow/tests.rs"]
mod tests;
