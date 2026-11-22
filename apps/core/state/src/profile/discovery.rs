// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Directory-backed discovery of portable instances.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{FILE_NAME, Profile, SCHEMA_VERSION, invalid, is_link, read};

#[derive(Debug)]
pub enum DiscoveryError {
    Read {
        path: PathBuf,
        source: io::Error,
    },
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    UnsupportedSchema {
        path: PathBuf,
        found: u32,
    },
    InvalidManifest {
        path: PathBuf,
        reason: String,
    },
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(formatter, "cannot read {}: {source}", path.display())
            }
            Self::Parse { path, source } => {
                write!(formatter, "cannot parse {}: {source}", path.display())
            }
            Self::UnsupportedSchema { path, found } => write!(
                formatter,
                "{} uses schema {found}; this build reads schema {SCHEMA_VERSION}",
                path.display()
            ),
            Self::InvalidManifest { path, reason } => {
                write!(formatter, "invalid profile {}: {reason}", path.display())
            }
        }
    }
}

impl std::error::Error for DiscoveryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::UnsupportedSchema { .. } | Self::InvalidManifest { .. } => None,
        }
    }
}

/// Valid instances together with per-directory discovery errors.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct ProfileDiscovery {
    pub instances: Vec<DiscoveredProfile>,
    pub issues: Vec<DiscoveryError>,
}

/// The physical directory is independent of the UUID in the manifest.
#[derive(Debug)]
#[non_exhaustive]
pub struct DiscoveredProfile {
    pub directory: PathBuf,
    pub profile: Profile,
}

/// The direct children with readable `.axlprofile` files, ordered by name.
pub fn discover(profiles: &Path) -> ProfileDiscovery {
    let mut result = ProfileDiscovery::default();
    let entries = match fs::read_dir(profiles) {
        Ok(entries) => entries,
        Err(source) if source.kind() == io::ErrorKind::NotFound => return result,
        Err(source) => {
            result.issues.push(DiscoveryError::Read {
                path: profiles.to_path_buf(),
                source,
            });
            return result;
        }
    };
    for entry in entries {
        let discovered = entry
            .map_err(|source| DiscoveryError::Read {
                path: profiles.to_path_buf(),
                source,
            })
            .and_then(discover_entry);
        match discovered {
            Ok(Some(entry)) => result.instances.push(entry),
            Ok(None) => (),
            Err(error) => result.issues.push(error),
        }
    }
    result.instances.sort_by(|a, b| {
        a.profile
            .instance
            .name
            .cmp(&b.profile.instance.name)
            .then_with(|| a.directory.cmp(&b.directory))
    });
    report_duplicate_ids(&result.instances, &mut result.issues);
    result
}

fn discover_entry(entry: fs::DirEntry) -> Result<Option<DiscoveredProfile>, DiscoveryError> {
    let directory = entry.path();
    let metadata = entry.metadata().map_err(|source| DiscoveryError::Read {
        path: directory.clone(),
        source,
    })?;
    if !metadata.is_dir() || is_link(&metadata) {
        return Ok(None);
    }
    Ok(read(&directory)?.map(|profile| DiscoveredProfile { directory, profile }))
}

fn report_duplicate_ids(instances: &[DiscoveredProfile], issues: &mut Vec<DiscoveryError>) {
    let mut first_by_id = HashMap::new();
    for entry in instances {
        let first = first_by_id
            .entry(&entry.profile.id)
            .or_insert(&entry.directory);
        if *first != &entry.directory {
            issues.push(invalid(
                &entry.directory.join(FILE_NAME),
                format!("profile ID also used by {}", first.display()),
            ));
        }
    }
}
