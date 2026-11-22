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
use axlauncher_spi::{FetchError, Network};

use crate::assets::{
    AssetIndexDocument, DownloadOutcome, MissingFile, download_all, missing_assets,
    missing_libraries,
};

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
        Ok(bytes) => bytes,
        Err(_) => network
            .fetch(&index.url)
            .await
            .map_err(|source| PrepareError::AssetIndex {
                id: index.id.clone(),
                source,
            })?,
    };
    let document =
        AssetIndexDocument::parse(&bytes).map_err(|error| PrepareError::AssetIndexUnreadable {
            id: index.id.clone(),
            reason: error.to_string(),
        })?;

    if !path.is_file() {
        axlauncher_storage::write_atomic(&path, &bytes).map_err(|error| {
            PrepareError::AssetIndexUnreadable {
                id: index.id.clone(),
                reason: error.to_string(),
            }
        })?;
    }

    Ok(document)
}

/// Downloads everything `version` needs that is not on disk.
pub async fn prepare(
    network: &impl Network,
    game_root: &Path,
    version: &Version,
    target: &Target,
    concurrency: usize,
) -> Result<Prepared, PrepareError> {
    let index = ensure_asset_index(network, game_root, version).await?;
    let files = outstanding(game_root, version, &index, target);
    let outcomes = download_all(network, &files, concurrency).await;
    let mut prepared = summarise(&outcomes);

    // The flat tree copies from the object store, so it is built after the
    // objects land. A version whose index asks for none does nothing here.
    if let Some(index_id) = version.asset_index.as_ref().map(|index| index.id.as_str()) {
        prepared
            .failures
            .extend(crate::assets::materialise_flat(game_root, index_id, &index));
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
mod tests {
    use super::*;
    use axlauncher_spi::Network;
    use std::collections::BTreeMap;
    use std::fs;

    struct FakeNetwork {
        bodies: BTreeMap<String, Vec<u8>>,
    }

    impl Network for FakeNetwork {
        async fn fetch(&self, url: &str) -> Result<Vec<u8>, FetchError> {
            self.bodies
                .get(url)
                .cloned()
                .ok_or_else(|| FetchError::Fatal(format!("no body for {url}")))
        }
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path = tempfile::tempdir().expect("temp directory").keep();
            Self(path)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        use std::task::{Context, Poll, Waker};

        let waker = Waker::noop();
        let mut context = Context::from_waker(waker);
        let mut future = std::pin::pin!(future);

        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(value) => return value,
                Poll::Pending => std::thread::yield_now(),
            }
        }
    }

    fn version_with_index(id: &str, url: &str) -> Version {
        Version {
            id: "1.21".into(),
            kind: "release".into(),
            main_class: "net.minecraft.client.main.Main".into(),
            libraries: Vec::new(),
            arguments: None,
            asset_index: Some(AssetIndex {
                id: id.into(),
                url: url.into(),
                sha1: None,
            }),
            assets: None,
            java_version: None,
            minecraft_arguments: None,
            inherits_from: None,
            jar: None,
            downloads: None,
            logging: None,
            processors: Vec::new(),
            data: BTreeMap::default(),
        }
    }

    #[test]
    fn an_absent_index_is_fetched_and_written() {
        let temp = TempDir::new();
        let body = br#"{"objects":{}}"#.to_vec();
        let network = FakeNetwork {
            bodies: BTreeMap::from([("https://example.test/34.json".to_string(), body.clone())]),
        };
        let version = version_with_index("34", "https://example.test/34.json");

        let index = block_on(ensure_asset_index(&network, &temp.0, &version)).expect("index");

        assert!(index.objects.is_empty());
        let path = asset_index_path(&temp.0, version.asset_index.as_ref().expect("index"));
        assert_eq!(fs::read(path).expect("written index"), body);
    }

    #[test]
    fn an_index_already_on_disk_is_not_fetched_again() {
        let temp = TempDir::new();
        let version = version_with_index("34", "https://example.test/unreachable.json");
        let path = asset_index_path(&temp.0, version.asset_index.as_ref().expect("index"));
        fs::create_dir_all(path.parent().expect("parent")).expect("index directory");
        fs::write(&path, br#"{"objects":{}}"#).expect("seed");

        // An empty network: any fetch would fail.
        let network = FakeNetwork {
            bodies: BTreeMap::new(),
        };
        let index = block_on(ensure_asset_index(&network, &temp.0, &version)).expect("index");

        assert!(index.objects.is_empty());
    }

    #[test]
    fn a_failed_fetch_names_the_index() {
        let temp = TempDir::new();
        let network = FakeNetwork {
            bodies: BTreeMap::new(),
        };
        let version = version_with_index("34", "https://example.test/34.json");

        let error =
            block_on(ensure_asset_index(&network, &temp.0, &version)).expect_err("must fail");

        assert!(matches!(error, PrepareError::AssetIndex { .. }));
        assert!(error.to_string().contains("34"));
    }

    #[test]
    fn an_unparsable_index_reports_the_index_id() {
        let temp = TempDir::new();
        let network = FakeNetwork {
            bodies: BTreeMap::from([(
                "https://example.test/34.json".to_string(),
                b"not json".to_vec(),
            )]),
        };
        let version = version_with_index("34", "https://example.test/34.json");

        let error =
            block_on(ensure_asset_index(&network, &temp.0, &version)).expect_err("must fail");

        assert!(matches!(error, PrepareError::AssetIndexUnreadable { .. }));
    }

    #[test]
    fn a_version_without_an_index_prepares_nothing_from_assets() {
        let temp = TempDir::new();
        let mut version = version_with_index("34", "https://example.test/34.json");
        version.asset_index = None;
        let network = FakeNetwork {
            bodies: BTreeMap::new(),
        };

        let index = block_on(ensure_asset_index(&network, &temp.0, &version)).expect("index");

        assert!(index.objects.is_empty());
    }

    #[test]
    fn summarise_counts_written_files_and_names_failures() {
        use crate::assets::DownloadOutcome;

        let outcomes = vec![
            DownloadOutcome {
                path: PathBuf::from("a.jar"),
                result: Ok(()),
            },
            DownloadOutcome {
                path: PathBuf::from("b.jar"),
                result: Err("sha1 mismatch".into()),
            },
        ];

        let prepared = summarise(&outcomes);

        assert_eq!(prepared.requested, 2);
        assert_eq!(prepared.written, 1);
        assert!(!prepared.is_complete());
        assert!(prepared.failures[0].contains("b.jar"));
    }

    #[test]
    fn a_complete_pass_reports_complete() {
        use crate::assets::DownloadOutcome;

        let prepared = summarise(&[DownloadOutcome {
            path: PathBuf::from("a.jar"),
            result: Ok(()),
        }]);

        assert!(prepared.is_complete());
    }
}
