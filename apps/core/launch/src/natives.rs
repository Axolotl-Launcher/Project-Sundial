// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! The native jars a version needs and the files they unpack on disk.
//!
//! Two document shapes exist. A version from 1.20.1 onward lists natives as
//! ordinary libraries whose name carries a `:natives-<classifier>` suffix,
//! gated by rules. Older versions instead map an operating system to a
//! classifier key through `natives`, with the arch filled into `${arch}`.
//! Selection is pure; extraction reads through [`NativeReader`], so the
//! unpacking logic does not depend on a particular zip decoder.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::rules::{self, Target};
use crate::version::Library;

/// The marker that distinguishes a native library from a classpath library.
const NATIVES_MARKER: &str = ":natives-";

/// The placeholder older documents use inside a classifier key.
const ARCH_PLACEHOLDER: &str = "${arch}";

/// The game's classifier spelling for a 64-bit x86 target.
const GAME_ARCH_64: &str = "64";

/// The game's classifier spelling for a 32-bit x86 target.
const GAME_ARCH_32: &str = "32";

/// The target vocabulary's 64-bit x86 name.
const TARGET_ARCH_X86_64: &str = "x86_64";

/// The target vocabulary's 32-bit x86 name.
const TARGET_ARCH_X86: &str = "x86";

/// Extensions a native library can carry.
const NATIVE_EXTENSIONS: [&str; 4] = ["dll", "so", "dylib", "jnilib"];

/// How many ignored entry names are worth reporting to a caller.
const MAX_SKIPPED_NAMES: usize = 64;

/// Why a natives directory could not be produced.
#[derive(Debug)]
pub enum NativeError {
    /// A file could not be read or written.
    Io {
        /// The file the operation was aimed at.
        path: PathBuf,
        /// The underlying filesystem failure.
        source: io::Error,
    },
    /// The jar is a zip archive but its contents could not be decoded.
    Zip {
        /// The jar being read.
        path: PathBuf,
        /// What went wrong while decoding it.
        reason: String,
    },
    /// The file is not a zip archive, so it cannot hold natives.
    NotAJar {
        /// The file that was expected to be a jar.
        path: PathBuf,
    },
}

impl std::fmt::Display for NativeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(formatter, "cannot access {}: {source}", path.display())
            }
            Self::Zip { path, reason } => {
                write!(formatter, "cannot read jar {}: {reason}", path.display())
            }
            Self::NotAJar { path } => {
                write!(formatter, "{} is not a jar", path.display())
            }
        }
    }
}

impl std::error::Error for NativeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Zip { .. } | Self::NotAJar { .. } => None,
        }
    }
}

/// A jar that carries native libraries for one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeEntry {
    /// The jar's path relative to the libraries root, as the document names it.
    pub path: String,
}

/// What an extraction produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracted {
    /// The natives written under the destination, each path listed once.
    pub files: Vec<PathBuf>,
    /// Non-native entry names that were ignored, sorted and capped.
    pub skipped: Vec<String>,
}

/// A source of jar entries, so extraction can be tested without a zip decoder.
///
/// An implementation that opens a file and finds it is not a zip archive
/// reports [`NativeError::NotAJar`] with that file's path.
pub trait NativeReader {
    /// The entries of the jar at `path`, as `(name, bytes)` pairs.
    fn read_jar(&self, path: &Path) -> Result<Vec<(String, Vec<u8>)>, NativeError>;
}

/// The native jars that apply on `target`, in document order.
pub fn entries(libraries: &[Library], target: &Target) -> Vec<NativeEntry> {
    libraries
        .iter()
        .filter(|library| rules::library_applies(library, target))
        .filter_map(|library| native_jar(library, target))
        .map(|path| NativeEntry { path })
        .collect()
}

/// Extracts the native files of `entries` into `destination`.
///
/// The destination is created when missing, and an existing native is replaced
/// rather than treated as an error. Each file is written atomically, so a
/// failed extraction never leaves a truncated native behind.
pub fn extract<R: NativeReader>(
    reader: &R,
    jars_root: &Path,
    entries: &[NativeEntry],
    destination: &Path,
) -> Result<Extracted, NativeError> {
    fs::create_dir_all(destination).map_err(|source| NativeError::Io {
        path: destination.to_path_buf(),
        source,
    })?;

    let mut files = Vec::new();
    let mut skipped = BTreeSet::new();
    for entry in entries {
        let jar = jars_root.join(&entry.path);
        for (name, bytes) in reader.read_jar(&jar)? {
            match native_relative_path(&name) {
                Some(relative) => write_native(destination, &relative, &bytes, &mut files)?,
                None => record_skipped(&mut skipped, name),
            }
        }
    }

    Ok(Extracted {
        files,
        skipped: skipped.into_iter().collect(),
    })
}

/// The jar of one library that holds natives, when it names one for `target`.
fn native_jar(library: &Library, target: &Target) -> Option<String> {
    if library.name.contains(NATIVES_MARKER) {
        return library
            .downloads
            .as_ref()
            .and_then(|downloads| downloads.artifact.as_ref())
            .map(|artifact| artifact.path.clone());
    }

    let classifier = library.natives.as_ref()?.get(target.os.as_str())?;
    let classifier = classifier.replace(ARCH_PLACEHOLDER, classifier_arch(&target.arch));
    library
        .downloads
        .as_ref()
        .and_then(|downloads| downloads.classifiers.as_ref())
        .and_then(|classifiers| classifiers.get(&classifier))
        .map(|artifact| artifact.path.clone())
}

/// The classifier spelling of `arch`, which the game writes as `64` or `32`.
fn classifier_arch(arch: &str) -> &str {
    match arch {
        TARGET_ARCH_X86_64 => GAME_ARCH_64,
        TARGET_ARCH_X86 => GAME_ARCH_32,
        other => other,
    }
}

/// The safe relative path of an entry when it is a native file.
fn native_relative_path(entry: &str) -> Option<PathBuf> {
    if entry.ends_with('/') || entry.ends_with('\\') {
        return None;
    }
    let extension = Path::new(entry).extension()?.to_str()?.to_ascii_lowercase();
    if !NATIVE_EXTENSIONS.contains(&extension.as_str()) {
        return None;
    }
    safe_components(Path::new(entry))
}

/// The path with only normal components kept, rejecting anything that escapes.
fn safe_components(path: &Path) -> Option<PathBuf> {
    let mut safe = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => safe.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    if safe.as_os_str().is_empty() {
        None
    } else {
        Some(safe)
    }
}

/// Writes one native and records its path.
fn write_native(
    destination: &Path,
    relative: &Path,
    bytes: &[u8],
    files: &mut Vec<PathBuf>,
) -> Result<(), NativeError> {
    let target = destination.join(relative);
    superlogical_storage::write_atomic(&target, bytes).map_err(|source| NativeError::Io {
        path: target.clone(),
        source,
    })?;
    if !files.contains(&target) {
        files.push(target);
    }
    Ok(())
}

/// Keeps the ignored name for logging while the list still has room.
fn record_skipped(skipped: &mut BTreeSet<String>, name: String) {
    if skipped.len() < MAX_SKIPPED_NAMES {
        skipped.insert(name);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::version::{Artifact, LibraryDownloads, OsRule, Rule};

    fn target(os: &str, arch: &str) -> Target {
        Target {
            os: os.into(),
            arch: arch.into(),
        }
    }

    fn artifact(path: &str) -> Artifact {
        Artifact {
            path: path.into(),
            sha1: None,
            size: None,
            url: None,
        }
    }

    fn allow(os: &str) -> Vec<Rule> {
        vec![Rule {
            action: "allow".into(),
            os: Some(OsRule {
                name: Some(os.into()),
                arch: None,
            }),
        }]
    }

    fn native_library(name: &str, path: &str, os: &str) -> Library {
        Library {
            name: name.into(),
            downloads: Some(LibraryDownloads {
                artifact: Some(artifact(path)),
                classifiers: None,
            }),
            natives: None,
            rules: Some(allow(os)),
        }
    }

    fn ordinary_library(name: &str, path: &str, os: &str) -> Library {
        Library {
            name: name.into(),
            downloads: Some(LibraryDownloads {
                artifact: Some(artifact(path)),
                classifiers: None,
            }),
            natives: None,
            rules: Some(allow(os)),
        }
    }

    #[derive(Default)]
    struct MemoryReader {
        jars: BTreeMap<PathBuf, Vec<(String, Vec<u8>)>>,
    }

    impl MemoryReader {
        fn with_jar(mut self, path: PathBuf, entries: Vec<(String, Vec<u8>)>) -> Self {
            self.jars.insert(path, entries);
            self
        }
    }

    impl NativeReader for MemoryReader {
        fn read_jar(&self, path: &Path) -> Result<Vec<(String, Vec<u8>)>, NativeError> {
            self.jars
                .get(path)
                .cloned()
                .ok_or_else(|| NativeError::NotAJar {
                    path: path.to_path_buf(),
                })
        }
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "superlogical-natives-{}-{name}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("create scratch directory");
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn new_shape_natives_follow_the_target_rules() {
        let libraries = [
            native_library(
                "org.lwjgl:lwjgl:3.3.1:natives-linux",
                "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-linux.jar",
                "linux",
            ),
            native_library(
                "org.lwjgl:lwjgl:3.3.1:natives-windows",
                "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows.jar",
                "windows",
            ),
            ordinary_library(
                "org.lwjgl:lwjgl:3.3.1",
                "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1.jar",
                "linux",
            ),
        ];

        assert_eq!(
            entries(&libraries, &target("linux", "x86_64")),
            vec![NativeEntry {
                path: "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-linux.jar".into(),
            }]
        );
        assert_eq!(
            entries(&libraries, &target("windows", "x86_64")),
            vec![NativeEntry {
                path: "org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows.jar".into(),
            }]
        );
    }

    #[test]
    fn old_shape_natives_substitute_the_target_arch() {
        let library = Library {
            name: "org.lwjgl.lwjgl:lwjgl-platform:2.9.4".into(),
            downloads: Some(LibraryDownloads {
                artifact: None,
                classifiers: Some(BTreeMap::from([
                    (
                        "natives-windows-32".into(),
                        artifact("lwjgl-platform-2.9.4-natives-windows-32.jar"),
                    ),
                    (
                        "natives-windows-64".into(),
                        artifact("lwjgl-platform-2.9.4-natives-windows-64.jar"),
                    ),
                ])),
            }),
            natives: Some(BTreeMap::from([(
                "windows".into(),
                "natives-windows-${arch}".into(),
            )])),
            rules: None,
        };

        assert_eq!(
            entries(std::slice::from_ref(&library), &target("windows", "x86_64")),
            vec![NativeEntry {
                path: "lwjgl-platform-2.9.4-natives-windows-64.jar".into(),
            }]
        );
        assert_eq!(
            entries(std::slice::from_ref(&library), &target("windows", "x86")),
            vec![NativeEntry {
                path: "lwjgl-platform-2.9.4-natives-windows-32.jar".into(),
            }]
        );
    }

    #[test]
    fn only_native_files_are_extracted_and_directories_are_skipped() {
        let root = TempDir::new("extensions");
        let jars_root = root.0.join("libraries");
        let destination = root.0.join("natives");
        let jar = jars_root.join("natives.jar");
        let reader = MemoryReader::default().with_jar(
            jar,
            vec![
                ("lwjgl.dll".into(), b"dll".to_vec()),
                ("liblwjgl.so".into(), b"so".to_vec()),
                ("liblwjgl.dylib".into(), b"dylib".to_vec()),
                ("liblwjgl.jnilib".into(), b"jnilib".to_vec()),
                ("META-INF/".into(), Vec::new()),
                ("META-INF/MANIFEST.MF".into(), b"manifest".to_vec()),
                ("notes.txt".into(), b"text".to_vec()),
            ],
        );

        let extracted = extract(&reader, &jars_root, &[entry("natives.jar")], &destination)
            .expect("extraction succeeds");

        assert_eq!(
            extracted.files,
            vec![
                destination.join("lwjgl.dll"),
                destination.join("liblwjgl.so"),
                destination.join("liblwjgl.dylib"),
                destination.join("liblwjgl.jnilib"),
            ]
        );
        assert_eq!(
            fs::read(destination.join("lwjgl.dll")).expect("read extracted dll"),
            b"dll"
        );
        assert_eq!(
            extracted.skipped,
            vec![
                "META-INF/".to_string(),
                "META-INF/MANIFEST.MF".to_string(),
                "notes.txt".to_string(),
            ]
        );
        assert!(!destination.join("META-INF").exists());
    }

    #[test]
    fn entries_that_escape_the_destination_are_skipped() {
        let root = TempDir::new("zip-slip");
        let jars_root = root.0.join("libraries");
        let destination = root.0.join("natives");
        let jar = jars_root.join("natives.jar");
        let outside = root.0.join("escaped.dll");
        let reader = MemoryReader::default().with_jar(
            jar,
            vec![
                ("../escaped.dll".into(), b"escape".to_vec()),
                ("a/../../escaped.dll".into(), b"escape".to_vec()),
                ("/absolute.dll".into(), b"escape".to_vec()),
                ("nested/ok.dll".into(), b"kept".to_vec()),
            ],
        );

        let extracted = extract(&reader, &jars_root, &[entry("natives.jar")], &destination)
            .expect("extraction succeeds");

        assert!(!outside.exists(), "a native escaped the destination");
        assert_eq!(
            extracted.files,
            vec![destination.join("nested").join("ok.dll")]
        );
        assert_eq!(extracted.skipped.len(), 3);
    }

    #[test]
    fn a_nested_directory_is_preserved_inside_the_destination() {
        let root = TempDir::new("nested");
        let jars_root = root.0.join("libraries");
        let destination = root.0.join("natives");
        let jar = jars_root.join("natives.jar");
        let reader = MemoryReader::default()
            .with_jar(jar, vec![("linux/x64/liblwjgl.so".into(), b"so".to_vec())]);

        extract(&reader, &jars_root, &[entry("natives.jar")], &destination)
            .expect("extraction succeeds");

        assert!(
            destination
                .join("linux")
                .join("x64")
                .join("liblwjgl.so")
                .is_file()
        );
    }

    #[test]
    fn extraction_creates_a_missing_destination() {
        let root = TempDir::new("missing-destination");
        let jars_root = root.0.join("libraries");
        let destination = root.0.join("missing").join("natives");
        assert!(!destination.exists());
        let reader = MemoryReader::default().with_jar(
            jars_root.join("natives.jar"),
            vec![("lwjgl.dll".into(), b"dll".to_vec())],
        );

        extract(&reader, &jars_root, &[entry("natives.jar")], &destination)
            .expect("extraction succeeds");

        assert!(destination.is_dir());
        assert_eq!(
            fs::read(destination.join("lwjgl.dll")).expect("read extracted dll"),
            b"dll"
        );
    }

    #[test]
    fn extraction_replaces_an_existing_native() {
        let root = TempDir::new("replace");
        let jars_root = root.0.join("libraries");
        let destination = root.0.join("natives");
        let jar = jars_root.join("natives.jar");
        let first = MemoryReader::default()
            .with_jar(jar.clone(), vec![("lwjgl.dll".into(), b"first".to_vec())]);
        let replacement =
            MemoryReader::default().with_jar(jar, vec![("lwjgl.dll".into(), b"second".to_vec())]);

        extract(&first, &jars_root, &[entry("natives.jar")], &destination)
            .expect("first extraction succeeds");
        extract(
            &replacement,
            &jars_root,
            &[entry("natives.jar")],
            &destination,
        )
        .expect("second extraction succeeds");

        assert_eq!(
            fs::read(destination.join("lwjgl.dll")).expect("read replaced dll"),
            b"second"
        );
    }

    #[test]
    fn a_file_that_is_not_a_jar_is_reported() {
        let root = TempDir::new("not-a-jar");
        let jars_root = root.0.join("libraries");
        let destination = root.0.join("natives");
        let jar = jars_root.join("broken.jar");
        let reader = MemoryReader::default();

        let error = extract(&reader, &jars_root, &[entry("broken.jar")], &destination)
            .expect_err("a non-jar fails");

        match error {
            NativeError::NotAJar { path } => assert_eq!(path, jar),
            other => panic!("expected NotAJar, got {other:?}"),
        }
    }

    fn entry(path: &str) -> NativeEntry {
        NativeEntry { path: path.into() }
    }
}
