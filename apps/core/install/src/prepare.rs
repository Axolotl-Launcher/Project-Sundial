// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Getting a version's files onto disk before a launch.
//!
//! The steps are the ones a fresh install needs: acquire the asset index if it
//! is missing, list everything absent, download and verify it, and report what
//! landed. Starting the game is a separate call, so a caller can show progress
//! between the two.

use std::path::{Path, PathBuf};

use axlauncher_launch::Target;
use axlauncher_launch::version::{AssetIndex, Version};
use axlauncher_network::verify;
use axlauncher_spi::{FetchError, Network};

use crate::assets::{
    AssetIndexDocument, DownloadOutcome, MissingFile, download_all_reporting, missing_assets,
    missing_libraries,
};

/// The phase and file counts a caller can display during installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    Version,
    CheckingFiles,
    Downloading { processed: usize, total: usize },
    Finishing,
}

/// Where the asset index for a version is stored, under `game_root`.
pub fn asset_index_path(game_root: &Path, index: &AssetIndex) -> PathBuf {
    game_root
        .join("assets")
        .join("indexes")
        .join(format!("{}.json", index.id))
}

/// A version's files that were absent and are now on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    /// How many files were requested.
    pub requested: usize,
    /// How many were written.
    pub written: usize,
    /// One line per file that could not be written, naming the file.
    pub failures: Vec<String>,
}

impl Prepared {
    /// Whether every requested file is now present and verified.
    pub fn is_complete(&self) -> bool {
        self.failures.is_empty()
    }
}

/// Why preparation stopped.
#[derive(Debug)]
pub enum PrepareError {
    /// The asset index is absent and could not be fetched.
    AssetIndex {
        /// The index id the version names.
        id: String,
        /// What the network reported.
        source: FetchError,
    },
    /// The asset index was fetched but does not parse.
    AssetIndexUnreadable {
        /// The index id the version names.
        id: String,
        /// The parse failure.
        reason: String,
    },
    /// A file that was written unverified does not match its recorded size.
    Verification {
        /// The file whose size disagreed.
        path: PathBuf,
    },
}

impl std::fmt::Display for PrepareError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AssetIndex { id, source } => {
                write!(formatter, "cannot fetch asset index {id}: {source}")
            }
            Self::AssetIndexUnreadable { id, reason } => {
                write!(formatter, "asset index {id} does not parse: {reason}")
            }
            Self::Verification { path } => {
                write!(
                    formatter,
                    "{} does not match its recorded size",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for PrepareError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::AssetIndex { source, .. } => Some(source),
            Self::AssetIndexUnreadable { .. } | Self::Verification { .. } => None,
        }
    }
}

/// Ensures the asset index document exists under `game_root`.
///
/// An index already on disk is read as it is: it is content-addressed by the
/// version document, so a second copy could only be the same bytes.
pub async fn ensure_asset_index(
    network: &impl Network,
    game_root: &Path,
    version: &Version,
) -> Result<AssetIndexDocument, PrepareError> {
    let Some(index) = version.asset_index.clone() else {
        return Ok(AssetIndexDocument::default());
    };

    let path = asset_index_path(game_root, &index);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => return parse_index(&index, &bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => network
            .fetch(&index.url)
            .await
            .map_err(|source| PrepareError::AssetIndex {
                id: index.id.clone(),
                source,
            })?,
        Err(error) => return Err(index_error(&index, error)),
    };
    let document = parse_index(&index, &bytes)?;
    axlauncher_storage::write_atomic(&path, &bytes).map_err(|error| index_error(&index, error))?;
    Ok(document)
}

pub(crate) fn parse_index(
    index: &AssetIndex,
    bytes: &[u8],
) -> Result<AssetIndexDocument, PrepareError> {
    if let Some(hash) = &index.sha1
        && !verify(bytes, hash)
    {
        return Err(index_error(index, "SHA-1 mismatch"));
    }
    AssetIndexDocument::parse(bytes).map_err(|error| index_error(index, error))
}

fn index_error(index: &AssetIndex, reason: impl ToString) -> PrepareError {
    PrepareError::AssetIndexUnreadable {
        id: index.id.clone(),
        reason: reason.to_string(),
    }
}

/// Downloads everything `version` needs that is not on disk.
pub async fn prepare(
    network: &impl Network,
    game_root: &Path,
    version: &Version,
    target: &Target,
    concurrency: usize,
) -> Result<Prepared, PrepareError> {
    prepare_reporting(network, game_root, version, target, concurrency, |_| {}).await
}

pub(crate) async fn prepare_reporting(
    network: &impl Network,
    game_root: &Path,
    version: &Version,
    target: &Target,
    concurrency: usize,
    mut report: impl FnMut(Progress),
) -> Result<Prepared, PrepareError> {
    report(Progress::CheckingFiles);
    let index = ensure_asset_index(network, game_root, version).await?;
    let files = outstanding(game_root, version, &index, target);
    let total = files.len();
    if total > 0 {
        report(Progress::Downloading {
            processed: 0,
            total,
        });
    }
    let outcomes = download_all_reporting(network, &files, concurrency, |processed| {
        report(Progress::Downloading { processed, total });
    })
    .await;
    report(Progress::Finishing);
    let mut prepared = summarise(&outcomes);

    // The flat tree copies from the object store, so it is built after the
    // objects land. A version whose index asks for none does nothing here.
    if let Some(index_id) = version.asset_index.as_ref().map(|index| index.id.as_str()) {
        let failures = if version.assets.as_deref() == Some("legacy") {
            crate::assets::materialise_flat_at(game_root, &game_root.join("resources"), &index)
        } else {
            crate::assets::materialise_flat(game_root, index_id, &index)
        };
        prepared.failures.extend(failures);
    }
    Ok(prepared)
}

/// The counts a caller reports after a download pass.
pub fn summarise(outcomes: &[DownloadOutcome]) -> Prepared {
    let mut failures = Vec::new();
    let mut written = 0;

    for outcome in outcomes {
        match &outcome.result {
            Ok(()) => written += 1,
            Err(reason) => failures.push(format!("{}: {reason}", outcome.path.display())),
        }
    }

    Prepared {
        requested: outcomes.len(),
        written,
        failures,
    }
}

/// The files a launch would still be missing.
///
/// The client jar and the logging configuration are part of this list, not a
/// separate pass: a version whose libraries are complete but whose jar is
/// absent fails at launch with an error the game never gets to print.
pub fn outstanding(
    game_root: &Path,
    version: &Version,
    index: &AssetIndexDocument,
    target: &Target,
) -> Vec<MissingFile> {
    let mut files: Vec<MissingFile> = [
        crate::assets::client_jar(game_root, version),
        crate::assets::log_config(game_root, version),
    ]
    .into_iter()
    .flatten()
    .collect();
    files.extend(missing_libraries(game_root, &version.libraries, target));
    files.extend(missing_assets(game_root, index));
    files
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/install/src/prepare.rs"]
mod tests;
