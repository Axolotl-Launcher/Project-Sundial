// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use crate::version::{Argument, ValueOrValues};

fn values() -> Values {
    Values::new()
        .with("auth_player_name", "Player")
        .with("version_name", "1.21")
        .with("classpath", "a.jar:b.jar")
}

#[test]
fn a_known_placeholder_becomes_its_value() {
    assert_eq!(
        substitute("--username ${auth_player_name}", &values()),
        "--username Player"
    );
}

#[test]
fn several_placeholders_in_one_argument_are_replaced() {
    assert_eq!(
        substitute("-cp ${classpath} --version ${version_name}", &values()),
        "-cp a.jar:b.jar --version 1.21"
    );
}

#[test]
fn an_unknown_placeholder_is_left_visible() {
    assert_eq!(
        substitute("--width ${resolution_width}", &values()),
        "--width ${resolution_width}"
    );
}

#[test]
fn an_unterminated_placeholder_is_left_alone() {
    assert_eq!(
        substitute("--x ${unterminated", &values()),
        "--x ${unterminated"
    );
}

#[test]
fn text_without_placeholders_is_unchanged() {
    assert_eq!(substitute("-Xmx2G", &values()), "-Xmx2G");
}

#[test]
fn a_legacy_flat_string_contributes_its_tokens() {
    let values = Values::new()
        .with("auth_player_name", "Player")
        .with("game_directory", "C:/game");

    let arguments = legacy_game(
        "--username ${auth_player_name} --gameDir ${game_directory}",
        &[],
        &values,
    );

    assert_eq!(arguments, ["--username", "Player", "--gameDir", "C:/game"]);
}

#[test]
fn an_option_the_modern_list_states_is_not_repeated() {
    let values = Values::new().with("auth_player_name", "Player");
    let modern = vec!["--username".to_string(), "Player".to_string()];

    let arguments = legacy_game("--username ${auth_player_name} --demo", &modern, &values);

    assert_eq!(arguments, ["--demo"], "the repeated pair is dropped");
}

#[test]
fn a_legacy_value_with_a_space_still_resolves_as_one_token() {
    let values = Values::new().with("version_name", "1.20.1");

    let arguments = legacy_game("--version ${version_name}", &[], &values);

    assert_eq!(arguments, ["--version", "1.20.1"]);
}

#[test]
fn applicable_arguments_are_substituted_and_ordered() {
    let arguments = Arguments {
        jvm: vec![
            Argument::Plain("-Xmx2G".into()),
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
            Argument::Plain("-Djava.library.path=${classpath}".into()),
        ],
        game: vec![],
        default_user_jvm: vec![],
    };
    let windows = Target {
        os: "windows".into(),
        arch: "x86_64".into(),
    };

    assert_eq!(
        jvm(&arguments, &windows, &values()),
        ["-Xmx2G", "-Djava.library.path=a.jar:b.jar"]
    );
}

fn optional(name: &str) -> Argument {
    Argument::Plain(format!("--{name}"))
}

fn plain(text: &str) -> Argument {
    Argument::Plain(text.to_string())
}

fn any_target() -> Target {
    Target {
        os: "linux".into(),
        arch: "x86_64".into(),
    }
}

#[test]
fn a_pair_with_a_value_is_kept() {
    let arguments = vec![
        optional("width"),
        plain("${resolution_width}"),
        plain("--username"),
        plain("${auth_player_name}"),
    ];
    let values = Values::new()
        .with("resolution_width", "1280")
        .with("auth_player_name", "Player");

    assert_eq!(
        resolved(&arguments, &any_target(), &values),
        ["--width", "1280", "--username", "Player"]
    );
}

#[test]
fn a_pair_without_a_value_drops_the_flag_too() {
    let arguments = vec![
        plain("--username"),
        plain("${auth_player_name}"),
        optional("width"),
        plain("${resolution_width}"),
        plain("--version"),
        plain("${version_name}"),
    ];
    let values = Values::new()
        .with("auth_player_name", "Player")
        .with("version_name", "1.21");

    assert_eq!(
        resolved(&arguments, &any_target(), &values),
        ["--username", "Player", "--version", "1.21"]
    );
}

#[test]
fn a_placeholder_that_is_part_of_an_argument_is_substituted_not_dropped() {
    let arguments = vec![
        plain("-Djava.library.path=${natives_directory}"),
        plain("${classpath}"),
    ];
    let values = Values::new().with("classpath", "a.jar");

    assert_eq!(
        resolved(&arguments, &any_target(), &values),
        ["-Djava.library.path=", "a.jar"]
    );
}

#[test]
fn a_flag_with_an_equals_sign_is_not_treated_as_a_pair() {
    let arguments = vec![plain("--demo=false")];

    assert_eq!(
        resolved(&arguments, &any_target(), &Values::new()),
        ["--demo=false"]
    );
}

#[test]
fn a_trailing_flag_without_a_value_survives() {
    let arguments = vec![plain("--demo"), plain("--width")];

    assert_eq!(
        resolved(&arguments, &any_target(), &Values::new()),
        ["--demo", "--width"]
    );
}
