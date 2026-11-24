// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Domain types shared by every system.

pub mod maven;

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
