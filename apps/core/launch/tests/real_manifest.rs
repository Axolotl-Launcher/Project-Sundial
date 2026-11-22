// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! Checks the launch path against a real launcher installation when one exists.
//!
//! The fixtures in the unit tests are small and hand-written; they cannot catch
//! a mistake that only a real manifest exposes — an optional argument pair, a
//! library declared as a four-part coordinate, a natives entry that is a
//! classifier rather than a map. This test discovers a real installation and
//! asserts against it, and skips (without failing) when there is none, so the
//! suite still runs on a machine that never installed the game.

use std::path::{Path, PathBuf};

use superlogical_launch::{Layout, Session, Target, classpath, natives, plan, rules, version};

/// Directories a launcher's game root may live in, most specific first.
fn candidate_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    for key in ["APPDATA", "LOCALAPPDATA"] {
        let Ok(base) = std::env::var(key) else {
            continue;
        };
        let base = PathBuf::from(base);
        roots.push(base.join(".minecraft"));
        roots.push(base.join("red.ghs.axolotl").join("meta"));
    }
    roots
}

/// The first root that holds at least one version document.
fn installed_root() -> Option<PathBuf> {
    candidate_roots().into_iter().find(|root| {
        std::fs::read_dir(root.join("versions"))
            .map(|entries| entries.flatten().any(|entry| entry.path().is_dir()))
            .unwrap_or(false)
    })
}

fn version_ids(root: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(root.join("versions")) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
        .collect()
}

#[test]
fn a_real_manifest_yields_a_complete_command() {
    let Some(root) = installed_root() else {
        eprintln!("no installed Minecraft found; skipping");
        return;
    };
    let target = Target::host();
    let mut checked = 0;

    for id in version_ids(&root) {
        let Ok(document) = version::load(&root.join("versions"), &id) else {
            continue;
        };
        if !is_complete(&root, &document, &target) {
            eprintln!("{id}: incomplete install, skipping");
            continue;
        }

        let command = build_command(&root, &id, &document, &target);
        assert_no_placeholders(&id, &command);
        assert_main_class(&id, &document, &command);
        assert_natives(&id, &document, &target);

        eprintln!(
            "{id}: command with {} arguments, all resolved",
            command.args.len()
        );
        checked += 1;
    }

    eprintln!(
        "checked {checked} complete version(s) under {}",
        root.display()
    );
    assert!(
        checked > 0,
        "found an install but no complete version in it"
    );
}

/// Whether every file the version needs for `target` is on disk.
fn is_complete(root: &Path, document: &version::Version, target: &Target) -> bool {
    if !plan::client_jar(root, &document.id).is_file() {
        return false;
    }
    let libraries = root.join("libraries");
    document
        .libraries
        .iter()
        .filter(|library| rules::library_applies(library, target))
        .all(|library| {
            classpath::library_jar(&libraries, library, target)
                .map(|jar| jar.is_file())
                .unwrap_or(false)
        })
}

fn build_command(
    root: &Path,
    id: &str,
    document: &version::Version,
    target: &Target,
) -> superlogical_spi::LaunchCommand {
    let instance = std::env::temp_dir()
        .join("superlogical-real-manifest")
        .join(id);
    let layout = Layout {
        game_root: root.to_path_buf(),
        game_directory: instance.clone(),
        natives_directory: instance.join("natives"),
        launcher_name: "Superlogical".into(),
        launcher_version: "0.1.0".into(),
    };

    plan::build(
        Path::new("java"),
        document,
        &layout,
        &Session::offline("Check"),
        target,
    )
    .unwrap_or_else(|error| panic!("{id}: {error}"))
}

fn assert_no_placeholders(id: &str, command: &superlogical_spi::LaunchCommand) {
    let unresolved: Vec<&String> = command
        .args
        .iter()
        .filter(|argument| argument.contains("${"))
        .collect();
    assert!(
        unresolved.is_empty(),
        "{id}: unresolved placeholders {unresolved:?}"
    );
}

fn assert_main_class(
    id: &str,
    document: &version::Version,
    command: &superlogical_spi::LaunchCommand,
) {
    assert!(
        command
            .args
            .iter()
            .any(|argument| argument == &document.main_class),
        "{id}: main class missing from the command"
    );
}

fn assert_natives(id: &str, document: &version::Version, target: &Target) {
    let jars = natives::entries(&document.libraries, target);
    assert!(
        !jars.is_empty(),
        "{id}: no native jars found for {}",
        target.os
    );
}
