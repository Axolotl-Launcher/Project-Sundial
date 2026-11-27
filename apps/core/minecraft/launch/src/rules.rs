// SPDX-FileCopyrightText: 2026 Axolotl Team
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
    // No optional manifest features are enabled by the current launch flow.
    if rule.features.values().any(|enabled| *enabled) {
        return false;
    }
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
#[path = "../../../../../tests/apps/core/minecraft/launch/src/rules.rs"]
mod tests;
