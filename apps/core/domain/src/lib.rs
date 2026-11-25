// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Domain types shared by every system.

pub mod maven;

/// A portable directory name for a Minecraft version. The official catalog
/// includes IDs with internal spaces, including historical pre-releases.
pub fn is_version_id(id: &str) -> bool {
    if id.is_empty()
        || id.ends_with(['.', ' '])
        || id.contains(['/', '\\', ':', '<', '>', '"', '|', '?', '*'])
        || id.chars().any(char::is_control)
    {
        return false;
    }
    // A device name remains reserved on Windows even with an extension.
    let stem = id
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ');
    !matches!(
        stem.to_ascii_uppercase().as_str(),
        "CON" | "PRN" | "AUX" | "NUL"
    ) && !is_numbered_device(stem)
}

fn is_numbered_device(stem: &str) -> bool {
    let stem = stem.to_ascii_uppercase();
    stem.strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"))
        .is_some_and(|suffix| {
            matches!(suffix.as_bytes(), [b'1'..=b'9']) || matches!(suffix, "¹" | "²" | "³")
        })
}

/// A Minecraft language code in its modern spelling, or an empty preference.
pub fn normalize_game_language(value: &str) -> Option<String> {
    let value = value.trim();
    if !value.is_empty()
        && !value
            .split(['-', '_'])
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_alphanumeric()))
    {
        return None;
    }
    Some(value.replace('-', "_").to_ascii_lowercase())
}

/// A stable identifier for anything the launcher persists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Id(u64);

impl Id {
    pub fn new(raw: u64) -> Self {
        Self(raw)
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/domain/src/lib.rs"]
mod tests;
