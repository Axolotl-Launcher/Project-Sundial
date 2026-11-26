// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::path::PathBuf;

use axlauncher_ui::{App, PathPromptOptions};

pub(crate) fn choose_file(
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
