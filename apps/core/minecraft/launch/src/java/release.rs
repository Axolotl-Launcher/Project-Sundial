// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Version ordering for JEP 223 releases and legacy Java updates.

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct JavaRelease {
    major: u32,
    updates: Vec<u32>,
    stage: Stage,
    build: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Stage {
    Number(u32),
    Named(String),
    Final,
}

impl JavaRelease {
    pub(super) fn parse(version: &str) -> Option<Self> {
        let legacy = version.strip_prefix("1.");
        let version = legacy.unwrap_or(version);
        let (base, build) = version.split_once('+').unwrap_or((version, ""));
        let (numbers, suffix) = base.split_once('-').unwrap_or((base, ""));
        let mut numbers = numbers.split(['.', '_']).map(str::parse::<u32>);
        let major = numbers.next()?.ok().filter(|major| *major > 0)?;
        let mut updates = numbers.collect::<Result<Vec<_>, _>>().ok()?;
        while updates.last() == Some(&0) {
            updates.pop();
        }
        let legacy_build = legacy
            .and(suffix.strip_prefix('b'))
            .and_then(|build| build.parse().ok());
        let build = build
            .split('-')
            .next()
            .and_then(|build| build.parse().ok())
            .or(legacy_build)
            .unwrap_or(0);
        Some(Self {
            major,
            updates,
            stage: stage(suffix, legacy_build.is_some()),
            build,
        })
    }

    pub(super) fn major(&self) -> u32 {
        self.major
    }
}

fn stage(suffix: &str, legacy_build: bool) -> Stage {
    if suffix.is_empty() || legacy_build || suffix.eq_ignore_ascii_case("lts") {
        return Stage::Final;
    }
    match suffix.parse() {
        Ok(value) => Stage::Number(value),
        Err(_) => Stage::Named(suffix.to_owned()),
    }
}

#[cfg(test)]
#[path = "../../../../../../tests/apps/core/minecraft/launch/src/java/release.rs"]
mod tests;
