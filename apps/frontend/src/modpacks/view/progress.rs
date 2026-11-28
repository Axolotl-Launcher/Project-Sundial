// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use crate::i18n::tr;

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

    pub(crate) fn message(self, cx: &axlauncher_ui::App) -> String {
        let pack = match self.pack {
            PackState::Planning => tr(cx, "modpacks.progress.planning").into(),
            PackState::Downloading { processed, total } => crate::i18n::interpolate(
                tr(cx, "modpacks.progress.downloading"),
                &[
                    ("processed", &processed.to_string()),
                    ("total", &total.to_string()),
                ],
            ),
            PackState::Supplying => tr(cx, "modpacks.progress.verifying").into(),
            PackState::ManualFiles => tr(cx, "modpacks.progress.manual").into(),
            PackState::Copying => tr(cx, "modpacks.progress.copying").into(),
            PackState::Ready => tr(cx, "modpacks.progress.ready").into(),
            PackState::Failed => tr(cx, "modpacks.progress.failed").into(),
            PackState::Publishing => return tr(cx, "modpacks.progress.publishing").into(),
        };
        match self.game {
            GameState::Pending => pack,
            GameState::Preparing(stage) => {
                format!("{pack} · Minecraft ({})", progress_message(stage, cx))
            }
            GameState::Ready => format!("{pack} · {}", tr(cx, "modpacks.progress.game-ready")),
            GameState::Failed => format!("{pack} · {}", tr(cx, "modpacks.progress.game-failed")),
        }
    }
}
