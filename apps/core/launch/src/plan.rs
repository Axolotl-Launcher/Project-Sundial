// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! Assembles a complete launch from a version document and the layout on disk.
//!
//! The result is a command, not a running process: nothing here spawns
//! anything, so the whole assembly is testable from fixture directories.

use std::path::{Path, PathBuf};

use superlogical_spi::LaunchCommand;

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
    let client_jar = client_jar(&layout.game_root, &version.id);
    if !client_jar.is_file() {
        return Err(PlanError::NoClientJar { path: client_jar });
    }

    let classpath = classpath::build(
        &layout.game_root.join("libraries"),
        &version.libraries,
        &client_jar,
        target,
    );
    let values = values(version, layout, session, &classpath);
    let arguments = version.arguments.clone().unwrap_or_default();

    let mut args = arguments::jvm(&arguments, target, &values);
    args.push(version.main_class.clone());
    args.extend(arguments::game(&arguments, target, &values));

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

fn values(version: &Version, layout: &Layout, session: &Session, classpath: &str) -> Values {
    let assets_index = version
        .asset_index
        .as_ref()
        .map(|index| index.id.clone())
        .or_else(|| version.assets.clone())
        .unwrap_or_else(|| version.id.clone());

    Values::new()
        .with("classpath", classpath)
        .with(
            "natives_directory",
            layout.natives_directory.display().to_string(),
        )
        .with("launcher_name", layout.launcher_name.clone())
        .with("launcher_version", layout.launcher_version.clone())
        .with(
            "game_directory",
            layout.game_directory.display().to_string(),
        )
        .with(
            "assets_root",
            layout.game_root.join("assets").display().to_string(),
        )
        .with("assets_index_name", assets_index)
        .with("version_name", version.id.clone())
        .with("version_type", version.kind.clone())
        .with("auth_player_name", session.player_name.clone())
        .with("auth_uuid", session.uuid.clone())
        .with("auth_access_token", session.access_token.clone())
        .with("user_type", session.user_type.clone())
}

/// Whether `library` contributes jars the game expects on the classpath.
pub fn on_classpath(library: &Library) -> bool {
    library.downloads.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::{Argument, Arguments, AssetIndex, ValueOrValues};
    use std::fs;

    static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("superlogical-plan-{}-{unique}", std::process::id()));
            fs::create_dir_all(&path).expect("temp directory");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn fixture(root: &Path, version_id: &str) -> (Version, Layout) {
        let jar = client_jar(root, version_id);
        fs::create_dir_all(jar.parent().expect("parent")).expect("versions");
        fs::write(&jar, b"jar").expect("client jar");
        let libraries = root.join("libraries");
        fs::create_dir_all(&libraries).expect("libraries");
        fs::create_dir_all(root.join("assets")).expect("assets");

        let version = Version {
            id: version_id.into(),
            kind: "release".into(),
            main_class: "net.minecraft.client.main.Main".into(),
            libraries: Vec::new(),
            arguments: Some(Arguments {
                game: vec![
                    Argument::Plain("--username".into()),
                    Argument::Plain("${auth_player_name}".into()),
                    Argument::Plain("--version".into()),
                    Argument::Plain("${version_name}".into()),
                    Argument::Plain("--gameDir".into()),
                    Argument::Plain("${game_directory}".into()),
                    Argument::Plain("--assetIndex".into()),
                    Argument::Plain("${assets_index_name}".into()),
                ],
                jvm: vec![
                    Argument::Plain("-Djava.library.path=${natives_directory}".into()),
                    Argument::Conditional {
                        rules: vec![crate::version::Rule {
                            action: "allow".into(),
                            os: Some(crate::version::OsRule {
                                name: Some("osx".into()),
                                arch: None,
                            }),
                        }],
                        value: ValueOrValues::One("-XstartOnFirstThread".into()),
                    },
                    Argument::Plain("-cp".into()),
                    Argument::Plain("${classpath}".into()),
                ],
                default_user_jvm: Vec::new(),
            }),
            asset_index: Some(AssetIndex { id: "34".into() }),
            assets: None,
            java_version: None,
        };

        let layout = Layout {
            game_root: root.to_path_buf(),
            game_directory: root.join("instances").join("one"),
            natives_directory: root.join("natives"),
            launcher_name: "Superlogical".into(),
            launcher_version: "0.1.0".into(),
        };

        (version, layout)
    }

    fn windows() -> Target {
        Target {
            os: "windows".into(),
            arch: "x86_64".into(),
        }
    }

    #[test]
    fn the_command_runs_the_main_class_between_the_two_argument_groups() {
        let temp = TempDir::new();
        let (version, layout) = fixture(temp.0.as_path(), "1.21");

        let command = build(
            Path::new("java"),
            &version,
            &layout,
            &Session::offline("Player"),
            &windows(),
        )
        .expect("plan");

        let main = command
            .args
            .iter()
            .position(|argument| argument == "net.minecraft.client.main.Main")
            .expect("main class");
        assert!(command.args[..main].contains(&"-cp".to_string()));
        assert!(command.args[main + 1..].contains(&"--username".to_string()));
    }

    #[test]
    fn placeholders_receive_launcher_values() {
        let temp = TempDir::new();
        let (version, layout) = fixture(temp.0.as_path(), "1.21");

        let command = build(
            Path::new("java"),
            &version,
            &layout,
            &Session::offline("Player"),
            &windows(),
        )
        .expect("plan");

        assert_eq!(command.program, "java");
        assert_eq!(command.working_directory, layout.game_directory);
        assert!(command.environment.is_empty());
        let joined = command.args.join(" ");
        assert!(joined.contains("Player"), "{joined}");
        assert!(joined.contains("1.21"), "{joined}");
        assert!(joined.contains("--assetIndex 34"), "{joined}");
        assert!(!joined.contains("${"), "unresolved placeholder in {joined}");
    }

    #[test]
    fn the_classpath_ends_with_the_client_jar() {
        let temp = TempDir::new();
        let (version, layout) = fixture(temp.0.as_path(), "1.21");

        let command = build(
            Path::new("java"),
            &version,
            &layout,
            &Session::offline("Player"),
            &windows(),
        )
        .expect("plan");

        let classpath = command
            .args
            .windows(2)
            .find(|pair| pair[0] == "-cp")
            .map(|pair| pair[1].clone())
            .expect("classpath argument");
        assert!(classpath.ends_with("1.21.jar"), "{classpath}");
    }

    #[test]
    fn a_platform_specific_argument_is_included_only_on_its_platform() {
        let temp = TempDir::new();
        let (version, layout) = fixture(temp.0.as_path(), "1.21");
        let osx = Target {
            os: "osx".into(),
            arch: "x86_64".into(),
        };

        let on_windows = build(
            Path::new("java"),
            &version,
            &layout,
            &Session::offline("P"),
            &windows(),
        )
        .expect("plan");
        let on_osx = build(
            Path::new("java"),
            &version,
            &layout,
            &Session::offline("P"),
            &osx,
        )
        .expect("plan");

        assert!(
            !on_windows
                .args
                .contains(&"-XstartOnFirstThread".to_string())
        );
        assert!(on_osx.args.contains(&"-XstartOnFirstThread".to_string()));
        assert!(on_windows.args.iter().any(|argument| argument == "-cp"));
        assert!(
            on_osx
                .args
                .windows(2)
                .any(|pair| pair[0] == "-cp" && pair[1].contains(':'))
        );
    }

    #[test]
    fn a_missing_client_jar_is_an_error_naming_the_path() {
        let temp = TempDir::new();
        let (version, layout) = fixture(temp.0.as_path(), "1.21");
        fs::remove_file(client_jar(temp.0.as_path(), "1.21")).expect("remove jar");

        let error = build(
            Path::new("java"),
            &version,
            &layout,
            &Session::offline("P"),
            &windows(),
        )
        .expect_err("must fail");

        assert!(matches!(error, PlanError::NoClientJar { .. }));
        assert!(error.to_string().contains("1.21.jar"));
    }
}
