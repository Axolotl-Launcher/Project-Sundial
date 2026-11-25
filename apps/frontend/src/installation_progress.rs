// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use axlauncher_core::install::prepare::Progress;

const PROGRESS_UPDATES: usize = 100;

pub(crate) fn progress_message(stage: Progress) -> String {
    match stage {
        Progress::Version => "Getting the version manifest".into(),
        Progress::Loader => "Resolving the mod loader".into(),
        Progress::Java => "Preparing the Java runtime".into(),
        Progress::CheckingFiles => "Checking local game files".into(),
        Progress::Downloading { processed, total } => {
            format!("Processing downloads ({processed}/{total} files)")
        }
        Progress::Finishing => "Finishing installation".into(),
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
