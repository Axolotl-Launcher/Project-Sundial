// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! The rule engine the game uses to decide which libraries and arguments apply.
//!
//! The rules are part of the game's format, not a platform quirk: a library
//! allowed only on `osx` is skipped on Windows because the document says so.
//! What differs by host is the platform the rules are evaluated against, which
//! arrives as a value rather than a compile-time branch.

use crate::version::{Argument, Library, Rule};

/// The machine a rule is evaluated against.
///
/// Names follow the game's vocabulary: `windows`, `osx`, `linux`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub os: String,
    pub arch: String,
}

impl Target {
    pub fn host() -> Self {
        Self {
            os: host_os().to_string(),
            arch: host_arch().to_string(),
        }
    }
}

/// The current host, in the game's vocabulary.
fn host_os() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "osx"
    } else {
        "linux"
    }
}

fn host_arch() -> &'static str {
    if cfg!(target_arch = "x86") {
        "x86"
    } else if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86_64"
    }
}

/// Whether an item with `rules` applies on `target`.
///
/// No rules means it always applies. Otherwise the last matching rule decides,
/// and a document that matches nothing is excluded — the game's behaviour, and
/// the reason a `disallow` after an `allow` reads as "anything but this".
pub fn allows(rules: Option<&[Rule]>, target: &Target) -> bool {
    // An empty list gates nothing, the same as an absent one; a list whose
    // rules all fail to match excludes the item, which is how the game reads it.
    let Some(rules) = rules.filter(|rules| !rules.is_empty()) else {
        return true;
    };

    // Reverse so the last matching rule is found first.
    match rules.iter().rev().find(|rule| matches(rule, target)) {
        Some(rule) => rule.action == "allow",
        None => false,
    }
}

fn matches(rule: &Rule, target: &Target) -> bool {
    let Some(os) = &rule.os else {
        return true;
    };
    os.name.as_deref().is_none_or(|name| name == target.os)
        && os.arch.as_deref().is_none_or(|arch| arch == target.arch)
}

/// Whether a library applies on `target`.
pub fn library_applies(library: &Library, target: &Target) -> bool {
    allows(library.rules.as_deref(), target)
}

/// The values of the arguments that apply on `target`, in document order.
pub fn applicable(arguments: &[Argument], target: &Target) -> Vec<String> {
    arguments
        .iter()
        .flat_map(|argument| match argument {
            Argument::Plain(value) => vec![value.clone()],
            Argument::Conditional { rules, value } => {
                if allows(Some(rules), target) {
                    value.as_slice().to_vec()
                } else {
                    Vec::new()
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::OsRule;

    fn target(os: &str, arch: &str) -> Target {
        Target {
            os: os.into(),
            arch: arch.into(),
        }
    }

    fn rule(action: &str, os: Option<&str>, arch: Option<&str>) -> Rule {
        Rule {
            action: action.into(),
            os: (os.is_some() || arch.is_some()).then(|| OsRule {
                name: os.map(str::to_string),
                arch: arch.map(str::to_string),
            }),
        }
    }

    #[test]
    fn no_rules_means_it_applies() {
        assert!(allows(None, &target("windows", "x86_64")));
        assert!(allows(Some(&[]), &target("windows", "x86_64")));
    }

    #[test]
    fn an_allow_for_another_platform_does_not_apply() {
        let rules = [rule("allow", Some("osx"), None)];
        assert!(!allows(Some(&rules), &target("windows", "x86_64")));
        assert!(allows(Some(&rules), &target("osx", "x86_64")));
    }

    #[test]
    fn the_last_matching_rule_decides() {
        let rules = [
            rule("allow", Some("windows"), None),
            rule("disallow", Some("windows"), Some("arm64")),
        ];
        assert!(allows(Some(&rules), &target("windows", "x86_64")));
        assert!(!allows(Some(&rules), &target("windows", "arm64")));
    }

    #[test]
    fn a_disallow_without_an_os_matches_everything() {
        let rules = [rule("disallow", None, None)];
        assert!(!allows(Some(&rules), &target("linux", "x86_64")));
    }

    #[test]
    fn an_arch_rule_compares_the_architecture() {
        let rules = [rule("allow", None, Some("x86"))];
        assert!(allows(Some(&rules), &target("windows", "x86")));
        assert!(!allows(Some(&rules), &target("windows", "x86_64")));
    }

    #[test]
    fn rules_that_match_nothing_exclude_the_item() {
        let rules = [rule("allow", Some("osx"), None)];
        assert!(!allows(Some(&rules), &target("linux", "x86_64")));
    }

    #[test]
    fn arguments_keep_document_order_and_skip_the_inapplicable() {
        let arguments = vec![
            Argument::Plain("-a".into()),
            Argument::Conditional {
                rules: vec![rule("allow", Some("windows"), None)],
                value: crate::version::ValueOrValues::Many(vec!["-b".into(), "-c".into()]),
            },
            Argument::Plain("-d".into()),
            Argument::Conditional {
                rules: vec![rule("allow", Some("osx"), None)],
                value: crate::version::ValueOrValues::One("-e".into()),
            },
        ];

        assert_eq!(
            applicable(&arguments, &target("windows", "x86_64")),
            ["-a", "-b", "-c", "-d"]
        );
        assert_eq!(
            applicable(&arguments, &target("osx", "x86_64")),
            ["-a", "-d", "-e"]
        );
    }
}
