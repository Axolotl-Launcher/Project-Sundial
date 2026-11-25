// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::BTreeMap;

use unicode_normalization::UnicodeNormalization;

use crate::PackError;

pub(crate) fn validate(path: &str) -> Result<(), PackError> {
    let mut parts = path.split('/');
    let reserved = parts.next().is_some_and(|first| {
        [".axlprofile", ".axl-runtime", "files.json"]
            .iter()
            .any(|name| first.eq_ignore_ascii_case(name))
    });
    if reserved || path.len() > 4096 || !path.split('/').all(valid_segment) {
        return Err(PackError::Invalid(format!("unsafe destination {path:?}")));
    }
    Ok(())
}

fn valid_segment(segment: &str) -> bool {
    if segment.is_empty()
        || segment.len() > 255
        || segment.ends_with(['.', ' '])
        || segment
            .chars()
            .any(|c| c.is_control() || "\\:<>\"|?*".contains(c))
    {
        return false;
    }
    let stem = segment
        .split('.')
        .next()
        .unwrap_or(segment)
        .trim_end()
        .to_ascii_uppercase();
    if matches!(
        stem.as_str(),
        "CON" | "NUL" | "PRN" | "AUX" | "CONIN$" | "CONOUT$"
    ) {
        return false;
    }
    let port = stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"));
    !port.is_some_and(|number| {
        matches!(
            number,
            "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
        )
    })
}

/// Verifies one file tree on case-insensitive and Unicode-normalizing hosts.
pub(crate) fn check_tree<'a>(paths: impl IntoIterator<Item = &'a str>) -> Result<(), PackError> {
    let mut seen = BTreeMap::new();
    for path in paths {
        register_path(&mut seen, path)?;
    }
    Ok(())
}

fn register_path<'a>(
    seen: &mut BTreeMap<String, (&'a str, bool)>,
    path: &'a str,
) -> Result<(), PackError> {
    for (end, _) in path.match_indices('/').chain([(path.len(), "")]) {
        let prefix = &path[..end];
        let key: String = prefix.to_lowercase().nfc().collect();
        let is_file = end == path.len();
        let previous = seen.insert(key, (prefix, is_file));
        if previous.is_some_and(|(old, was_file)| old != prefix || was_file || is_file) {
            return Err(PackError::Invalid(format!(
                "conflicting destination {path:?}"
            )));
        }
    }
    Ok(())
}

pub(super) fn validate_version(version: &str) -> Result<(), PackError> {
    if version.is_empty()
        || version.len() > 128
        || version.ends_with('.')
        || !version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".-_+".contains(&byte))
    {
        return Err(PackError::Invalid(format!(
            "invalid game or loader version {version:?}"
        )));
    }
    Ok(())
}
