// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The steps a loader runs before the game starts.
//!
//! Forge and NeoForge do not ship a ready client jar. Their profile lists
//! `processors`: small Java tools that take the vanilla jar, apply binary
//! patches, remap the result and write the jar the loader's bootstrap class
//! then launches. A launcher that skips them gets a bootstrap that cannot find
//! its game jar.
//!
//! Each step is described by its own document, and this module only implements
//! that description:
//!
//! - `jar` names the tool's Maven coordinate; the tool runs from that jar's
//!   `Main-Class`, with `classpath` on the JVM's classpath;
//! - `args` are templates: `{NAME}` is replaced from `data`, and `[coord]` is
//!   the path of a library already downloaded;
//! - `outputs` name the files the step writes, so a re-run can skip it when
//!   they are current;
//! - `sides` restricts a step to the client, the server, or both.
//!
//! Nothing here decides *what* to run; that is the profile's job. The work is
//! stateless against disk, so a failed install can be resumed.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::Duration;

use axlauncher_launch::classpath;
use axlauncher_launch::rules::Target;
use axlauncher_launch::version::{Processor, Version};
use axlauncher_spi::credentials::sanitize_child_environment;
use axlauncher_storage::GameDir;
use axlauncher_worker::Cancellation;

mod outputs;
mod templates;
use outputs::{Output, output_paths};
use templates::{launcher_data, library_path, resolve_template};

/// The manifest entry a jar's entry point is read from.
const MANIFEST: &str = "META-INF/MANIFEST.MF";

/// The prefix a manifest line gives the main class.
const MAIN_CLASS: &str = "Main-Class:";

/// The only side a client launch runs.
const CLIENT: &str = "client";

/// Why the patch stage stopped.
#[derive(Debug)]
pub enum ProcessorError {
    /// A tool could not be located under `libraries`.
    ToolMissing {
        /// The coordinate the profile named.
        coordinate: String,
    },
    /// A tool's manifest could not be read or names no entry point.
    ToolUnreadable {
        /// The tool's jar.
        path: PathBuf,
        /// What went wrong.
        reason: String,
    },
    /// A tool exited non-zero.
    Failed {
        /// The coordinate whose run failed.
        coordinate: String,
        /// The exit code, when the tool returned one.
        code: Option<i32>,
    },
    /// The tool could not be started.
    Spawn {
        /// The executable that could not be started.
        program: PathBuf,
        /// Why.
        source: io::Error,
    },
    Cancelled,
    Output {
        path: PathBuf,
        reason: String,
    },
}

impl std::fmt::Display for ProcessorError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ToolMissing { coordinate } => {
                write!(formatter, "no jar for processor {coordinate}")
            }
            Self::ToolUnreadable { path, reason } => {
                write!(formatter, "cannot read {}: {reason}", path.display())
            }
            Self::Failed { coordinate, code } => match code {
                Some(code) => write!(formatter, "processor {coordinate} exited with {code}"),
                None => write!(formatter, "processor {coordinate} was terminated"),
            },
            Self::Spawn { program, source } => {
                write!(formatter, "cannot start {}: {source}", program.display())
            }
            Self::Cancelled => formatter.write_str("Installation cancelled."),
            Self::Output { path, reason } => write!(formatter, "{}: {reason}", path.display()),
        }
    }
}

impl std::error::Error for ProcessorError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn { source, .. } => Some(source),
            _ => None,
        }
    }
}

struct Step {
    main_class: String,
    classpath: OsString,
    args: Vec<OsString>,
    outputs: Vec<Output>,
}

fn plan(
    game: &GameDir,
    game_version: &str,
    version: &Version,
    target: &Target,
) -> Result<Vec<Step>, ProcessorError> {
    let libraries = game.libraries();
    let values = launcher_data(version, game.root(), game.root(), game_version, libraries);
    let mut steps = Vec::new();
    for processor in &version.processors {
        if !applies(processor) {
            continue;
        }
        let tool =
            library_path(libraries, &processor.jar).ok_or_else(|| ProcessorError::ToolMissing {
                coordinate: processor.jar.clone(),
            })?;
        steps.push(Step {
            main_class: read_main_class(&tool)?,
            classpath: processor_classpath(libraries, &tool, processor, target)?,
            args: processor_arguments(libraries, processor, version, &values)?,
            outputs: output_paths(processor, game, version, &values)?,
        });
    }
    Ok(steps)
}

/// Runs the profile's client processors in order.
///
/// `java` is the executable the game itself will run on: a tool and the game
/// share the runtime, and a version whose tools need a newer Java than the game
/// states is a manifest error, not something to paper over.
pub async fn run(
    java: &Path,
    game: &GameDir,
    game_version: &str,
    version: &Version,
    target: &Target,
    retry: bool,
    cancel: &Cancellation,
) -> Result<(), ProcessorError> {
    for step in plan(game, game_version, version, target)? {
        if !cancel.is_pending() {
            return Err(ProcessorError::Cancelled);
        }
        if !step.outputs.is_empty() && step.outputs.iter().all(|output| output.current(retry)) {
            continue;
        }
        withdraw_stale_outputs(&step, game, retry)?;
        execute(java, game.root(), &step, cancel).await?;
        for output in &step.outputs {
            output.verify()?;
        }
    }
    Ok(())
}

fn withdraw_stale_outputs(step: &Step, game: &GameDir, retry: bool) -> Result<(), ProcessorError> {
    if !retry {
        return Ok(());
    }
    for output in &step.outputs {
        if output.current(true) {
            continue;
        }
        output.withdraw(game)?;
    }
    Ok(())
}

/// Starts one resolved step and waits for it.
async fn execute(
    java: &Path,
    working_directory: &Path,
    step: &Step,
    cancel: &Cancellation,
) -> Result<(), ProcessorError> {
    let mut command = Command::new(java);
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = sanitize_child_environment(&mut command)
        .current_dir(working_directory)
        .arg("-cp")
        .arg(&step.classpath)
        .arg(&step.main_class)
        .args(&step.args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|source| ProcessorError::Spawn {
            program: java.to_path_buf(),
            source,
        })?;

    let status = wait(&mut child, cancel).await;
    if !cancel.is_pending() {
        Err(ProcessorError::Cancelled)
    } else if status.success() {
        Ok(())
    } else {
        Err(ProcessorError::Failed {
            coordinate: step.main_class.clone(),
            code: status.code(),
        })
    }
}

async fn wait(child: &mut Child, cancel: &Cancellation) -> ExitStatus {
    const POLL_INTERVAL: Duration = Duration::from_millis(20);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status,
            Ok(None) if cancel.is_pending() => {
                tokio::select! {
                    () = tokio::time::sleep(POLL_INTERVAL) => (),
                    () = cancel.cancelled() => (),
                }
                continue;
            }
            _ => (),
        }
        let _ = child.kill();
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// Whether a step belongs to the client the launcher starts.
fn applies(processor: &Processor) -> bool {
    match &processor.sides {
        None => true,
        Some(sides) => sides.iter().any(|side| side == CLIENT),
    }
}

/// The classpath a step's tool runs with: the profile's list plus the tool's
/// own jar.
///
/// The profile's list names the tool among its entries for most loaders but not
/// all, and a JVM that cannot find the main class fails in a way that reads as a
/// corrupt download. Appending the jar, deduplicated, makes both shapes work.
///
/// The separator follows the game platform, not the host, for the same reason
/// the game's own classpath does: the JVM reading it is the game's.
fn processor_classpath(
    libraries: &Path,
    tool: &Path,
    processor: &Processor,
    target: &Target,
) -> Result<OsString, ProcessorError> {
    let mut entries: Vec<PathBuf> = Vec::with_capacity(processor.classpath.len() + 1);
    for coordinate in &processor.classpath {
        entries.push(library_path(libraries, coordinate).ok_or_else(|| {
            ProcessorError::ToolMissing {
                coordinate: coordinate.clone(),
            }
        })?);
    }
    if !entries.iter().any(|entry| entry == tool) {
        entries.push(tool.to_path_buf());
    }

    let separator = classpath::separator(&target.os).to_string();
    Ok(entries
        .iter()
        .map(|entry| entry.as_os_str())
        .collect::<Vec<_>>()
        .join(std::ffi::OsStr::new(&separator)))
}

/// The step's arguments, with every template resolved.
fn processor_arguments(
    libraries: &Path,
    processor: &Processor,
    version: &Version,
    values: &[(&str, OsString)],
) -> Result<Vec<OsString>, ProcessorError> {
    processor
        .args
        .iter()
        .map(|argument| resolve_template(argument, libraries, version, values))
        .collect()
}

/// The entry point a tool's manifest declares.
fn read_main_class(jar: &Path) -> Result<String, ProcessorError> {
    let unreadable = |reason: String| ProcessorError::ToolUnreadable {
        path: jar.to_path_buf(),
        reason,
    };

    let file = std::fs::File::open(jar).map_err(|error| unreadable(error.to_string()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|error| unreadable(format!("not a jar: {error}")))?;
    let mut manifest = archive
        .by_name(MANIFEST)
        .map_err(|_| unreadable(format!("no {MANIFEST}")))?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut manifest, &mut text)
        .map_err(|error| unreadable(error.to_string()))?;

    // A manifest may wrap a long value onto a continuation line starting with
    // one space, and a main class is short enough that this project's tools do
    // not need the fold; a wrapped value would simply not be found.
    for line in text.lines() {
        let Some(class) = line.trim().strip_prefix(MAIN_CLASS).map(str::trim) else {
            continue;
        };
        if !class.is_empty() {
            return Ok(class.to_string());
        }
    }
    Err(unreadable(format!("{MANIFEST} names no main class")))
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/lib/installation/src/processors.rs"]
mod tests;
