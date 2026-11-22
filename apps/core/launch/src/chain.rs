// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! Resolving a version that inherits from another.
//!
//! A loader's profile is a partial document: it names its vanilla version in
//! `inheritsFrom` and states only what it changes. The format defines how the
//! two combine, and the rules are not the same for every field:
//!
//! - a scalar the child states replaces the parent's, and one it omits is
//!   inherited;
//! - libraries and arguments are **appended**, child after parent, because the
//!   game is meant to see the loader's jars and flags after the vanilla ones;
//! - `jar` names the jar under `versions/` the game runs from, so a loader
//!   profile points at the vanilla client jar instead of one of its own.
//!
//! Resolving by field name, rather than by merging raw JSON, keeps a field the
//! game adds later visible instead of silently dropped.

use std::collections::HashSet;
use std::path::Path;

use crate::version::{self, Version};

/// How many links a chain may have before it is treated as a loop.
///
/// A real chain is two links, a nested loader at most three. The limit turns a
/// corrupt document into an error instead of a stack exhausted mid-launch.
const MAX_DEPTH: usize = 8;

/// Why a version could not be resolved against what it inherits from.
#[derive(Debug)]
pub enum ChainError {
    /// A document in the chain could not be read or parsed.
    Document { source: version::VersionError },
    /// A profile names the same parent twice, directly or through another.
    Cycle { id: String },
    /// The chain is longer than [`MAX_DEPTH`].
    TooDeep { id: String },
}

impl std::fmt::Display for ChainError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Document { source } => write!(formatter, "{source}"),
            Self::Cycle { id } => write!(formatter, "{id} inherits from itself"),
            Self::TooDeep { id } => write!(
                formatter,
                "{id} inherits through more than {MAX_DEPTH} documents"
            ),
        }
    }
}

impl std::error::Error for ChainError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Document { source } => Some(source),
            Self::Cycle { .. } | Self::TooDeep { .. } => None,
        }
    }
}

/// The document `child` becomes once `parent` is beneath it.
///
/// The child's `id` is kept: it is the name the caller asked for, and the name
/// the game is told. `inherits_from` is cleared, so a resolved document no
/// longer claims to be a fragment of something else.
pub fn merge(parent: Version, child: Version) -> Version {
    let mut libraries = parent.libraries;
    libraries.extend(child.libraries);

    let arguments = match (parent.arguments, child.arguments) {
        (None, None) => None,
        (Some(only), None) | (None, Some(only)) => Some(only),
        (Some(mut parent), Some(child)) => {
            parent.jvm.extend(child.jvm);
            parent.game.extend(child.game);
            parent.default_user_jvm.extend(child.default_user_jvm);
            Some(parent)
        }
    };

    let mut data = parent.data;
    data.extend(child.data);
    let mut processors = parent.processors;
    processors.extend(child.processors);

    Version {
        id: child.id,
        kind: if child.kind.is_empty() {
            parent.kind
        } else {
            child.kind
        },
        main_class: if child.main_class.is_empty() {
            parent.main_class
        } else {
            child.main_class
        },
        libraries,
        arguments,
        minecraft_arguments: child.minecraft_arguments.or(parent.minecraft_arguments),
        asset_index: child.asset_index.or(parent.asset_index),
        assets: child.assets.or(parent.assets),
        java_version: child.java_version.or(parent.java_version),
        inherits_from: None,
        jar: child.jar.or(parent.jar),
        downloads: child.downloads.or(parent.downloads),
        logging: child.logging.or(parent.logging),
        processors,
        data,
    }
}

/// The document `version` describes once everything it inherits is beneath it.
pub fn resolve(root: &Path, version: Version) -> Result<Version, ChainError> {
    let mut visited = HashSet::new();
    resolve_below(root, version, &mut visited)
}

fn resolve_below(
    root: &Path,
    mut version: Version,
    visited: &mut HashSet<String>,
) -> Result<Version, ChainError> {
    if !visited.insert(version.id.clone()) {
        return Err(ChainError::Cycle { id: version.id });
    }
    if visited.len() > MAX_DEPTH {
        return Err(ChainError::TooDeep { id: version.id });
    }

    if let Some(parent_id) = version.inherits_from.clone() {
        let parent =
            version::load(root, &parent_id).map_err(|source| ChainError::Document { source })?;
        let parent = resolve_below(root, parent, visited)?;
        version = merge(parent, version);
    }

    visited.remove(&version.id);
    Ok(version)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::{Argument, Arguments};
    use std::collections::BTreeMap;

    fn minimal(id: &str) -> Version {
        Version {
            id: id.into(),
            kind: "release".into(),
            main_class: "net.minecraft.client.main.Main".into(),
            libraries: Vec::new(),
            arguments: None,
            minecraft_arguments: None,
            asset_index: None,
            assets: None,
            java_version: None,
            inherits_from: None,
            jar: None,
            downloads: None,
            logging: None,
            processors: Vec::new(),
            data: BTreeMap::default(),
        }
    }

    fn plain(argument: &str) -> Argument {
        Argument::Plain(argument.into())
    }

    #[test]
    fn a_field_the_child_omits_is_the_parents() {
        let mut parent = minimal("1.20.1");
        parent.assets = Some("5".into());

        let child = minimal("1.20.1-forge");

        let merged = merge(parent, child);
        assert_eq!(merged.assets.as_deref(), Some("5"));
        assert_eq!(merged.id, "1.20.1-forge");
    }

    #[test]
    fn a_field_the_child_states_replaces_the_parents() {
        let mut parent = minimal("1.20.1");
        parent.main_class = "net.minecraft.client.main.Main".into();

        let mut child = minimal("1.20.1-forge");
        child.main_class = "cpw.mods.bootstraplauncher.BootstrapLauncher".into();

        let merged = merge(parent, child);
        assert_eq!(
            merged.main_class,
            "cpw.mods.bootstraplauncher.BootstrapLauncher"
        );
    }

    #[test]
    fn libraries_keep_the_parent_first_and_the_child_after() {
        let mut parent = minimal("1.20.1");
        parent.libraries = vec![crate::version::Library {
            name: "org.example:vanilla:1".into(),
            downloads: None,
            natives: None,
            rules: None,
            url: None,
            include_in_classpath: true,
        }];
        let mut child = minimal("1.20.1-forge");
        child.libraries = vec![crate::version::Library {
            name: "org.example:loader:1".into(),
            downloads: None,
            natives: None,
            rules: None,
            url: None,
            include_in_classpath: true,
        }];

        let merged = merge(parent, child);
        let names: Vec<&str> = merged
            .libraries
            .iter()
            .map(|library| library.name.as_str())
            .collect();
        assert_eq!(names, ["org.example:vanilla:1", "org.example:loader:1"]);
    }

    #[test]
    fn arguments_are_appended_rather_than_replaced() {
        let mut parent = minimal("1.20.1");
        parent.arguments = Some(Arguments {
            jvm: vec![plain("-cp")],
            game: vec![plain("--username")],
            default_user_jvm: Vec::new(),
        });
        let mut child = minimal("1.20.1-forge");
        child.arguments = Some(Arguments {
            jvm: vec![plain("-Dloader=1")],
            game: vec![plain("--fml.neoForgeVersion")],
            default_user_jvm: Vec::new(),
        });

        let merged = merge(parent, child);
        let arguments = merged.arguments.expect("arguments");
        assert_eq!(arguments.jvm.len(), 2);
        assert_eq!(arguments.game.len(), 2);
    }

    #[test]
    fn a_resolved_document_no_longer_names_a_parent() {
        let mut parent = minimal("1.20.1");
        parent.jar = None;
        let mut child = minimal("1.20.1-forge");
        child.inherits_from = Some("1.20.1".into());
        child.jar = None;

        let merged = merge(parent, child);
        assert!(merged.inherits_from.is_none());
    }

    #[test]
    fn the_jar_a_child_names_wins_over_the_parents() {
        let mut parent = minimal("1.20.1");
        parent.jar = Some("1.20.1".into());
        let mut child = minimal("1.20.1-forge");
        child.jar = Some("1.20.1-forge".into());

        let merged = merge(parent, child);
        assert_eq!(merged.jar.as_deref(), Some("1.20.1-forge"));
    }
}

#[cfg(test)]
mod document_tests {
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
}
