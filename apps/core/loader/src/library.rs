// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Expands Maven shorthand in loader profiles into explicit downloads.

use axlauncher_domain::maven;
use axlauncher_spi::{LoaderError, LoaderKind};
use serde_json::{Map, Value, json};

pub(super) fn native_downloads(
    kind: LoaderKind,
    name: &str,
    repository: &str,
    natives: &Map<String, Value>,
) -> Result<Value, LoaderError> {
    let mut classifiers = Map::new();
    let mut add_classifier = |classifier: String| {
        let artifact = artifact(kind, &format!("{name}:{classifier}"), repository)?;
        classifiers.insert(classifier, artifact);
        Ok::<(), LoaderError>(())
    };
    for classifier in natives.values().filter_map(Value::as_str) {
        add_classifier(classifier.replace("${arch}", "32"))?;
        if classifier.contains("${arch}") {
            add_classifier(classifier.replace("${arch}", "64"))?;
        }
    }
    Ok(json!({"classifiers": classifiers}))
}

pub(super) fn artifact(
    kind: LoaderKind,
    coordinate: &str,
    repository: &str,
) -> Result<Value, LoaderError> {
    let path = path(kind, coordinate)?;
    let repository = repository.trim_end_matches('/');
    Ok(json!({"path": path, "url": format!("{repository}/{path}")}))
}

pub(super) fn path(kind: LoaderKind, coordinate: &str) -> Result<String, LoaderError> {
    maven::path(coordinate)
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .ok_or_else(|| LoaderError::Malformed {
            loader: kind,
            reason: format!("an invalid library coordinate {coordinate:?}"),
        })
}
