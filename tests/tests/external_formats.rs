// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Guards the boundary in AGENTS.md for installed launcher data.
//!
//! Converting another launcher's installed data belongs to a separate tool, so a module
//! or file named for compatibility, legacy data, migration, importing or
//! conversion is a signal the rule was broken. The modpack plugin can read a
//! selected MultiMC export archive. This checks paths rather than
//! contents: the architecture lives in the module layout, and a comment is not
//! evidence either way.

use std::fs;
use std::path::{Path, PathBuf};

const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
const SOURCE_ROOTS: [&str; 2] = ["apps", "platforms"];

/// Name fragments that appear only when a foreign format is read or converted
/// inside this codebase.
const FORBIDDEN_STEMS: [&str; 5] = ["compat", "legacy", "migrat", "importer", "converter"];

#[test]
fn no_compatibility_layer_is_built_in() {
    let root = Path::new(WORKSPACE_ROOT);
    let mut scanned = 0;
    let mut offenders = Vec::new();

    for name in SOURCE_ROOTS {
        let directory = root.join(name);
        assert!(
            directory.is_dir(),
            "source root missing: {}",
            directory.display()
        );
        let (count, found) = scan(&directory, root);
        scanned += count;
        offenders.extend(found);
    }

    assert!(scanned > 0, "scanned no files; the path logic is wrong");
    assert!(
        offenders.is_empty(),
        "files named for another launcher's format belong in a separate tool:\n{}",
        offenders.join("\n")
    );
}

/// Files under `directory` and the ones whose name suggests a compatibility
/// layer.
fn scan(directory: &Path, root: &Path) -> (usize, Vec<String>) {
    let paths = entries(directory);
    let offenders = paths
        .iter()
        .filter_map(|path| forbidden_name(path, root))
        .collect();
    (paths.len(), offenders)
}

fn entries(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(listing) = fs::read_dir(root) else {
        return found;
    };
    for entry in listing.flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(entries(&path));
        } else {
            found.push(path);
        }
    }
    found
}

fn forbidden_name(path: &Path, root: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let lowered = relative.to_string_lossy().to_lowercase();
    FORBIDDEN_STEMS
        .iter()
        .find(|stem| lowered.contains(**stem))
        .map(|stem| format!("{} ({stem})", relative.display()))
}
