// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Guards the part of the comment rule in AGENTS.md that a test can decide
//! without guessing: a doc comment must not open with a generic verb.
//!
//! `/// Returns the current value` above `fn value()` says what the signature
//! already says. Detecting every such comment needs judgement, so this test
//! only rejects the unambiguous shape — a doc comment whose first word is a
//! verb that describes the mechanics of the item rather than its meaning.
//! A comment starting any other way is left to review.

use std::fs;
use std::path::{Path, PathBuf};

const WORKSPACE_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
const SOURCE_ROOTS: [&str; 3] = ["apps", "platforms", "tests"];

/// Opening verbs that restate a signature. All are lowercased for comparison.
const MECHANICAL_OPENERS: [&str; 22] = [
    "adds",
    "check",
    "checks",
    "create",
    "creates",
    "construct",
    "constructs",
    "get",
    "gets",
    "increment",
    "increments",
    "decrement",
    "decrements",
    "return",
    "returns",
    "set",
    "sets",
    "reset",
    "resets",
    "update",
    "updates",
    "wrap",
];

#[test]
fn doc_comments_do_not_open_with_a_mechanical_verb() {
    let root = Path::new(WORKSPACE_ROOT);
    let mut offences = Vec::new();
    let mut scanned = 0;

    for name in SOURCE_ROOTS {
        let dir = root.join(name);
        assert!(dir.is_dir(), "source root missing: {}", dir.display());
        for file in rust_files(&dir) {
            scanned += 1;
            check_file(&file, &mut offences);
        }
    }

    assert!(scanned > 0, "scanned no files; the path logic is wrong");
    assert!(
        offences.is_empty(),
        "doc comments that restate their signature:\n{}",
        offences.join("\n")
    );
}

fn rust_files(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        // node_modules holds symlinked copies of workspace crates; scanning them
        // would report every offence twice.
        if name == "node_modules" || name == "target" {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            found.extend(rust_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            found.push(path);
        }
    }
    found
}

fn check_file(path: &Path, offences: &mut Vec<String>) {
    let Ok(source) = fs::read_to_string(path) else {
        return;
    };

    for (index, line) in source.lines().enumerate() {
        let Some(text) = line.trim().strip_prefix("///") else {
            continue;
        };
        if opens_with_mechanical_verb(text) {
            offences.push(format!("{}:{}: ///{}", path.display(), index + 1, text));
        }
    }
}

fn opens_with_mechanical_verb(comment: &str) -> bool {
    let first = comment
        .split(|c: char| !c.is_alphanumeric())
        .find(|word| !word.is_empty())
        .unwrap_or_default()
        .to_lowercase();

    MECHANICAL_OPENERS.contains(&first.as_str())
}
