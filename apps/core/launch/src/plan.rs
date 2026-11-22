// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Assembles a complete launch from a version document and the layout on disk.
//!
//! The result is a command, not a running process: nothing here spawns
//! anything, so the whole assembly is testable from fixture directories.

use std::path::{Path, PathBuf};

use axlauncher_spi::LaunchCommand;

use crate::arguments::{self, Values};
use crate::classpath;
use crate::rules::Target;
use crate::version::{Library, Version};

/// Where a version's files live, and the values only the launcher can supply.
#[derive(Debug, Clone)]
pub struct Layout {
    /// The directory holding `versions/`, `libraries/` and `assets/`.
    pub game_root: PathBuf,
    /// The instance's own directory, which the game writes its files into.
    pub game_directory: PathBuf,
    pub natives_directory: PathBuf,
    pub launcher_name: String,
    pub launcher_version: String,
}

/// Who the game will think is playing.
///
/// Offline values are the default for this build; a real account supplies the
/// same fields.
#[derive(Debug, Clone)]
pub struct Session {
    pub player_name: String,
    pub uuid: String,
    pub access_token: String,
    pub user_type: String,
}

impl Session {
    /// A session with no account behind it. The token is the placeholder the
    /// game accepts for offline play.
    pub fn offline(player_name: impl Into<String>) -> Self {
        Self {
            player_name: player_name.into(),
            uuid: "00000000-0000-0000-0000-000000000000".into(),
            access_token: "0".into(),
            user_type: "legacy".into(),
        }
    }
}

/// Why a launch could not be assembled.
#[derive(Debug)]
pub enum PlanError {
    NoClientJar { path: PathBuf },
}

impl std::fmt::Display for PlanError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoClientJar { path } => {
                write!(formatter, "no client jar at {}", path.display())
            }
        }
    }
}

impl std::error::Error for PlanError {}

/// The classpath separator a Windows game expects.
pub const WINDOWS: &str = "windows";

/// Assembles the command for `version`, resolved against `layout`.
///
/// `java` is the executable the caller already resolved through the Java port.
pub fn build(
    java: &Path,
    version: &Version,
    layout: &Layout,
    session: &Session,
    target: &Target,
) -> Result<LaunchCommand, PlanError> {
    let client_jar = client_jar(&layout.game_root, version.jar_id());
    if !client_jar.is_file() {
        return Err(PlanError::NoClientJar { path: client_jar });
    }

    let classpath = classpath::build(
        &layout.game_root.join("libraries"),
        &version.libraries,
        &client_jar,
        target,
    );
    let mut values = values(version, layout, session, &classpath, target);
    values.omit("auth_xuid");
    values.omit("clientid");
    values.omit("user_properties");
    let arguments = version.arguments.clone().unwrap_or_default();

    let mut args = arguments::resolved(&arguments.jvm, target, &values);
    args.extend(logging_argument(version, layout));
    args.push(version.main_class.clone());

    let modern = arguments::resolved(&arguments.game, target, &values);
    let game = match &version.minecraft_arguments {
        Some(legacy) => {
            let mut game = arguments::legacy_game(legacy, &modern, &values);
            game.extend(modern);
            game
        }
        None => modern,
    };
    args.extend(game);

    Ok(LaunchCommand {
        program: java.display().to_string(),
        args,
        working_directory: layout.game_directory.clone(),
        environment: Vec::new(),
    })
}

/// The client jar the game runs from.
pub fn client_jar(game_root: &Path, version_id: &str) -> PathBuf {
    game_root
        .join("versions")
        .join(version_id)
        .join(format!("{version_id}.jar"))
}

fn values(
    version: &Version,
    layout: &Layout,
    session: &Session,
    classpath: &str,
    target: &Target,
) -> Values {
    let assets_index = version
        .asset_index
        .as_ref()
        .map(|index| index.id.clone())
        .or_else(|| version.assets.clone())
        .unwrap_or_else(|| version.id.clone());
    let assets_root = layout.game_root.join("assets");
    let game_assets =
        flat_assets(&layout.game_root, &assets_index).unwrap_or_else(|| assets_root.clone());

    Values::new()
        .with("classpath", classpath)
        .with(
            "classpath_separator",
            classpath::separator(&target.os).to_string(),
        )
        .with(
            "natives_directory",
            layout.natives_directory.display().to_string(),
        )
        .with(
            "library_directory",
            layout.game_root.join("libraries").display().to_string(),
        )
        .with("launcher_name", layout.launcher_name.clone())
        .with("launcher_version", layout.launcher_version.clone())
        .with(
            "game_directory",
            layout.game_directory.display().to_string(),
        )
        .with("assets_root", assets_root.display().to_string())
        .with("game_assets", game_assets.display().to_string())
        .with("assets_index_name", assets_index)
        .with("version_name", version.id.clone())
        .with("version_type", version.kind.clone())
        .with("auth_player_name", session.player_name.clone())
        .with("auth_uuid", session.uuid.clone())
        .with("auth_access_token", session.access_token.clone())
        .with("auth_session", session.access_token.clone())
        .with("user_type", session.user_type.clone())
        .with("user_properties", "{}")
}

/// The argument that points the game at its logging configuration.
///
/// The document states it as `-Dlog4j.configurationFile=${path}` with the
/// configuration file beside the other game files, so the placeholder is the
/// only part this build supplies. A document without logging needs no
/// argument, and neither does one whose file has not been downloaded: the game
/// then logs with its built-in default instead of failing to read a listed
/// file.
fn logging_argument(version: &Version, layout: &Layout) -> Option<String> {
    let client = version.logging.as_ref()?.client.as_ref()?;
    let path = layout
        .game_root
        .join("assets")
        .join("log_configs")
        .join(&client.file.id);
    Some(
        client
            .argument
            .replace("${path}", &path.display().to_string()),
    )
}

/// The flat asset tree the game reads by plain path, when the index asks for
/// one.
///
/// Two flags exist and they name different trees: `virtual` (1.7–1.8 and the
/// HMCL-era `legacy` index) materialises under `assets/virtual/<id>`, while
/// `map_to_resources` (1.5 and older) materialises under `resources/`. The
/// flags live in the index document, which an install has already written, so
/// this reads it rather than guessing from a version number.
fn flat_assets(game_root: &Path, index_id: &str) -> Option<PathBuf> {
    let path = game_root
        .join("assets")
        .join("indexes")
        .join(format!("{index_id}.json"));
    let bytes = std::fs::read(path).ok()?;
    let document: serde_json::Value = serde_json::from_slice(&bytes).ok()?;

    if document.get("virtual").and_then(serde_json::Value::as_bool) == Some(true) {
        return Some(game_root.join("assets").join("virtual").join(index_id));
    }
    if document
        .get("map_to_resources")
        .and_then(serde_json::Value::as_bool)
        == Some(true)
    {
        return Some(game_root.join("resources"));
    }
    None
}

/// Whether `library` contributes jars the game expects on the classpath.
pub fn on_classpath(library: &Library) -> bool {
    library.downloads.is_some()
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/launch/src/plan/tests.rs"]
mod tests;
