// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! One call that carries a version document on disk to a started game.
//!
//! The steps are fixed and nothing else happens here: no download, no login,
//! no update. Each step reports its own failure, so a caller can tell a missing
//! document from a missing Java or a missing client jar.

use std::path::{Path, PathBuf};

use axlauncher_spi::{Java, JavaMode, LaunchCommand, LaunchError, Launcher};

use crate::jar::JarReader;
use crate::natives::{self, NativeError};
use crate::plan::{self, Layout, PlanError, Session};
use crate::rules::Target;
use crate::version::{self, Version, VersionError};

/// The directory under a game root that holds version documents.
const VERSIONS: &str = "versions";

/// The directory under a game root that holds library jars.
const LIBRARIES: &str = "libraries";

/// The directory under an instance that receives unpacked natives.
const NATIVES: &str = "natives";

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
    /// The command the launcher was given.
    pub command: LaunchCommand,
    /// How many native files were unpacked.
    pub natives: usize,
}

/// Why a launch stopped before the game was running.
#[derive(Debug)]
pub enum FlowError {
    /// The version document could not be read or parsed.
    VersionDocument {
        /// The read or parse failure, which names the document.
        source: VersionError,
    },
    /// No Java runtime was found.
    NoJava {
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
            Self::NoJava {
                preferred: Some(path),
            } => write!(
                formatter,
                "no Java found at {} or on this system",
                path.display()
            ),
            Self::NoJava { preferred: None } => write!(formatter, "no Java found on this system"),
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
            Self::Natives { source, .. } => Some(source),
            Self::Plan { source } => Some(source),
            Self::Launch { source, .. } => Some(source),
            Self::NoJava { .. } => None,
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
    let target = Target::host();
    let version = load_version(&request)?;
    let executable = java_executable(java, &request)?;
    let natives = unpack_natives(&request, &version, &target)?;
    let command = assemble(&executable, &version, &request, &target)?;

    launcher
        .launch(command.clone())
        .await
        .map_err(|source| FlowError::Launch {
            program: executable,
            source,
        })?;

    Ok(LaunchOutcome { command, natives })
}

/// The version document `<game_root>/versions/<id>/<id>.json` describes.
fn load_version(request: &LaunchRequest) -> Result<Version, FlowError> {
    let root = request.game_root.join(VERSIONS);
    version::load(&root, &request.version_id)
        .map_err(|source| FlowError::VersionDocument { source })
}

/// The Java executable the game runs on, if this system offers one.
fn java_executable(java: &impl Java, request: &LaunchRequest) -> Result<PathBuf, FlowError> {
    java.executable(request.java_preference.as_deref(), JavaMode::Windowed)
        .ok_or_else(|| FlowError::NoJava {
            preferred: request.java_preference.clone(),
        })
}

/// The directory an instance's natives are unpacked into.
fn natives_directory(request: &LaunchRequest) -> PathBuf {
    request.instance_directory.join(NATIVES)
}

/// The number of native files unpacked into the instance's natives directory.
fn unpack_natives(
    request: &LaunchRequest,
    version: &Version,
    target: &Target,
) -> Result<usize, FlowError> {
    let directory = natives_directory(request);
    let entries = natives::entries(&version.libraries, target);
    let libraries = request.game_root.join(LIBRARIES);

    match natives::extract(&JarReader, &libraries, &entries, &directory) {
        Ok(extracted) => Ok(extracted.files.len()),
        Err(source) => Err(FlowError::Natives { directory, source }),
    }
}

/// The command that runs `version` on `executable`, as `request` describes it.
fn assemble(
    executable: &Path,
    version: &Version,
    request: &LaunchRequest,
    target: &Target,
) -> Result<LaunchCommand, FlowError> {
    let layout = Layout {
        game_root: request.game_root.clone(),
        game_directory: request.instance_directory.clone(),
        natives_directory: natives_directory(request),
        launcher_name: LAUNCHER_NAME.to_owned(),
        launcher_version: LAUNCHER_VERSION.to_owned(),
    };
    let session = Session::offline(request.player_name.clone());

    plan::build(executable, version, &layout, &session, target)
        .map_err(|source| FlowError::Plan { source })
}

#[cfg(test)]
mod tests;
