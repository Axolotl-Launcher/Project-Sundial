// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

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
