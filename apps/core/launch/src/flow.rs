// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! One call that carries a version document on disk to a started game.
//!
//! The steps are fixed and nothing else happens here: no download, no login,
//! no update. Each step reports its own failure, so a caller can tell a missing
//! document from a missing Java or a missing client jar.

use std::path::{Path, PathBuf};

use axlauncher_spi::{Java, JavaMode, LaunchCommand, LaunchError, Launcher};

use crate::chain::{self, ChainError};
use crate::jar::JarReader;
use crate::natives::{self, NativeError};
use crate::plan::{self, Layout, PlanError, Session};
use crate::rules::Target;
use crate::version::{self, Version, VersionError};

/// The directory under a game root that holds version documents.
const VERSIONS: &str = "versions";

/// The launcher name the game is told about.
const LAUNCHER_NAME: &str = "Axolotl Launcher";

/// The launcher version the game is told about.
const LAUNCHER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Everything a launch needs from its caller.
#[derive(Debug, Clone)]
pub struct LaunchRequest {
    /// The directory holding `versions/`, `libraries/` and `assets/`.
    pub game_root: PathBuf,
    /// The instance's own directory, which the game writes its files into.
    pub instance_directory: PathBuf,
    /// The version to launch, as its directory under `versions/` names it.
    pub version_id: String,
    /// The user's Java executable, when they configured one.
    pub java_preference: Option<PathBuf>,
    /// Who the game will think is playing.
    pub player_name: String,
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
    NoJava {
        /// The executable the caller preferred, when one was given.
        preferred: Option<PathBuf>,
    },
    /// No runtime of the major version the game needs was found.
    NoMatchingJava {
        /// The major version the version document names.
        required: u32,
        /// The executable the caller preferred, when one was given.
        preferred: Option<PathBuf>,
    },
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
            Self::NoJava {
                preferred: Some(path),
            } => write!(
                formatter,
                "no Java found at {} or on this system",
                path.display()
            ),
            Self::NoJava { preferred: None } => write!(formatter, "no Java found on this system"),
            Self::NoMatchingJava {
                required,
                preferred: Some(path),
            } => write!(
                formatter,
                "no Java {required} runtime found for {} or on this system",
                path.display()
            ),
            Self::NoMatchingJava {
                required,
                preferred: None,
            } => write!(formatter, "no Java {required} runtime found on this system"),
            Self::Natives { directory, source } => write!(
                formatter,
                "cannot unpack natives into {}: {source}",
                directory.display()
            ),
            Self::Plan { source } => {
                write!(formatter, "cannot assemble the launch command: {source}")
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
            Self::Launch { source, .. } => Some(source),
            Self::NoJava { .. } | Self::NoMatchingJava { .. } => None,
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

    launcher
        .launch(command.clone())
        .await
        .map_err(|source| FlowError::Launch {
            program: executable,
            source,
        })?;

    session.redact_command(&mut command);
    Ok(LaunchOutcome { command, natives })
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
    let root = request.game_root.join(VERSIONS);
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
/// document without that field asks for nothing in particular, which is what
/// every manifest written before 1.17 does; those run on the launcher's default
/// runtime.
fn java_executable(
    java: &impl Java,
    version: &Version,
    request: &LaunchRequest,
) -> Result<PathBuf, FlowError> {
    let preferred = request.java_preference.as_deref();

    let Some(required) = version.java_version.as_ref().map(|java| java.major) else {
        return java
            .executable(preferred, JavaMode::Windowed)
            .ok_or_else(|| FlowError::NoJava {
                preferred: request.java_preference.clone(),
            });
    };

    // A caller's preferred executable is used as given when it exists: the
    // user said which runtime to run, and second-guessing an explicit choice
    // would hide their configuration from them. The major version is checked
    // for the runtimes discovery finds, where nothing was stated.
    if let Some(path) = preferred.filter(|path| path.is_file()) {
        return Ok(path.to_path_buf());
    }
    java.executable_for(required, JavaMode::Windowed)
        .ok_or_else(|| FlowError::NoMatchingJava {
            required,
            preferred: request.java_preference.clone(),
        })
}

/// The shared directory for this version's unpacked natives.
fn natives_directory(request: &LaunchRequest) -> PathBuf {
    axlauncher_storage::game_natives(&request.game_root).join(&request.version_id)
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
