// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::path::PathBuf;

use axlauncher_modpack::{OptionId, OptionalFile, Pack};
use axlauncher_ui::{
    App, BackgroundExecutor, Checkbox, Disableable, ElementId, Label, SharedString, Window,
};

pub(super) fn data_pack_notice() -> Label {
    Label::new(
        "Data packs were saved in the instance’s datapacks folder. In vanilla Minecraft, add them when creating a world or copy them into an existing world’s datapacks folder.",
    )
}

pub(super) fn preview_title(pack: &Pack) -> String {
    if pack.version().is_empty() {
        return pack.name().into();
    }
    format!("{} · {}", pack.name(), pack.version())
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

pub(super) fn option_checkbox(
    file: &OptionalFile,
    checked: bool,
    busy: bool,
    change: impl Fn(&(OptionId, bool), &mut Window, &mut App) + 'static,
) -> Checkbox {
    let id = file.id().clone();
    let label = SharedString::from(file.label().to_owned());
    Checkbox::new(ElementId::Name(SharedString::from(id.key().to_owned())))
        .label(label.clone())
        .tooltip(label)
        .checked(checked)
        .disabled(busy)
        .on_change(move |checked, window, cx| change(&(id.clone(), *checked), window, cx))
}

pub(super) async fn inspect_path(
    path: Result<Option<PathBuf>, String>,
    executor: BackgroundExecutor,
) -> Result<Option<Pack>, String> {
    let Some(path) = path? else { return Ok(None) };
    executor
        .spawn(async move { crate::modpacks::service::inspect(&path).map(Some) })
        .await
}
