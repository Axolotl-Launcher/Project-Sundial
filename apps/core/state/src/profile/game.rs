// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct Game {
    pub minecraft: String,
    pub loader: Option<Loader>,
    pub version_id: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    isolated_runtime: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[non_exhaustive]
pub struct Loader {
    pub kind: String,
    pub version: Option<String>,
}

impl Game {
    /// A game with no loader: the vanilla version the profile names.
    pub fn new(minecraft: impl Into<String>) -> Self {
        Self {
            minecraft: minecraft.into(),
            loader: None,
            version_id: None,
            isolated_runtime: false,
        }
    }

    /// Attaches a loader to the game.
    pub fn with_loader(mut self, kind: impl Into<String>, version: Option<String>) -> Self {
        self.loader = Some(Loader {
            kind: kind.into(),
            version,
        });
        self
    }

    /// Uses the game files prepared inside this instance's runtime directory.
    pub fn with_isolated_runtime(mut self) -> Self {
        self.isolated_runtime = true;
        self
    }

    pub fn is_runtime_isolated(&self) -> bool {
        self.isolated_runtime
    }
}
