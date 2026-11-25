// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use crate::installation_progress::{progress_message, should_report};
use crate::modpacks::service::{GameState, PackState, ProgressUpdate};

#[derive(Clone, Copy, Default)]
pub(crate) struct Progress {
    pack: PackState,
    game: GameState,
}

impl Progress {
    pub(super) fn update(&mut self, change: ProgressUpdate) -> bool {
        match change {
            ProgressUpdate::Pack(pack) => self.pack = pack,
            ProgressUpdate::Game(GameState::Preparing(stage)) if !should_report(stage) => {
                return false;
            }
            ProgressUpdate::Game(game) => self.game = game,
        }
        true
    }

    pub(crate) fn message(self) -> String {
        let pack = match self.pack {
            PackState::Planning => "Resolving pack files".into(),
            PackState::Downloading { processed, total } => {
                format!("Pack downloads ({processed}/{total} files processed)")
            }
            PackState::Supplying => "Verifying selected files".into(),
            PackState::ManualFiles => "Manual files required".into(),
            PackState::Copying => "Copying bundled files".into(),
            PackState::Ready => "Pack files ready".into(),
            PackState::Failed => "Pack file error".into(),
            PackState::Publishing => return "Publishing instance".into(),
        };
        match self.game {
            GameState::Pending => pack,
            GameState::Preparing(stage) => {
                format!("{pack} · Minecraft ({})", progress_message(stage))
            }
            GameState::Ready => format!("{pack} · Minecraft ready"),
            GameState::Failed => format!("{pack} · Minecraft preparation failed"),
        }
    }
}
