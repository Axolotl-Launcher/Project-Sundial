// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! The classpath a launch runs with.
//!
//! The separator follows the platform the game's Java runs on, not the host:
//! a Windows Java reads `;` and a Linux one `:`. The value is passed in rather
//! than read from a constant so a wrong pairing is visible at the call site.

use std::path::{Path, PathBuf};

use crate::rules::{self, Target};
use crate::version::Library;

/// The separator Windows Java expects.
pub const WINDOWS_SEPARATOR: char = ';';

/// The separator Java expects everywhere else.
pub const UNIX_SEPARATOR: char = ':';

/// The separator for a game running on `os`, in the game's vocabulary.
pub fn separator(os: &str) -> char {
    if os == "windows" {
        WINDOWS_SEPARATOR
    } else {
        UNIX_SEPARATOR
    }
}

/// The jar of a library, when the document names one for `target`.
pub fn artifact_path(library: &Library, target: &Target) -> Option<PathBuf> {
    if !rules::library_applies(library, target) {
        return None;
    }
    library
        .downloads
        .as_ref()
        .and_then(|downloads| downloads.artifact.as_ref())
        .map(|artifact| PathBuf::from(&artifact.path))
}

/// The jar a library contributes on `target`, resolved against `libraries_root`.
pub fn library_jar(library_root: &Path, library: &Library, target: &Target) -> Option<PathBuf> {
    artifact_path(library, target).map(|relative| library_root.join(relative))
}

/// The full classpath for a version, ending with its client jar.
///
/// Order matters to the game, so libraries keep document order and the client
/// jar comes last.
pub fn build(
    libraries_root: &Path,
    libraries: &[Library],
    client_jar: &Path,
    target: &Target,
) -> String {
    let jars: Vec<String> = libraries
        .iter()
        .filter_map(|library| library_jar(libraries_root, library, target))
        .map(|jar| jar.display().to_string())
        .chain(std::iter::once(client_jar.display().to_string()))
        .collect();

    jars.join(&separator(&target.os).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::version::{Artifact, LibraryDownloads, OsRule, Rule};

    fn target(os: &str) -> Target {
        Target {
            os: os.into(),
            arch: "x86_64".into(),
        }
    }

    fn library(name: &str, path: &str, os: Option<&str>) -> Library {
        Library {
            name: name.into(),
            downloads: Some(LibraryDownloads {
                artifact: Some(Artifact {
                    path: path.into(),
                    sha1: None,
                    size: None,
                    url: None,
                }),
                classifiers: None,
            }),
            natives: None,
            rules: os.map(|name| {
                vec![Rule {
                    action: "allow".into(),
                    os: Some(OsRule {
                        name: Some(name.into()),
                        arch: None,
                    }),
                }]
            }),
        }
    }

    #[test]
    fn the_separator_follows_the_game_platform() {
        assert_eq!(separator("windows"), ';');
        assert_eq!(separator("linux"), ':');
        assert_eq!(separator("osx"), ':');
    }

    #[test]
    fn only_libraries_allowed_on_the_target_are_included() {
        let libraries = [
            library("a", "a/a.jar", None),
            library("b", "b/b.jar", Some("windows")),
            library("c", "c/c.jar", Some("osx")),
        ];

        let classpath = build(
            Path::new("libs"),
            &libraries,
            Path::new("client.jar"),
            &target("windows"),
        );
        let entries: Vec<&str> = classpath.split(WINDOWS_SEPARATOR).collect();

        // Paths are joined by the host, so only the entry names are asserted.
        assert_eq!(entries.len(), 3, "{classpath}");
        assert!(entries[0].ends_with("a.jar"), "{classpath}");
        assert!(entries[1].ends_with("b.jar"), "{classpath}");
        assert_eq!(entries[2], "client.jar");
    }

    #[test]
    fn the_client_jar_comes_last() {
        let libraries = [library("a", "a.jar", None)];
        let classpath = build(
            Path::new("l"),
            &libraries,
            Path::new("c.jar"),
            &target("linux"),
        );
        assert!(classpath.ends_with(":c.jar"), "{classpath}");
    }

    #[test]
    fn a_library_without_an_artifact_is_skipped() {
        let mut library = library("a", "a.jar", None);
        library.downloads = Some(LibraryDownloads {
            artifact: None,
            classifiers: None,
        });

        assert_eq!(
            build(
                Path::new("l"),
                std::slice::from_ref(&library),
                Path::new("c.jar"),
                &target("linux")
            ),
            "c.jar"
        );
    }
}
