// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! The parts of a Minecraft version document the launcher reads.
//!
//! Only what a launch needs is modelled. Fields this build does not use are
//! ignored by serde rather than rejected, so a newer game version does not make
//! an older launcher fail to parse it.

use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Why a version document could not be used.
#[derive(Debug)]
pub enum VersionError {
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
            Self::Read { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
        }
    }
}

/// A version document as Mojang publishes it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Version {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(rename = "mainClass")]
    pub main_class: String,
    pub libraries: Vec<Library>,
    #[serde(default)]
    pub arguments: Option<Arguments>,
    #[serde(default, rename = "assetIndex")]
    pub asset_index: Option<AssetIndex>,
    #[serde(default)]
    pub assets: Option<String>,
    #[serde(default, rename = "javaVersion")]
    pub java_version: Option<JavaVersion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct AssetIndex {
    pub id: String,
    /// Where the index document can be fetched when it is not on disk.
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub sha1: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct JavaVersion {
    #[serde(rename = "majorVersion")]
    pub major: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(untagged)]
pub enum Argument {
    Plain(String),
    Conditional {
        rules: Vec<Rule>,
        value: ValueOrValues,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Library {
    pub name: String,
    #[serde(default)]
    pub downloads: Option<LibraryDownloads>,
    #[serde(default)]
    pub natives: Option<std::collections::BTreeMap<String, String>>,
    #[serde(default)]
    pub rules: Option<Vec<Rule>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct LibraryDownloads {
    #[serde(default)]
    pub artifact: Option<Artifact>,
    #[serde(default)]
    pub classifiers: Option<std::collections::BTreeMap<String, Artifact>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Artifact {
    pub path: String,
    #[serde(default)]
    pub sha1: Option<String>,
    #[serde(default)]
    pub size: Option<u64>,
    #[serde(default)]
    pub url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Rule {
    pub action: String,
    #[serde(default)]
    pub os: Option<OsRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct OsRule {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
}

/// Reads `<root>/<id>/<id>.json`.
pub fn load(root: &Path, id: &str) -> Result<Version, VersionError> {
    let path = root.join(id).join(format!("{id}.json"));
    let bytes = std::fs::read(&path).map_err(|source| VersionError::Read {
        path: path.clone(),
        source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| VersionError::Parse { path, source })
}
