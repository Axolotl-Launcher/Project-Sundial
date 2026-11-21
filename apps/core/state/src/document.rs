// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! Shared handling for the versioned documents this crate reads and writes.
//!
//! Every document carries `schema_version` at the top level. A writer must know
//! what it is about to replace, and the three answers — absent, unreadable,
//! another schema — lead to different actions, so they stay distinct.

use std::fs;
use std::io;
use std::path::Path;

const SCHEMA_KEY: &str = "schema_version";

/// What a write is about to replace.
pub(crate) enum Existing {
    Missing,
    Unreadable,
    Schema(u32),
}

/// Reads just enough of `path` to answer what a write would replace.
pub(crate) fn inspect(path: &Path) -> io::Result<Existing> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Existing::Missing),
        Err(error) => return Err(error),
    };

    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return Ok(Existing::Unreadable);
    };
    let Some(version) = value.get(SCHEMA_KEY).and_then(serde_json::Value::as_u64) else {
        return Ok(Existing::Unreadable);
    };

    Ok(Existing::Schema(u32::try_from(version).unwrap_or(u32::MAX)))
}
