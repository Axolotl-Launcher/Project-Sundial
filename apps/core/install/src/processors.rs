// SPDX-FileCopyrightText: 2026 The Superlogical Authors
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

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use axlauncher_launch::classpath;
use axlauncher_launch::rules::Target;
use axlauncher_launch::version::{Processor, Version};

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

/// What a patch run produced.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Patched {
    /// How many steps the profile listed for the client.
    pub applicable: usize,
    /// How many steps ran.
    pub ran: usize,
    /// How many were skipped because their outputs were current.
    pub current: usize,
}

/// One step, resolved to everything needed to start it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// The tool's entry point.
    pub main_class: String,
    /// The JVM classpath the tool runs with, already joined.
    pub classpath: String,
    /// The resolved arguments.
    pub args: Vec<String>,
    /// Whether the files this step writes are already current, in which case a
    /// caller may skip it.
    pub current: bool,
}

/// Resolves the profile's client processors into runnable steps.
///
/// This reads the filesystem (to decide whether a step is current) but starts
/// nothing, so it is testable against a fixture tree and a caller can report
/// what it is about to run before running it.
pub fn plan(
    libraries: &Path,
    game_root: &Path,
    version: &Version,
    target: &Target,
) -> Result<Vec<Step>, ProcessorError> {
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
            args: processor_arguments(libraries, game_root, processor, version)?,
            current: outputs_are_current(processor, libraries, game_root, version),
        });
    }
    Ok(steps)
}

/// Runs the profile's client processors in order.
///
/// `java` is the executable the game itself will run on: a tool and the game
/// share the runtime, and a version whose tools need a newer Java than the game
/// states is a manifest error, not something to paper over.
pub fn run(
    java: &Path,
    libraries: &Path,
    working_directory: &Path,
    version: &Version,
    target: &Target,
) -> Result<Patched, ProcessorError> {
    let steps = plan(libraries, working_directory, version, target)?;
    let mut result = Patched {
        applicable: steps.len(),
        ..Patched::default()
    };

    for step in steps {
        if step.current {
            result.current += 1;
            continue;
        }
        execute(java, working_directory, &step)?;
        result.ran += 1;
    }
    Ok(result)
}

/// Starts one resolved step and waits for it.
fn execute(java: &Path, working_directory: &Path, step: &Step) -> Result<(), ProcessorError> {
    let status = Command::new(java)
        .current_dir(working_directory)
        .arg("-cp")
        .arg(&step.classpath)
        .arg(&step.main_class)
        .args(&step.args)
        .status()
        .map_err(|source| ProcessorError::Spawn {
            program: java.to_path_buf(),
            source,
        })?;

    if status.success() {
        Ok(())
    } else {
        Err(ProcessorError::Failed {
            coordinate: step.main_class.clone(),
            code: status.code(),
        })
    }
}

/// Whether a step belongs to the client the launcher starts.
fn applies(processor: &Processor) -> bool {
    match &processor.sides {
        None => true,
        Some(sides) => sides.iter().any(|side| side == CLIENT),
    }
}

/// Whether every file the step writes already exists.
///
/// A step with no `outputs` cannot prove it is current, so it runs: the format
/// treats an absent list as "always run", and guessing would skip real work.
/// One tool is the exception, because its own command line names its output:
/// a `binarypatcher` step states `--output {PATCHED}` and nothing else, and
/// without reading that the most expensive step would rerun on every launch.
fn outputs_are_current(
    processor: &Processor,
    libraries: &Path,
    game_root: &Path,
    version: &Version,
) -> bool {
    let declared: Vec<&String> = if processor.outputs.is_empty() {
        inferred_outputs(processor).into_iter().collect()
    } else {
        processor.outputs.values().collect()
    };
    if declared.is_empty() {
        return false;
    }
    declared.into_iter().all(|template| {
        resolve_template(template, libraries, version, game_root)
            .is_ok_and(|path| Path::new(&path).is_file())
    })
}

/// The outputs a step names on its own command line instead of in `outputs`.
fn inferred_outputs(processor: &Processor) -> Vec<&String> {
    if !is_binary_patcher(processor) {
        return Vec::new();
    }
    processor
        .args
        .windows(2)
        .filter(|pair| pair[0] == OUTPUT_FLAG)
        .map(|pair| &pair[1])
        .collect()
}

/// The flag a binary patcher names the jar it writes with.
const OUTPUT_FLAG: &str = "--output";

/// Whether a step runs the tool whose output argument is worth reading.
fn is_binary_patcher(processor: &Processor) -> bool {
    processor
        .jar
        .split(':')
        .nth(1)
        .is_some_and(|artifact| artifact.eq_ignore_ascii_case("binarypatcher"))
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
) -> Result<String, ProcessorError> {
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

    let joined = entries
        .iter()
        .map(|entry| entry.display().to_string())
        .collect::<Vec<_>>()
        .join(&classpath::separator(&target.os).to_string());
    Ok(joined)
}

/// The step's arguments, with every template resolved.
fn processor_arguments(
    libraries: &Path,
    game_root: &Path,
    processor: &Processor,
    version: &Version,
) -> Result<Vec<String>, ProcessorError> {
    processor
        .args
        .iter()
        .map(|argument| resolve_template(argument, libraries, version, game_root))
        .collect()
}

/// The values the launcher itself supplies, which no profile states.
///
/// A loader's processors refer to `{MINECRAFT_JAR}`, `{SIDE}` and friends as if
/// they were data entries; they are not, because only the launcher knows the
/// paths it chose. Supplying them here keeps the resolution rule uniform: a
/// template is filled from one map, and whether a value came from the profile
/// or from the launcher is not something a processor needs to know.
fn launcher_data(
    version: &Version,
    game_root: &Path,
    libraries: &Path,
) -> Vec<(&'static str, String)> {
    let jar = game_root
        .join("versions")
        .join(version.jar_id())
        .join(format!("{}.jar", version.jar_id()));
    vec![
        (SIDE, CLIENT.to_string()),
        (MINECRAFT_JAR, jar.display().to_string()),
        (MINECRAFT_VERSION, version.jar_id().to_string()),
        (ROOT, game_root.display().to_string()),
        (LIBRARY_DIR, libraries.display().to_string()),
    ]
}

/// The name of the side a processor runs for.
const SIDE: &str = "SIDE";

/// The vanilla jar the loader patches.
const MINECRAFT_JAR: &str = "MINECRAFT_JAR";

/// The game version the loader patches.
const MINECRAFT_VERSION: &str = "MINECRAFT_VERSION";

/// The game root the launcher chose.
const ROOT: &str = "ROOT";

/// The libraries directory the launcher chose.
const LIBRARY_DIR: &str = "LIBRARY_DIR";

/// One argument or output template, with `{NAME}` and `[coord]` resolved.
///
/// A `[coord]` is the whole template; a `{NAME}` may sit inside surrounding
/// text, as `{ROOT}/libraries/` does, so the replacement is a substring search
/// rather than an exact match.
fn resolve_template(
    template: &str,
    libraries: &Path,
    version: &Version,
    game_root: &Path,
) -> Result<String, ProcessorError> {
    if let Some(coordinate) = bracket(template) {
        return library_path(libraries, coordinate)
            .ok_or_else(|| ProcessorError::ToolMissing {
                coordinate: coordinate.to_string(),
            })
            .map(|path| path.display().to_string());
    }

    let mut resolved = template.to_string();
    for (name, value) in launcher_data(version, game_root, libraries) {
        resolved = resolved.replace(&format!("{{{name}}}"), &value);
    }
    for (name, entry) in &version.data {
        let value = match bracket(&entry.client) {
            Some(coordinate) => library_path(libraries, coordinate)
                .ok_or_else(|| ProcessorError::ToolMissing {
                    coordinate: coordinate.to_string(),
                })?
                .display()
                .to_string(),
            None => unquote(&entry.client).to_string(),
        };
        resolved = resolved.replace(&format!("{{{name}}}"), &value);
    }
    Ok(resolved)
}

/// The text inside `[...]`, when a template is exactly that.
fn bracket(template: &str) -> Option<&str> {
    template
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
}

/// A `data` value written in single quotes is a literal, not a path.
fn unquote(value: &str) -> &str {
    value
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
        .unwrap_or(value)
}

/// Where a coordinate's jar lives under `libraries`.
fn library_path(libraries: &Path, coordinate: &str) -> Option<PathBuf> {
    axlauncher_launch::version::maven_path(coordinate).map(|path| libraries.join(path))
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
#[path = "../../../../tests/apps/core/install/src/processors/tests.rs"]
mod tests;
