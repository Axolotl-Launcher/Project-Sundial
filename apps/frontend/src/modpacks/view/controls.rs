// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::path::PathBuf;

use axlauncher_modpack::Pack;
use axlauncher_ui::{App, Checkbox, Disableable, PathPromptOptions, Window};

use crate::modpacks::Progress;

pub(super) fn choose_file(
    prompt: &'static str,
    app: &App,
) -> impl Future<Output = Result<Option<PathBuf>, String>> + use<> {
    let selection = app.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: false,
        multiple: false,
        prompt: Some(prompt.into()),
    });
    async move {
        selection
            .await
            .map_err(|error| error.to_string())?
            .map(|paths| paths.and_then(|paths| paths.into_iter().next()))
            .map_err(|error| error.to_string())
    }
}

pub(super) fn preview_title(pack: &Pack) -> String {
    if pack.version().is_empty() {
        return pack.name().into();
    }
    format!("{} · {}", pack.name(), pack.version())
}

pub(super) fn progress_label(progress: Progress) -> String {
    match progress {
        Progress::Planning => "Resolving pack files…".into(),
        Progress::Files { done, total } if done < total => {
            format!("Downloading pack files ({done}/{total})")
        }
        Progress::Files { .. } => "Preparing pack files…".into(),
        Progress::Game => "Preparing Minecraft…".into(),
    }
}

pub(super) fn restricted_download_option(
    automatic: bool,
    busy: bool,
    change: impl Fn(&bool, &mut Window, &mut App) + 'static,
) -> Checkbox {
    Checkbox::new("modpack-auto-download-restricted")
        .label("Try automatic downloads for restricted files")
        .checked(automatic)
        .disabled(busy)
        .on_change(change)
}
