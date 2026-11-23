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
#[derive(Clone)]
pub struct Session {
    player_name: String,
    uuid: String,
    access_token: String,
    user_type: &'static str,
    agent: Option<YggdrasilAgent>,
    xuid: Option<String>,
}

const REDACTED_TOKEN: &str = "[redacted]";

/// The JVM agent routes Minecraft's session services to a third-party server.
#[derive(Clone)]
pub struct YggdrasilAgent {
    path: PathBuf,
    root: String,
    metadata: Option<String>,
}

impl YggdrasilAgent {
    pub fn new(path: PathBuf, root: impl Into<String>) -> Self {
        Self {
            path,
            root: root.into(),
            metadata: None,
        }
    }

    pub fn with_metadata(mut self, encoded: String) -> Self {
        self.metadata = Some(encoded);
        self
    }

    fn arguments(&self) -> impl Iterator<Item = String> {
        let agent = format!("-javaagent:{}={}", self.path.display(), self.root);
        let metadata = self
            .metadata
            .as_ref()
            .map(|value| format!("-Dauthlibinjector.yggdrasil.prefetched={value}"));
        [agent, "-Dauthlibinjector.side=client".to_owned()]
            .into_iter()
            .chain(metadata)
    }
}

impl Session {
    /// A session with no account behind it. The token is the placeholder the
    /// game accepts for offline play.
    pub fn offline(player_name: impl Into<String>) -> Self {
        Self::offline_profile(player_name, "00000000-0000-0000-0000-000000000000")
    }

    /// A saved offline identity with the profile UUID supplied by its owner.
    pub fn offline_profile(player_name: impl Into<String>, uuid: impl Into<String>) -> Self {
        Self {
            player_name: player_name.into(),
            uuid: uuid.into(),
            access_token: "0".into(),
            user_type: "legacy",
            agent: None,
            xuid: None,
        }
    }

    /// A Microsoft profile and its Minecraft access token.
    pub fn microsoft(
        player_name: impl Into<String>,
        uuid: impl Into<String>,
        access_token: String,
    ) -> Self {
        Self {
            player_name: player_name.into(),
            uuid: uuid.into(),
            access_token,
            user_type: "msa",
            agent: None,
            xuid: None,
        }
    }

    /// An identity for diagnostics that do not require a live token.
    pub fn microsoft_preview(player_name: impl Into<String>, uuid: impl Into<String>) -> Self {
        Self::microsoft(player_name, uuid, REDACTED_TOKEN.to_owned())
    }

    /// A server-authenticated Minecraft profile using the third-party JVM agent.
    pub fn yggdrasil(
        player_name: impl Into<String>,
        uuid: impl Into<String>,
        access_token: String,
        agent: YggdrasilAgent,
    ) -> Self {
        Self {
            player_name: player_name.into(),
            uuid: uuid.into(),
            access_token,
            user_type: "legacy",
            agent: Some(agent),
            xuid: None,
        }
    }

    pub fn yggdrasil_preview(
        player_name: impl Into<String>,
        uuid: impl Into<String>,
        agent: YggdrasilAgent,
    ) -> Self {
        Self::yggdrasil(player_name, uuid, REDACTED_TOKEN.to_owned(), agent)
    }

    pub(super) fn redacted(&self) -> Self {
        let mut session = self.clone();
        if self.has_online_token() {
            session.access_token = REDACTED_TOKEN.to_owned();
        }
        session
    }

    fn has_online_token(&self) -> bool {
        self.user_type == "msa" || self.agent.is_some()
    }

    /// Xbox account metadata for manifests that declare `auth_xuid`.
    pub fn with_xuid(mut self, xuid: Option<String>) -> Self {
        self.xuid = xuid;
        self
    }

    fn legacy_session(&self) -> String {
        if self.has_online_token() {
            return format!("token:{}:{}", self.access_token, self.uuid);
        }
        "-".to_owned()
    }

    pub(super) fn redact_command(&self, command: &mut LaunchCommand) {
        if !self.has_online_token() || self.access_token.is_empty() {
            return;
        }
        for argument in command
            .args
            .iter_mut()
            .filter(|argument| argument.contains(&self.access_token))
        {
            *argument = argument.replace(&self.access_token, REDACTED_TOKEN);
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
    let values = version_values(version, layout, &classpath, target);
    let values = account_values(values, session);
    let arguments = version.arguments.clone().unwrap_or_default();

    let mut args = jvm_arguments(&arguments, target, &values);
    args.extend(session.agent.iter().flat_map(YggdrasilAgent::arguments));
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

fn jvm_arguments(
    arguments: &crate::version::Arguments,
    target: &Target,
    values: &Values,
) -> Vec<String> {
    if !arguments.jvm.is_empty() {
        return arguments::resolved(&arguments.jvm, target, values);
    }
    [
        "-Djava.library.path=${natives_directory}",
        "-cp",
        "${classpath}",
    ]
    .map(|argument| arguments::substitute(argument, values))
    .into()
}

fn version_values(version: &Version, layout: &Layout, classpath: &str, target: &Target) -> Values {
    let assets_index = version
        .asset_index
        .as_ref()
        .map(|index| index.id.clone())
        .or_else(|| version.assets.clone())
        .unwrap_or_else(|| version.id.clone());
    let assets_root = layout.game_root.join("assets");
    let resources = layout.game_root.join("resources");
    let flat = flat_assets(&layout.game_root, &assets_index);
    let game_assets = if version.assets.as_deref() == Some("legacy")
        && (resources.is_dir() || !flat.as_ref().is_some_and(|path| path.is_dir()))
    {
        resources
    } else {
        flat.unwrap_or_else(|| assets_root.clone())
    };

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
}

fn account_values(mut values: Values, session: &Session) -> Values {
    values.omit("clientid");
    if let Some(xuid) = &session.xuid {
        values = values.with("auth_xuid", xuid.clone());
    } else {
        values.omit("auth_xuid");
    }
    values
        .with("auth_player_name", session.player_name.clone())
        .with("auth_uuid", session.uuid.clone())
        .with("auth_access_token", session.access_token.clone())
        .with("access_token", session.access_token.clone())
        .with("accessToken", session.access_token.clone())
        .with("auth_session", session.legacy_session())
        .with("user_type", session.user_type)
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
    let mut path = axlauncher_storage::game_log_configs(&layout.game_root).join(&client.file.id);
    if !path.is_file() {
        path = layout
            .game_root
            .join("assets/log_configs")
            .join(&client.file.id);
    }
    // An install downloads this file; a launch that runs before one did keeps
    // the game's own default logging rather than naming a file that is not
    // there, which log4j reports as a configuration error.
    if !path.is_file() {
        return None;
    }
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
