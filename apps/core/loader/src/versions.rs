// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Ordering loader versions, whose schemes only share being dotted numbers.
//!
//! A loader's version is not semver: Forge says `47.4.28`, NeoForge says
//! `20.2.93` and `20.2.3-beta`, and a future loader may say anything. The
//! comparison here compares numeric runs numerically and falls back to text,
//! and it treats a pre-release suffix as older than the release it suffixes.

use std::cmp::Ordering;

/// Orders two version strings.
pub(crate) fn compare(left: &str, right: &str) -> Ordering {
    let (left_core, left_pre) = split_pre(left);
    let (right_core, right_pre) = split_pre(right);

    match compare_dotted(left_core, right_core) {
        Ordering::Equal => match (left_pre, right_pre) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater,
            (Some(_), None) => Ordering::Less,
            (Some(left), Some(right)) => compare_dotted(left, right),
        },
        other => other,
    }
}

fn split_pre(version: &str) -> (&str, Option<&str>) {
    match version.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (version, None),
    }
}

fn compare_dotted(left: &str, right: &str) -> Ordering {
    let left: Vec<&str> = left.split('.').collect();
    let right: Vec<&str> = right.split('.').collect();

    for (left, right) in left.iter().zip(right.iter()) {
        let ordering = match (left.parse::<u64>(), right.parse::<u64>()) {
            (Ok(left), Ok(right)) => left.cmp(&right),
            _ => left.cmp(right),
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    left.len().cmp(&right.len())
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/loader/src/versions.rs"]
mod tests;
