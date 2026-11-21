// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! The command line a launch runs with.
//!
//! The game's document writes `${placeholder}` tokens; this module fills them
//! from a fixed set of values and leaves anything unknown untouched, so a
//! placeholder this build does not know becomes a visible literal in the
//! command rather than a silent empty argument.

use std::collections::BTreeMap;

use crate::rules::{self, Target};
use crate::version::Arguments;

/// Values substituted into the game's arguments.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Values(BTreeMap<String, String>);

impl Values {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.0.insert(key.into(), value.into());
        self
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(String::as_str)
    }
}

/// Replaces every known `${key}` in `argument`.
///
/// Unknown keys are left in place on purpose: an empty string would produce a
/// command that looks valid and fails at runtime in a harder place to read.
pub fn substitute(argument: &str, values: &Values) -> String {
    let mut result = String::with_capacity(argument.len());
    let mut rest = argument;

    while let Some(start) = rest.find("${") {
        result.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            result.push_str(&rest[start..]);
            return result;
        };
        let key = &after[..end];
        match values.get(key) {
            Some(value) => result.push_str(value),
            None => result.push_str(&rest[start..start + 2 + end + 1]),
        }
        rest = &after[end + 1..];
    }

    result.push_str(rest);
    result
}

/// The jvm arguments for `target`, substituted.
pub fn jvm(arguments: &Arguments, target: &Target, values: &Values) -> Vec<String> {
    rules::applicable(&arguments.jvm, target)
        .iter()
        .map(|argument| substitute(argument, values))
        .collect()
}

/// The game arguments for `target`, substituted.
pub fn game(arguments: &Arguments, target: &Target, values: &Values) -> Vec<String> {
    rules::applicable(&arguments.game, target)
        .iter()
        .map(|argument| substitute(argument, values))
        .collect()
}

#[cfg(test)]
mod tests {
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
    fn applicable_arguments_are_substituted_and_ordered() {
        let arguments = Arguments {
            jvm: vec![
                Argument::Plain("-Xmx2G".into()),
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
}
