// SPDX-FileCopyrightText: 2026 Axolotl Team
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
///
/// The path is the one the document states, or the one the coordinate implies
/// when a loader names a Maven root instead.
///
/// A classifier jar carrying natives **is** a classpath entry. The 1.19 split
/// moved natives into ordinary four-part coordinates, and both things still
/// happen to them: the jar goes on the classpath and the files inside it are
/// extracted. LWJGL's loader looks for its shared library through the jar on
/// the classpath first, so leaving it out starts the game and fails inside it
/// with `Failed to locate library: lwjgl.dll`.
///
/// Only a declaration with no jar at all is excluded: the old shape lists
/// natives solely through `classifiers` and has no separate Java jar to offer.
pub fn artifact_path(library: &Library, target: &Target) -> Option<PathBuf> {
    if !rules::library_applies(library, target) || !library.include_in_classpath {
        return None;
    }
    if library.is_native_only() {
        return None;
    }
    library
        .download()
        .map(|artifact| PathBuf::from(artifact.path))
        .or_else(|| library.maven_path())
}

/// The jar a library contributes on `target`, resolved against `libraries_root`.
pub fn library_jar(library_root: &Path, library: &Library, target: &Target) -> Option<PathBuf> {
    artifact_path(library, target).map(|relative| library_root.join(relative))
}

/// The full classpath for a version, ending with its client jar.
///
/// Order matters to the game, so libraries keep document order and the client
/// jar comes last.
///
/// Two libraries can name the same artifact when a manifest carries
/// platform-specific revisions of one Maven module; only one jar may reach the
/// classpath or the older revision shadows the newer one's native ABI. The
/// later entry wins, so the list is walked from the end and the result is put
/// back in document order.
pub fn build(
    libraries_root: &Path,
    libraries: &[Library],
    client_jar: &Path,
    target: &Target,
) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut jars: Vec<String> = libraries
        .iter()
        .rev()
        .filter_map(|library| {
            let key = coordinate_key(library);
            if !seen.insert(key) {
                return None;
            }
            library_jar(libraries_root, library, target)
        })
        .map(|jar| jar.display().to_string())
        .collect();
    jars.reverse();
    jars.push(client_jar.display().to_string());

    jars.join(&separator(&target.os).to_string())
}

/// What makes two library entries the same artifact for classpath purposes.
///
/// The classifier is part of the key: a native classifier shares a group and
/// artifact with the Java jar but is a different file, and dropping it would
/// remove the Java classes.
fn coordinate_key(library: &Library) -> String {
    let mut parts = library.name.split(':');
    let group = parts.next().unwrap_or_default();
    let artifact = parts.next().unwrap_or_default();
    let classifier = parts.nth(1).unwrap_or_default();
    format!("{group}:{artifact}:{classifier}")
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
            url: None,
            include_in_classpath: true,
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

    #[test]
    fn a_fabric_style_library_is_resolved_from_its_coordinate() {
        let mut fabric = library("org.ow2.asm:asm:9.10.1", "", None);
        fabric.downloads = None;
        fabric.url = Some("https://maven.fabricmc.net/".into());

        let classpath = build(
            Path::new("libraries"),
            std::slice::from_ref(&fabric),
            Path::new("c.jar"),
            &target("linux"),
        );

        assert!(classpath.contains("asm-9.10.1.jar"), "{classpath}");
        assert!(
            classpath.contains("org/ow2/asm/asm/9.10.1"),
            "coordinate path: {classpath}"
        );
    }

    #[test]
    fn a_native_classifier_jar_is_also_a_classpath_entry() {
        let java = library(
            "org.lwjgl:lwjgl:3.3.1",
            "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1.jar",
            None,
        );
        let native = library(
            "org.lwjgl:lwjgl:3.3.1:natives-windows",
            "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows.jar",
            None,
        );

        let classpath = build(
            Path::new("l"),
            &[java, native],
            Path::new("c.jar"),
            &target("windows"),
        );
        let entries: Vec<&str> = classpath.split(WINDOWS_SEPARATOR).collect();

        // Both jars are here. The loader finds its shared library through the
        // classifier jar on the classpath, so keeping only the Java jar makes
        // the game start and then fail inside LWJGL.
        assert_eq!(entries.len(), 3, "{classpath}");
        assert!(entries[0].ends_with("lwjgl-3.3.1.jar"), "{classpath}");
        assert!(
            entries[1].ends_with("lwjgl-3.3.1-natives-windows.jar"),
            "{classpath}"
        );
        assert_eq!(entries[2], "c.jar");
    }

    #[test]
    fn two_revisions_of_a_native_are_told_apart_by_their_classifier() {
        let windows = library("org.lwjgl:lwjgl:3.3.1:natives-windows", "win.jar", None);
        let linux = library("org.lwjgl:lwjgl:3.3.1:natives-linux", "linux.jar", None);

        let classpath = build(
            Path::new("l"),
            &[windows, linux],
            Path::new("c.jar"),
            &target("linux"),
        );

        // The Java jar must not be the key for a classifier: two natives of one
        // artifact are different files and both belong on the classpath.
        assert!(classpath.contains("win.jar"), "{classpath}");
        assert!(classpath.contains("linux.jar"), "{classpath}");
    }

    #[test]
    fn a_repeated_artifact_keeps_only_its_last_revision() {
        let old = library("org.lwjgl:lwjgl:3.2.1", "old/lwjgl-3.2.1.jar", None);
        let new = library("org.lwjgl:lwjgl:3.3.1", "new/lwjgl-3.3.1.jar", None);

        let classpath = build(
            Path::new("l"),
            &[old, new],
            Path::new("c.jar"),
            &target("linux"),
        );

        assert!(!classpath.contains("3.2.1"), "{classpath}");
        assert!(classpath.contains("3.3.1"), "{classpath}");
    }

    #[test]
    fn a_tool_the_loader_runs_itself_stays_off_the_classpath() {
        let mut tool = library("net.neoforged:installertools:2.1.2", "tool.jar", None);
        tool.include_in_classpath = false;

        assert_eq!(
            build(
                Path::new("l"),
                std::slice::from_ref(&tool),
                Path::new("c.jar"),
                &target("linux")
            ),
            "c.jar"
        );
    }
}
