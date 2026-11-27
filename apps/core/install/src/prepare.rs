//! File plans, progress and local readiness shared by installation and launch.

use axlauncher_download_spi::{DownloadOutcome, MissingFile};
use axlauncher_launch::Target;
use axlauncher_launch::version::{AssetIndex, Version};
use axlauncher_network::verify;
use axlauncher_network_spi::FetchError;
use axlauncher_storage::{AssetPath, GameDir};
use serde::{Deserialize, Serialize};

use crate::assets::{self, AssetIndexDocument};

/// The phase and file counts a caller can display during installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum Progress {
    Version,
    Loader,
    Java,
    CheckingFiles,
    Downloading { processed: usize, total: usize },
    Finishing,
}

/// The outcome of preparing files absent from a version's game tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prepared {
    pub requested: usize,
    pub written: usize,
    pub failures: Vec<String>,
}

impl Prepared {
    pub fn is_complete(&self) -> bool {
        self.failures.is_empty()
    }
}

/// Why an asset index could not be used.
#[derive(Debug)]
pub enum PrepareError {
    AssetIndex { id: String, source: FetchError },
    AssetIndexUnreadable { id: String, reason: String },
}

impl std::fmt::Display for PrepareError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AssetIndex { id, source } => {
                write!(formatter, "cannot fetch asset index {id}: {source}")
            }
            Self::AssetIndexUnreadable { id, reason } => {
                write!(formatter, "cannot read asset index {id}: {reason}")
            }
        }
    }
}

impl std::error::Error for PrepareError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::AssetIndex { source, .. } => Some(source),
            Self::AssetIndexUnreadable { .. } => None,
        }
    }
}

pub fn parse_index(index: &AssetIndex, bytes: &[u8]) -> Result<AssetIndexDocument, PrepareError> {
    if let Some(hash) = &index.sha1
        && !verify(bytes, hash)
    {
        return Err(index_error(index, "SHA-1 mismatch"));
    }
    AssetIndexDocument::parse(bytes).map_err(|error| index_error(index, error))
}

pub fn index_error(index: &AssetIndex, reason: impl ToString) -> PrepareError {
    PrepareError::AssetIndexUnreadable {
        id: index.id.clone(),
        reason: reason.to_string(),
    }
}

/// Counts and names file outcomes from a download pass.
pub fn summarise(outcomes: &[DownloadOutcome]) -> Prepared {
    let failures: Vec<_> = outcomes
        .iter()
        .filter_map(|outcome| {
            let reason = outcome.result.as_ref().err()?;
            Some(format!("{}: {reason}", outcome.path.display()))
        })
        .collect();
    Prepared {
        requested: outcomes.len(),
        written: outcomes.len() - failures.len(),
        failures,
    }
}

/// Downloadable files absent from the game tree.
pub fn outstanding(
    game_root: &GameDir,
    version: &Version,
    index: &AssetIndexDocument,
    target: &Target,
) -> Vec<MissingFile> {
    missing_files(game_root, version, index, target, false)
}

fn missing_files(
    game_root: &GameDir,
    version: &Version,
    index: &AssetIndexDocument,
    target: &Target,
    include_local: bool,
) -> Vec<MissingFile> {
    let mut files: Vec<_> = [
        assets::client_jar(game_root, version),
        assets::log_config(game_root, version),
    ]
    .into_iter()
    .flatten()
    .collect();
    files.extend(assets::missing_library_files(
        game_root,
        &version.libraries,
        target,
        include_local,
    ));
    files.extend(assets::missing_assets(game_root, index));
    files
}

/// Readiness requires downloaded files and flat asset copies; loader callers
/// reject a version whose processor pass did not finish.
pub fn check_local_files(
    game_root: &GameDir,
    version: &Version,
    target: &Target,
) -> Result<(), String> {
    let index = local_index(game_root, version.asset_index.as_ref())?;
    let missing = missing_files(game_root, version, &index, target, true);
    if let Some(file) = missing.first() {
        return Err(format!(
            "{} required game file(s) are missing or incomplete. First: {}",
            missing.len(),
            file.path.display()
        ));
    }
    if let Some(asset_index) = &version.asset_index
        && let Some((path, _)) =
            assets::missing_flat_copies(game_root, &asset_index.id, &index).next()
    {
        return Err(format!(
            "A required game asset is missing or incomplete: {}",
            path.display()
        ));
    }
    Ok(())
}

fn local_index(
    game_root: &GameDir,
    index: Option<&AssetIndex>,
) -> Result<AssetIndexDocument, String> {
    let Some(index) = index else {
        return Ok(AssetIndexDocument::default());
    };
    let path = game_root.asset_path(AssetPath::Index(&index.id));
    let bytes =
        std::fs::read(&path).map_err(|error| format!("Cannot read {}: {error}", path.display()))?;
    parse_index(index, &bytes).map_err(|error| error.to_string())
}
