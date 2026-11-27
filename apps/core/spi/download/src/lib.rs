//! File transfer requests and the installation-facing download port.

#![forbid(unsafe_code)]

use std::io::{self, BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

use axlauncher_network_spi::DownloadProtocol;
use futures::stream::{self, Stream, StreamExt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha512};
use url::Url;

pub const DEFAULT_CONCURRENCY: usize = 256;
pub const DEFAULT_FILE_CONNECTIONS: usize = 16;

#[derive(Debug, Clone, Copy)]
pub struct DownloadJob<'a> {
    pub sources: &'a [&'a str],
    pub destination: &'a Path,
    pub sha1: Option<&'a str>,
    pub sha1_alternatives: &'a [String],
    pub size: Option<u64>,
    pub batch: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissingFile {
    pub url: String,
    pub path: PathBuf,
    pub sha1: Option<String>,
    pub sha1_alternatives: Vec<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadOutcome {
    pub path: PathBuf,
    pub result: Result<(), String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferStats {
    pub active_connections: usize,
    pub connection_limit: usize,
    pub connections_per_file: usize,
    pub protocol: DownloadProtocol,
    pub bytes_received: u64,
}

impl Default for TransferStats {
    fn default() -> Self {
        Self {
            active_connections: 0,
            connection_limit: DEFAULT_CONCURRENCY,
            connections_per_file: DEFAULT_FILE_CONNECTIONS,
            protocol: DownloadProtocol::default(),
            bytes_received: 0,
        }
    }
}

/// A whole-file digest the owning manifest requires in addition to any SHA-1.
#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum ExtraDigest {
    Sha512([u8; 32], [u8; 32]),
    Md5([u8; 16]),
}

impl ExtraDigest {
    pub fn sha512(hash: [u8; 64]) -> Self {
        let (prefix, suffix) = hash.split_at(32);
        Self::Sha512(prefix.try_into().unwrap(), suffix.try_into().unwrap())
    }

    pub fn verify(self, input: impl Read) -> io::Result<()> {
        let (valid, name) = match self {
            Self::Sha512(prefix, suffix) => {
                let mut digest = Sha512::new();
                hash(input, |bytes| digest.update(bytes))?;
                let hash = digest.finalize();
                (hash[..32] == prefix && hash[32..] == suffix, "SHA-512")
            }
            Self::Md5(expected) => {
                use md5::Digest as _;
                let mut digest = md5::Md5::new();
                hash(input, |bytes| digest.update(bytes))?;
                (digest.finalize()[..] == expected, "MD5")
            }
        };
        if valid {
            return Ok(());
        }
        Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{name} mismatch"),
        ))
    }
}

fn hash(input: impl Read, mut update: impl FnMut(&[u8])) -> io::Result<()> {
    let mut input = BufReader::new(input);
    loop {
        let bytes = input.fill_buf()?;
        if bytes.is_empty() {
            return Ok(());
        }
        update(bytes);
        let count = bytes.len();
        input.consume(count);
    }
}

/// The authenticated CDN and redirect allow-lists for supported modpack sources.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PackOrigins {
    Modrinth,
    CurseForge,
    None,
}

impl PackOrigins {
    pub const AUTHENTICATED_CDN: &str = "edge.forgecdn.net";

    pub fn allows(self, url: &Url, redirect: bool) -> bool {
        if url.scheme() != "https"
            || url.port().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return false;
        }
        match (self, url.host_str()) {
            (
                Self::Modrinth,
                Some(
                    "cdn.modrinth.com" | "github.com" | "raw.githubusercontent.com" | "gitlab.com",
                ),
            ) => true,
            (
                Self::Modrinth,
                Some("release-assets.githubusercontent.com" | "objects.githubusercontent.com"),
            ) => redirect,
            (Self::CurseForge, Some(Self::AUTHENTICATED_CDN | "media.forgecdn.net")) => true,
            _ => false,
        }
    }
}

/// Promotes a file only after it passes the requested integrity checks.
pub trait FileDownloader {
    fn download(
        &self,
        job: DownloadJob<'_>,
        digest: Option<ExtraDigest>,
    ) -> impl Future<Output = Result<(), String>>;

    /// Reports file outcomes in completion order. Dropping the stream cancels unfinished work.
    fn batch<'a>(
        &'a self,
        files: &'a [MissingFile],
        concurrency: usize,
    ) -> impl Stream<Item = DownloadOutcome> + 'a {
        batch(self, files, concurrency)
    }
}

pub fn batch<'a>(
    downloader: &'a (impl FileDownloader + ?Sized),
    files: &'a [MissingFile],
    concurrency: usize,
) -> impl Stream<Item = DownloadOutcome> + 'a {
    let limit = if concurrency == 0 {
        DEFAULT_CONCURRENCY
    } else {
        concurrency
    };
    stream::iter(files)
        .map(move |file| async move {
            let result = downloader
                .download(
                    DownloadJob {
                        sources: &[&file.url],
                        destination: &file.path,
                        sha1: file.sha1.as_deref(),
                        sha1_alternatives: &file.sha1_alternatives,
                        size: file.size,
                        batch: true,
                    },
                    None,
                )
                .await;
            DownloadOutcome {
                path: file.path.clone(),
                result,
            }
        })
        .buffer_unordered(limit)
}
