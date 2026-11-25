// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The parts of a Minecraft version document the launcher reads.
//!
//! Only what a launch needs is modelled. Fields this build does not use are
//! ignored by serde rather than rejected, so a newer game version does not make
//! an older launcher fail to parse it.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use axlauncher_domain::is_version_id;
use serde::{Deserialize, Serialize};

/// Why a version document could not be used.
#[derive(Debug)]
pub enum VersionError {
    InvalidId {
        id: String,
    },
    Read {
        path: PathBuf,
        source: io::Error,
    },
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
}

impl std::fmt::Display for VersionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidId { id } => {
                write!(
                    formatter,
                    "invalid version id `{id}`: expected one directory name"
                )
            }
            Self::Read { path, source } => {
                write!(formatter, "cannot read {}: {source}", path.display())
            }
            Self::Parse { path, source } => {
                write!(formatter, "cannot parse {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for VersionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidId { .. } => None,
            Self::Read { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
        }
    }
}

/// A version document as Mojang publishes it.
///
/// A loader's profile is the same shape plus the fields that describe how it
/// is derived from a vanilla one. Keeping them in this type is what lets a
/// caller hold one value whether the document came from Mojang or from a
/// loader's installer.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Version {
    pub id: String,
    #[serde(rename = "type", default)]
    pub kind: String,
    #[serde(rename = "mainClass", default)]
    pub main_class: String,
    #[serde(default)]
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub arguments: Option<Arguments>,
    /// The flat argument string versions before 1.13 use.
    #[serde(default, rename = "minecraftArguments")]
    pub minecraft_arguments: Option<String>,
    #[serde(default, rename = "assetIndex")]
    pub asset_index: Option<AssetIndex>,
    #[serde(default)]
    pub assets: Option<String>,
    #[serde(default, rename = "javaVersion")]
    pub java_version: Option<JavaVersion>,
    /// The vanilla version a loader's profile is layered onto.
    #[serde(default, rename = "inheritsFrom")]
    pub inherits_from: Option<String>,
    /// The jar under `versions/` the game runs from, when it is not this
    /// document's own id.
    #[serde(default)]
    pub jar: Option<String>,
    /// The client jar download, which an install needs and a launch does not.
    #[serde(default)]
    pub downloads: Option<Downloads>,
    /// The logging configuration the JVM arguments point at.
    #[serde(default)]
    pub logging: Option<Logging>,
    /// The install steps a loader's profile declares.
    #[serde(default)]
    pub processors: Vec<Processor>,
    /// The values a processor's arguments refer to, keyed by the name it names.
    #[serde(default)]
    pub data: BTreeMap<String, DataEntry>,
}

/// The downloads a version document names for the game itself.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
pub struct Downloads {
    #[serde(default)]
    pub client: Option<GameDownload>,
}

/// One downloadable game file.
///
/// Unlike a library artifact it has no relative path: the game's own files are
/// named by the version directory they belong to, not by a Maven coordinate.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct GameDownload {
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub url: String,
}

/// The logging configuration the game is started with.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Logging {
    #[serde(default)]
    pub client: Option<LoggingClient>,
}

/// One side's logging configuration: the JVM argument and the file it names.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct LoggingClient {
    /// The argument with a `${path}` placeholder for the configuration file.
    pub argument: String,
    pub file: LoggingFile,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
}

/// The logging configuration file, named by id rather than by Maven path.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct LoggingFile {
    /// The name used for the file in the shared logging directory.
    pub id: String,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub url: String,
}

/// One step of a loader's install, run before the game is started.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Processor {
    /// The Maven coordinate of the tool that runs this step.
    pub jar: String,
    #[serde(default)]
    pub classpath: Vec<String>,
    #[serde(default)]
    pub args: Vec<String>,
    /// The files this step produces, as argument templates.
    #[serde(default)]
    pub outputs: BTreeMap<String, String>,
    /// Which side this step belongs to, when it names one.
    #[serde(default)]
    pub sides: Option<Vec<String>>,
}

/// One named value a processor's arguments refer to.
///
/// The value may be a literal, a `[Maven coordinate]` naming a library, or a
/// `'quoted literal'`; the brackets and quotes are part of how it is written.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct DataEntry {
    pub client: String,
    #[serde(default)]
    pub server: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct AssetIndex {
    pub id: String,
    /// Where the index document can be fetched when it is not on disk.
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub sha1: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct JavaVersion {
    #[serde(rename = "majorVersion")]
    pub major: u32,
}

impl Version {
    /// The jar under `versions/` the game runs from.
    ///
    /// A loader's profile names the vanilla jar here, because the loader's own
    /// code arrives on the classpath and the game jar stays the one Mojang
    /// shipped. A document without the field runs from its own folder.
    pub fn jar_id(&self) -> &str {
        self.jar.as_deref().unwrap_or(&self.id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize, Serialize)]
pub struct Arguments {
    #[serde(default)]
    pub game: Vec<Argument>,
    #[serde(default)]
    pub jvm: Vec<Argument>,
    /// Arguments for a launcher's own JVM, which this build does not use.
    #[serde(default)]
    pub default_user_jvm: Vec<Argument>,
}

/// One argument: either always present or gated by rules.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Argument {
    Plain(String),
    Conditional {
        rules: Vec<Rule>,
        value: ValueOrValues,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ValueOrValues {
    One(String),
    Many(Vec<String>),
}

impl ValueOrValues {
    pub fn as_slice(&self) -> &[String] {
        match self {
            Self::One(value) => std::slice::from_ref(value),
            Self::Many(values) => values,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: Option<LibraryDownloads>,
    #[serde(default)]
    pub natives: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    pub rules: Option<Vec<Rule>>,
    /// A Maven repository a loader names instead of listing a download.
    #[serde(default)]
    pub url: Option<String>,
    /// Whether the library belongs on the game's classpath. A loader sets this
    /// to false for a tool it runs itself.
    #[serde(default = "default_true")]
    pub include_in_classpath: bool,
}

fn default_true() -> bool {
    true
}

impl Library {
    /// The path this library's jar occupies under a Maven layout, derived from
    /// its coordinate when the document states no download.
    ///
    /// A coordinate is `group:artifact:version[:classifier][@extension]`, and
    /// the layout nests it as `group/as/path/artifact/version/artifact-version
    /// [-classifier].extension`. Loaders such as Fabric name their Maven root
    /// in `url` and rely on this derivation instead of `downloads`.
    pub fn maven_path(&self) -> Option<PathBuf> {
        maven_path(&self.name)
    }

    /// The repository a derived download comes from.
    pub fn maven_root(&self) -> Option<&str> {
        self.url.as_deref().filter(|url| !url.is_empty())
    }

    /// The download the document states, or the one its coordinate implies.
    pub fn download(&self) -> Option<Artifact> {
        if let Some(stated) = self
            .downloads
            .as_ref()
            .and_then(|downloads| downloads.artifact.clone())
        {
            return Some(stated);
        }
        let root = self.maven_root()?;
        let path = self.maven_path()?;
        let relative = path.to_string_lossy().replace('\\', "/");
        Some(Artifact {
            url: Some(format!("{}/{relative}", root.trim_end_matches('/'))),
            path: relative,
            sha1: None,
            sha1_alternatives: Vec::new(),
            size: None,
        })
    }

    /// Whether this library declares natives rather than Java classes.
    ///
    /// A native-only declaration publishes no plain jar anywhere, so it must
    /// never become a classpath entry; its files arrive by extraction instead.
    pub fn is_native_only(&self) -> bool {
        let has_artifact = self
            .downloads
            .as_ref()
            .and_then(|downloads| downloads.artifact.as_ref())
            .is_some();
        !has_artifact
            && (self.natives.is_some()
                || self
                    .downloads
                    .as_ref()
                    .and_then(|downloads| downloads.classifiers.as_ref())
                    .is_some_and(|classifiers| {
                        classifiers.keys().any(|key| key.starts_with("native"))
                    }))
    }

    /// Whether a four-part coordinate marks this as a native archive.
    pub fn is_classified_native(&self) -> bool {
        classifier(&self.name).is_some_and(|classifier| classifier.starts_with("natives-"))
    }

    /// The classifier part of the coordinate, when it has one.
    pub fn classifier(&self) -> Option<&str> {
        classifier(&self.name)
    }
}

/// The classifier in a `group:artifact:version[:classifier][@ext]` coordinate.
fn classifier(name: &str) -> Option<&str> {
    let without_extension = name.split('@').next().unwrap_or(name);
    without_extension.split(':').nth(3)
}

/// The Maven path a coordinate maps onto.
///
/// The rule is shared with the loader providers, which address their own
/// artifacts the same way, so it lives in the domain crate.
pub fn maven_path(coordinate: &str) -> Option<PathBuf> {
    axlauncher_domain::maven::path(coordinate)
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct LibraryDownloads {
    #[serde(default)]
    pub artifact: Option<Artifact>,
    #[serde(default)]
    pub classifiers: Option<std::collections::BTreeMap<String, Artifact>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Artifact {
    pub path: String,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sha1_alternatives: Vec<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Rule {
    pub action: String,
    #[serde(default)]
    pub os: Option<OsRule>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub features: BTreeMap<String, bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct OsRule {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
}

/// Reads `<root>/<id>/<id>.json` without resolving what it inherits.
pub fn load(root: &Path, id: &str) -> Result<Version, VersionError> {
    let version: Version = read_document(root, id)?;

    for candidate in [
        Some(version.id.as_str()),
        version.jar.as_deref(),
        version.inherits_from.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if !is_version_id(candidate) {
            return Err(VersionError::InvalidId {
                id: candidate.to_string(),
            });
        }
    }

    Ok(version)
}

/// The original game's publication date, independent of the loader's build.
pub(crate) fn release_time(root: &Path, id: &str) -> Result<Option<String>, VersionError> {
    #[derive(Deserialize)]
    struct Publication {
        #[serde(rename = "releaseTime")]
        release_time: Option<String>,
    }

    read_document::<Publication>(root, id).map(|publication| publication.release_time)
}

fn read_document<T: serde::de::DeserializeOwned>(root: &Path, id: &str) -> Result<T, VersionError> {
    if !is_version_id(id) {
        return Err(VersionError::InvalidId { id: id.to_string() });
    }
    let path = root.join(id).join(format!("{id}.json"));
    let bytes = std::fs::read(&path).map_err(|source| VersionError::Read {
        path: path.clone(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| VersionError::Parse { path, source })
}

/// Reads `<root>/<id>/<id>.json` and everything it inherits from.
///
/// A loader's profile names its vanilla version in `inheritsFrom` and carries
/// only the fields it changes, so the document on disk is not launchable by
/// itself. [`crate::chain::resolve`] applies the parent first and then the
/// profile's own values, which is the order the game's format defines.
pub fn load_resolved(root: &Path, id: &str) -> Result<Version, crate::chain::ChainError> {
    let version = load(root, id).map_err(|source| crate::chain::ChainError::Document { source })?;
    crate::chain::resolve(root, version)
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/launch/src/version.rs"]
mod tests;
