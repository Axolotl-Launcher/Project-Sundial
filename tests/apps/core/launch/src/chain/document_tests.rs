// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;
use std::fs;
use std::path::PathBuf;

static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// A directory that cleans itself up, matching the crate's other tests.
struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let unique = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "superlogical-chain-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("temp root");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn write(root: &Path, id: &str, document: &str) {
    let directory = root.join(id);
    fs::create_dir_all(&directory).expect("version directory");
    fs::write(directory.join(format!("{id}.json")), document).expect("document");
}

#[test]
fn a_profile_resolves_against_the_version_it_inherits() {
    let root = TempRoot::new();
    write(
        root.path(),
        "1.20.1",
        r#"{
            "id": "1.20.1",
            "type": "release",
            "mainClass": "net.minecraft.client.main.Main",
            "assets": "5",
            "libraries": [{"name": "org.example:vanilla:1", "downloads": {"artifact": {"path": "v.jar"}}}]
        }"#,
    );
    write(
        root.path(),
        "1.20.1-forge",
        r#"{
            "id": "1.20.1-forge",
            "inheritsFrom": "1.20.1",
            "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher",
            "libraries": [{"name": "org.example:loader:1", "downloads": {"artifact": {"path": "l.jar"}}}]
        }"#,
    );

    let resolved = version::load_resolved(root.path(), "1.20.1-forge").expect("resolved");

    assert_eq!(resolved.assets.as_deref(), Some("5"), "inherited");
    assert_eq!(
        resolved.main_class, "cpw.mods.bootstraplauncher.BootstrapLauncher",
        "overridden"
    );
    assert_eq!(resolved.libraries.len(), 2, "appended");
    assert!(resolved.inherits_from.is_none());
}

#[test]
fn a_cycle_is_an_error_rather_than_a_hang() {
    let root = TempRoot::new();
    write(
        root.path(),
        "a",
        r#"{"id": "a", "inheritsFrom": "b", "mainClass": "A"}"#,
    );
    write(
        root.path(),
        "b",
        r#"{"id": "b", "inheritsFrom": "a", "mainClass": "B"}"#,
    );

    let error = version::load_resolved(root.path(), "a").expect_err("cycle");
    assert!(matches!(error, ChainError::Cycle { .. }), "{error}");
}

#[test]
fn a_parent_that_is_absent_names_the_document_that_could_not_be_read() {
    let root = TempRoot::new();
    write(
        root.path(),
        "orphan",
        r#"{"id": "orphan", "inheritsFrom": "missing", "mainClass": "O"}"#,
    );

    let error = version::load_resolved(root.path(), "orphan").expect_err("missing parent");
    let text = error.to_string();
    assert!(text.contains("missing"), "{text}");
}

#[test]
fn a_document_with_nothing_to_inherit_resolves_to_itself() {
    let root = TempRoot::new();
    write(
        root.path(),
        "1.21",
        r#"{"id": "1.21", "type": "release", "mainClass": "Main", "assets": "34"}"#,
    );

    let resolved = version::load_resolved(root.path(), "1.21").expect("resolved");
    assert_eq!(resolved.id, "1.21");
    assert_eq!(resolved.main_class, "Main");
}
