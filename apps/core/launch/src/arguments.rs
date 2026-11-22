// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The command line a launch runs with.
//!
//! The game's document writes `${placeholder}` tokens; this module fills them
//! from a fixed set of values and leaves anything unknown untouched, so a
//! placeholder this build does not know becomes a visible literal in the
//! command rather than a silent empty argument.

use std::collections::{BTreeMap, BTreeSet};

use crate::rules::{self, Target};
use crate::version::{Argument, Arguments};

/// Values substituted into the game's arguments.
///
/// A key absent from the map is omitted; `Resolve` turns that into a dropped
/// flag. `omit` records a name the caller knows about but has no value for, so
/// the distinction from a name this build never heard of stays visible.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Values {
    present: BTreeMap<String, String>,
    omitted: BTreeSet<String>,
}

impl Values {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.present.insert(key.into(), value.into());
        self
    }

    /// Records a name the caller has no value for, so its flag is dropped.
    pub fn omit(&mut self, key: impl Into<String>) {
        self.omitted.insert(key.into());
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.present.get(key).map(String::as_str)
    }

    /// Whether the caller deliberately has no value for `key`.
    pub fn is_omitted(&self, key: &str) -> bool {
        self.omitted.contains(key)
    }
}

/// Replaces every known `${key}` in `argument`.
///
/// Unknown keys are left in place on purpose: an empty string would produce a
/// command that looks valid and fails at runtime in a harder place to read.
/// Dropping the whole `--flag value` pair is a different decision and runs
/// through [`without_absent`].
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

/// A value the caller either has or deliberately omits.
///
/// The game writes an optional pair as `--flag ${name}`, so a value the caller
/// does not have means the flag goes too. Leaving `--flag` in place would make
/// the game read whatever follows as its value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// The caller has a value.
    Present(String),
    /// The caller has none, so the flag is omitted.
    Absent,
}

/// The values a caller can supply, and which of them it deliberately omits.
pub trait Resolve {
    /// The value for `name`, or `Absent` when the argument should be dropped.
    fn resolve(&self, name: &str) -> Value;
}

impl Resolve for Values {
    fn resolve(&self, name: &str) -> Value {
        match self.get(name) {
            Some(value) => Value::Present(value.to_string()),
            // Both an omitted name and one never seen drop the flag: the game
            // supplies its own default either way.
            None => Value::Absent,
        }
    }
}

/// The arguments for `target`, with every `${name}` replaced and every
/// `--flag ${name}` pair dropped when the caller has no such value.
///
/// A placeholder that is only part of an argument (`-Dfoo=${bar}`) is not a
/// pair, so an absent value there substitutes an empty string and leaves the
/// surrounding text as the game wrote it.
pub fn resolved(arguments: &[Argument], target: &Target, values: &impl Resolve) -> Vec<String> {
    let mut assembler = Assembler::default();
    for argument in rules::applicable(arguments, target) {
        assembler.push(&argument, values);
    }
    assembler.finish()
}

/// Collects arguments while remembering a flag that is still waiting for its
/// value.
#[derive(Debug, Default)]
struct Assembler {
    kept: Vec<String>,
    pending_flag: Option<String>,
}

impl Assembler {
    fn push(&mut self, argument: &str, values: &impl Resolve) {
        match sole_placeholder(argument) {
            Some(name) => self.push_value(name, values),
            None => self.push_text(argument, values),
        }
    }

    /// A bare `${name}` is the value of the flag before it, so an absent value
    /// takes the flag with it.
    fn push_value(&mut self, name: &str, values: &impl Resolve) {
        match values.resolve(name) {
            Value::Present(value) => {
                self.kept.extend(self.pending_flag.take());
                self.kept.push(value);
            }
            Value::Absent => self.pending_flag = None,
        }
    }

    fn push_text(&mut self, argument: &str, values: &impl Resolve) {
        self.close_pending();
        if is_flag(argument) {
            self.pending_flag = Some(argument.to_string());
        } else {
            self.kept.push(substitute_resolved(argument, values));
        }
    }

    fn close_pending(&mut self) {
        if let Some(flag) = self.pending_flag.take() {
            self.kept.push(flag);
        }
    }

    fn finish(mut self) -> Vec<String> {
        self.close_pending();
        self.kept
    }
}

/// Whether `argument` is a flag that takes the next argument as its value.
fn is_flag(argument: &str) -> bool {
    argument.starts_with("--") && !argument.contains('=')
}

/// The placeholder name when `argument` is exactly `${name}`.
fn sole_placeholder(argument: &str) -> Option<&str> {
    argument
        .strip_prefix("${")
        .and_then(|rest| rest.strip_suffix('}'))
        .filter(|name| !name.contains('}') && !name.contains('{'))
}

/// Replaces the placeholders inside an argument, using an empty string for a
/// value the caller omits.
fn substitute_resolved(argument: &str, values: &impl Resolve) -> String {
    let mut result = String::with_capacity(argument.len());
    let mut rest = argument;

    while let Some(start) = rest.find("${") {
        result.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            result.push_str(&rest[start..]);
            return result;
        };
        match values.resolve(&after[..end]) {
            Value::Present(value) => result.push_str(&value),
            Value::Absent => {}
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

/// The game arguments a version before 1.13 states as one flat string.
///
/// The string is the game's own format, not a launcher convenience: a document
/// without `arguments` has no other place to say `--username` and `--gameDir`,
/// so a build that ignores it starts the game with nothing to read. Splitting
/// on whitespace is what the format expects; the values are placeholders or
/// flags, never paths with spaces.
///
/// Options the modern list already states are dropped, together with their
/// inline values: a merged loader profile can carry the same option in both
/// shapes, and the game's parser rejects a repeated option.
pub fn legacy_game(legacy: &str, modern: &[String], values: &Values) -> Vec<String> {
    let modern_options: std::collections::HashSet<&str> = modern
        .iter()
        .filter(|argument| argument.starts_with('-') && !argument.contains('='))
        .map(String::as_str)
        .collect();

    let mut tokens = legacy.split_whitespace().peekable();
    let mut kept = Vec::new();
    while let Some(token) = tokens.next() {
        if !modern_options.contains(token) {
            kept.push(substitute(token, values));
            continue;
        }
        skip_inline_value(&mut tokens);
    }
    kept
}

/// Consumes the value that follows a dropped option, when it has one.
///
/// A value is whatever does not itself start with `-`; the game's own options
/// never take a negative number, so this cannot swallow a neighbouring flag.
fn skip_inline_value(tokens: &mut std::iter::Peekable<std::str::SplitWhitespace<'_>>) {
    let Some(next) = tokens.peek() else {
        return;
    };
    if !next.starts_with('-') {
        tokens.next();
    }
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
}
