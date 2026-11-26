// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Ordering loader versions, whose schemes only share being dotted numbers.
//!
//! Numeric components sort numerically, text components sort after numeric
//! ones, and a development suffix sorts before its unsuffixed release.

use std::cmp::Ordering;

/// Orders two version strings, ignoring build metadata after `+`.
pub(super) fn compare(left: &str, right: &str) -> Ordering {
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
    let version = version.split_once('+').map_or(version, |(build, _)| build);
    match version.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (version, None),
    }
}

fn compare_dotted(left: &str, right: &str) -> Ordering {
    let mut right = right.split('.');
    for left in left.split('.') {
        let Some(right) = right.next() else {
            return Ordering::Greater;
        };
        let ordering = compare_component(left, right);
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    if right.next().is_some() {
        Ordering::Less
    } else {
        Ordering::Equal
    }
}

fn compare_component(left: &str, right: &str) -> Ordering {
    use crate::java::is_number;

    match (is_number(left), is_number(right)) {
        (true, true) => {
            let left = left.trim_start_matches('0');
            let right = right.trim_start_matches('0');
            left.len().cmp(&right.len()).then_with(|| left.cmp(right))
        }
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => left.cmp(right),
    }
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/mc_version/src/maven/order.rs"]
mod tests;
