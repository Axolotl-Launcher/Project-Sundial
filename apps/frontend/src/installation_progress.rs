// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use axlauncher_core::install::prepare::Progress;

use crate::i18n::tr;

const PROGRESS_UPDATES: usize = 100;

pub(crate) fn progress_message(stage: Progress, cx: &axlauncher_ui::App) -> String {
    match stage {
        Progress::Version => tr(cx, "install.progress.manifest").into(),
        Progress::Loader => tr(cx, "install.progress.loader").into(),
        Progress::Java => tr(cx, "install.progress.java").into(),
        Progress::CheckingFiles => tr(cx, "install.progress.checking").into(),
        Progress::Downloading { processed, total } => crate::i18n::interpolate(
            tr(cx, "install.progress.downloading"),
            &[
                ("processed", &processed.to_string()),
                ("total", &total.to_string()),
            ],
        ),
        Progress::Finishing => tr(cx, "install.progress.finishing").into(),
    }
}

/// Named phases and the final count reach the view even when file ticks are throttled.
pub(crate) fn should_report(stage: Progress) -> bool {
    match stage {
        Progress::Downloading { processed, total } => {
            processed == total || processed.is_multiple_of((total / PROGRESS_UPDATES).max(1))
        }
        _ => true,
    }
}
