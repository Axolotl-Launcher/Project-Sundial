// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use crate::version::{Argument, Arguments, AssetIndex, ValueOrValues};
use std::fs;

static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("axlauncher-plan-{}-{unique}", std::process::id()));
        fs::create_dir_all(&path).expect("temp directory");
        Self(path)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn sample_library() -> crate::version::Library {
    crate::version::Library {
        name: "org.example:lib:1.0".into(),
        downloads: Some(crate::version::LibraryDownloads {
            artifact: Some(crate::version::Artifact {
                path: "org/example/lib/1.0/lib-1.0.jar".into(),
                sha1: None,
                sha1_alternatives: Vec::new(),
                size: None,
                url: None,
            }),
            classifiers: None,
        }),
        natives: None,
        rules: None,
        url: None,
        include_in_classpath: true,
    }
}

fn fixture(root: &Path, version_id: &str) -> (Version, Layout) {
    let jar = GameDir::standalone(root).client_jar(version_id);
    fs::create_dir_all(jar.parent().expect("parent")).expect("versions");
    fs::write(&jar, b"jar").expect("client jar");
    fs::create_dir_all(root.join("libraries")).expect("libraries");
    fs::create_dir_all(root.join("assets")).expect("assets");

    (sample_version(version_id), layout(root))
}

fn sample_version(version_id: &str) -> Version {
    Version {
        id: version_id.into(),
        kind: "release".into(),
        main_class: "net.minecraft.client.main.Main".into(),
        libraries: vec![sample_library()],
        arguments: Some(sample_arguments()),
        asset_index: Some(AssetIndex {
            id: "34".into(),
            url: String::new(),
            sha1: None,
        }),
        assets: None,
        java_version: None,
        minecraft_arguments: None,
        inherits_from: None,
        jar: None,
        downloads: None,
        logging: None,
        processors: Vec::new(),
        data: std::collections::BTreeMap::default(),
    }
}

fn sample_arguments() -> Arguments {
    Arguments {
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
                    features: std::collections::BTreeMap::new(),
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
    }
}

#[test]
fn old_clients_receive_the_flat_asset_directory_selected_by_the_index() {
    let root = TempDir::new();
    let indexes = root.0.join("assets/indexes");
    fs::create_dir_all(&indexes).unwrap();
    fs::create_dir(root.0.join("resources")).unwrap();

    for (version_id, index_id, index, path) in [
        (
            "b1.7.3",
            "pre-1.6",
            r#"{"map_to_resources":true}"#,
            "resources",
        ),
        (
            "1.6.4",
            "legacy",
            r#"{"virtual":true}"#,
            "assets/virtual/legacy",
        ),
    ] {
        fs::write(indexes.join(format!("{index_id}.json")), index).unwrap();
        let (mut version, layout) = fixture(&root.0, version_id);
        version.arguments = None;
        version.minecraft_arguments = Some("--assetsDir ${game_assets}".into());
        version.assets = Some(index_id.into());
        version.asset_index.as_mut().unwrap().id = index_id.into();

        let command = build(
            Path::new("java"),
            &version,
            &layout,
            &Session::offline("Player"),
            &Target::host(),
            None,
        )
        .unwrap();
        assert!(
            command.args.windows(2).any(|pair| {
                pair[0] == "--assetsDir" && Path::new(&pair[1]) == root.0.join(path)
            })
        );
    }
}

fn layout(root: &Path) -> Layout {
    Layout {
        game_root: GameDir::standalone(root),
        game_directory: root.join("instances").join("one"),
        natives_directory: root.join("natives"),
        launcher_name: "Axolotl Launcher".into(),
        launcher_version: "0.1.0".into(),
    }
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
        None,
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
        None,
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
        None,
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
        None,
    )
    .expect("plan");
    let on_osx = build(
        Path::new("java"),
        &version,
        &layout,
        &Session::offline("P"),
        &osx,
        None,
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
    fs::remove_file(GameDir::standalone(&temp.0).client_jar("1.21")).expect("remove jar");

    let error = build(
        Path::new("java"),
        &version,
        &layout,
        &Session::offline("P"),
        &windows(),
        None,
    )
    .expect_err("must fail");

    assert!(matches!(error, PlanError::NoClientJar { .. }));
    assert!(error.to_string().contains("1.21.jar"));
}

#[test]
fn a_version_with_a_flat_argument_string_still_gets_game_arguments() {
    let temp = TempDir::new();
    let (mut version, layout) = fixture(temp.0.as_path(), "1.5.2");
    version.arguments = None;
    version.minecraft_arguments = Some(
        "--username ${auth_player_name} --gameDir ${game_directory} --assetsDir ${game_assets}"
            .into(),
    );

    let command = build(
        Path::new("java"),
        &version,
        &layout,
        &Session::offline("Player"),
        &windows(),
        None,
    )
    .expect("command");

    assert!(
        command
            .args
            .windows(2)
            .any(|pair| pair[0] == "--username" && pair[1] == "Player"),
        "{:?}",
        command.args
    );
    assert!(
        command
            .args
            .iter()
            .any(|argument| argument == "--assetsDir"),
        "{:?}",
        command.args
    );
}

#[test]
fn a_loader_profile_runs_the_jar_it_inherits() {
    let temp = TempDir::new();
    let (mut version, layout) = fixture(temp.0.as_path(), "1.20.1-forge");
    let game = GameDir::standalone(&temp.0);
    let vanilla = game.client_jar("1.20.1");
    fs::create_dir_all(vanilla.parent().expect("parent")).expect("versions");
    fs::write(&vanilla, b"vanilla").expect("vanilla jar");
    fs::remove_file(game.client_jar("1.20.1-forge")).expect("remove own jar");
    version.jar = Some("1.20.1".into());

    let command = build(
        Path::new("java"),
        &version,
        &layout,
        &Session::offline("Player"),
        &windows(),
        None,
    )
    .expect("command");

    assert!(
        command
            .args
            .iter()
            .any(|argument| argument.ends_with("1.20.1.jar")),
        "{:?}",
        command.args
    );
}

#[test]
fn the_logging_configuration_is_named_on_the_jvm_command_line() {
    let temp = TempDir::new();
    let (mut version, layout) = fixture(temp.0.as_path(), "1.21");
    version.logging = Some(crate::version::Logging {
        client: Some(crate::version::LoggingClient {
            argument: "-Dlog4j.configurationFile=${path}".into(),
            file: crate::version::LoggingFile {
                id: "client-1.21.xml".into(),
                sha1: None,
                size: None,
                url: String::new(),
            },
            kind: Some("log4j2-xml".into()),
        }),
    });

    // An install puts the file there; a launch without it stays silent.
    let log_config = layout
        .game_root
        .root()
        .join("log_configs")
        .join("client-1.21.xml");
    fs::create_dir_all(log_config.parent().expect("parent")).expect("log dir");
    fs::write(&log_config, b"<Configuration/>").expect("log config");

    let command = build(
        Path::new("java"),
        &version,
        &layout,
        &Session::offline("Player"),
        &windows(),
        None,
    )
    .expect("command");

    let expected = format!("-Dlog4j.configurationFile={}", log_config.display());
    assert!(
        command.args.iter().any(|argument| argument == &expected),
        "{:?}",
        command.args
    );
}
