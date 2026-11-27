// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Detects runtimes declared by an enclosing application's product metadata.

use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

#[derive(Deserialize)]
struct ProductInfo {
    launch: Vec<ProductLaunch>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProductLaunch {
    java_executable_path: Option<PathBuf>,
}

pub(super) fn owns_runtime(executable: &Path) -> bool {
    let Some(home) = executable.ancestors().nth(2) else {
        return false;
    };
    if home.ends_with("Contents/Home") {
        return home
            .ancestors()
            .nth(3)
            .is_some_and(|contents| matches_runtime(contents, executable, true));
    }
    home.parent()
        .is_some_and(|root| matches_runtime(root, executable, false))
}

fn matches_runtime(root: &Path, executable: &Path, mac_bundle: bool) -> bool {
    let metadata = if mac_bundle {
        root.join("Resources/product-info.json")
    } else {
        root.join("product-info.json")
    };
    let Ok(bytes) = fs::read(metadata) else {
        return false;
    };
    let Ok(product) = serde_json::from_slice::<ProductInfo>(&bytes) else {
        return false;
    };
    product
        .launch
        .iter()
        .filter_map(|launch| launch.java_executable_path.as_deref())
        .filter_map(|path| resolve_declaration(path, mac_bundle))
        .any(|path| dunce::canonicalize(root.join(path)).is_ok_and(|path| path == executable))
}

fn resolve_declaration(path: &Path, mac_bundle: bool) -> Option<&Path> {
    // A macOS product manifest lives in Resources and refers to its sibling runtime bundle.
    let path = if mac_bundle {
        path.strip_prefix("..").ok()?
    } else {
        path
    };
    path.components()
        .all(|part| matches!(part, Component::Normal(_)))
        .then_some(path)
}

#[cfg(test)]
#[path = "../../../../../../tests/apps/core/minecraft/launch/src/java/application.rs"]
mod tests;
