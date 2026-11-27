// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Mojang's Java runtimes, fetched as the files a version needs.
//!
//! A version document states the Java major it was built for, and the game
//! ships a runtime for it. Downloading that runtime is the same pipeline as the
//! game's own files: a manifest names every file with a hash, and a verified
//! copy lands under the launcher's runtime directory. The official runtime is
//! the one the game was tested against, so a machine without the right JDK
//! still gets a working launch.

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use axlauncher_network::verify;
use axlauncher_network_spi::{FetchError, FetchKind, Network};
use axlauncher_storage::{Document, write_atomic};
use futures::stream::{self, StreamExt};
use serde::Deserialize;

use crate::prepare::Progress;

mod record;

/// The index of every runtime Mojang publishes.
const RUNTIME_INDEX: &str = "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";

/// How many runtime files are fetched at once.
const RUNTIME_CONCURRENCY: usize = 8;
const LOCK_RETRY_DELAY: Duration = Duration::from_millis(100);

/// The Java executable's file name on this platform.
const JAVA_BIN: &str = if cfg!(windows) { "java.exe" } else { "java" };

/// The component that provides a Java major, in Mojang's own naming.
///
/// The mapping is Mojang's, not a guess: a version that asks for 17 is shipped
/// `java-runtime-gamma`, and one that asks for 25 is `java-runtime-epsilon`.
pub fn component_for(major: u32) -> Option<&'static str> {
    match major {
        8 => Some("jre-legacy"),
        16 => Some("java-runtime-alpha"),
        17 => Some("java-runtime-gamma"),
        21 => Some("java-runtime-delta"),
        25 => Some("java-runtime-epsilon"),
        _ => None,
    }
}

/// The platform key this build's runtimes are published under.
pub fn platform() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("windows-x64"),
        ("windows", "x86") => Some("windows-x86"),
        ("windows", "aarch64") => Some("windows-arm64"),
        ("macos", "x86_64") => Some("mac-os"),
        ("macos", "aarch64") => Some("mac-os-arm64"),
        ("linux", "x86_64") => Some("linux"),
        ("linux", "x86") => Some("linux-i386"),
        _ => None,
    }
}

/// A runtime that is on disk and can be started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledRuntime {
    /// The Java executable.
    pub java: PathBuf,
    /// The runtime's own version, as Mojang names it.
    pub full_version: String,
    /// The Java major it provides.
    pub major: u32,
    /// The component directory it lives in.
    pub component: String,
}

/// Why a runtime was not installed.
#[derive(Debug)]
pub enum RuntimeError {
    /// No published runtime exists for this Java major.
    Unsupported { major: u32 },
    /// This platform is not one Mojang publishes runtimes for.
    UnsupportedPlatform,
    /// A fetch failed.
    Fetch { url: String, source: FetchError },
    /// A fetched document or object is not what its hash says it is.
    Verify { path: String },
    /// A document did not parse.
    Malformed { reason: String },
    /// A file could not be written.
    Write { path: PathBuf, source: io::Error },
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported { major } => {
                write!(
                    formatter,
                    "no published Java runtime provides version {major}"
                )
            }
            Self::UnsupportedPlatform => {
                formatter.write_str("this platform has no published Java runtime")
            }
            Self::Fetch { url, source } => write!(formatter, "cannot fetch {url}: {source}"),
            Self::Verify { path } => write!(formatter, "{path} failed its SHA-1 check"),
            Self::Malformed { reason } => write!(formatter, "invalid runtime metadata: {reason}"),
            Self::Write { path, source } => {
                write!(formatter, "cannot write {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Fetch { source, .. } => Some(source),
            Self::Write { source, .. } => Some(source),
            Self::Unsupported { .. }
            | Self::UnsupportedPlatform
            | Self::Verify { .. }
            | Self::Malformed { .. } => None,
        }
    }
}

type Index = BTreeMap<String, BTreeMap<String, Vec<Entry>>>;

#[derive(Deserialize)]
struct Entry {
    version: EntryVersion,
    manifest: ManifestRef,
}

#[derive(Deserialize)]
struct EntryVersion {
    name: String,
}

#[derive(Deserialize)]
struct ManifestRef {
    url: String,
    sha1: String,
}

#[derive(Deserialize)]
struct RuntimeManifest {
    files: BTreeMap<String, RuntimeFile>,
}

#[derive(Deserialize)]
pub(super) struct RuntimeFile {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    executable: bool,
    #[serde(default)]
    downloads: Option<Downloads>,
}

impl RuntimeFile {
    /// The digest the file must have on disk, whichever form the manifest used.
    ///
    /// `raw` is the decompressed file, and it is what lands on disk whether or
    /// not the object was shipped packed, so it is the digest to check.
    pub(super) fn digest(&self) -> Option<&str> {
        self.downloads
            .as_ref()?
            .raw
            .as_ref()
            .map(|raw| raw.sha1.as_str())
    }
}

#[derive(Deserialize)]
struct Downloads {
    #[serde(default)]
    raw: Option<Object>,
    #[serde(default)]
    lzma: Option<Object>,
}

#[derive(Deserialize)]
struct Object {
    url: String,
    sha1: String,
}

/// A runtime for `major` is fetched once and reused: a directory carrying this
/// launcher's completion marker is accepted as the installed answer.
pub async fn install(
    network: &impl Network,
    runtimes: &Path,
    major: u32,
    mut report: impl FnMut(Progress),
) -> Result<InstalledRuntime, RuntimeError> {
    let component = component_for(major).ok_or(RuntimeError::Unsupported { major })?;
    let platform = platform().ok_or(RuntimeError::UnsupportedPlatform)?;
    let directory = runtimes.join(component);
    let _lock = wait_for_runtime(&directory).await?;
    report(Progress::Version);

    let entry = runtime_entry(network, platform, component, major).await?;

    let java = executable_in(&directory, platform);
    // A verified installation can be reused without fetching the file manifest.
    if let Some(recorded) = record::read(&directory)
        && java.is_file()
        && record::matches(&directory, &recorded)
    {
        return Ok(installed(java, &entry.version.name, major, component));
    }

    record::invalidate(&directory)?;
    let manifest = runtime_manifest(network, &entry.manifest).await?;
    let recorded = record::from_manifest(&manifest.files);
    place_files(network, &directory, manifest.files, &mut report).await?;
    if !java.is_file() {
        return Err(RuntimeError::Malformed {
            reason: format!("the runtime has no {}", java.display()),
        });
    }
    record::publish(&directory, &recorded)?;
    report(Progress::Finishing);
    Ok(installed(java, &entry.version.name, major, component))
}

async fn wait_for_runtime(directory: &Path) -> Result<fs::File, RuntimeError> {
    loop {
        match Document::RuntimeReceipt
            .lock(directory)
            .map_err(io::Error::from)
        {
            Ok(lock) => return Ok(lock),
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                tokio::time::sleep(LOCK_RETRY_DELAY).await;
            }
            Err(source) => {
                return Err(RuntimeError::Write {
                    path: directory.to_owned(),
                    source,
                });
            }
        }
    }
}

async fn place_files(
    network: &impl Network,
    directory: &Path,
    files: BTreeMap<String, RuntimeFile>,
    report: &mut impl FnMut(Progress),
) -> Result<(), RuntimeError> {
    let total = files.len();
    report(Progress::Downloading {
        processed: 0,
        total,
    });
    let mut processed = 0;
    let mut results = stream::iter(files)
        .map(|(name, file)| async move { place(network, directory, &name, &file).await })
        .buffer_unordered(RUNTIME_CONCURRENCY);
    while let Some(result) = results.next().await {
        result?;
        processed += 1;
        report(Progress::Downloading { processed, total });
    }

    Ok(())
}

/// Fetches and verifies the manifest a runtime entry names.
async fn runtime_manifest(
    network: &impl Network,
    reference: &ManifestRef,
) -> Result<RuntimeManifest, RuntimeError> {
    let bytes = verified(
        network,
        &reference.url,
        &reference.sha1,
        FetchKind::Metadata,
    )
    .await?;
    serde_json::from_slice(&bytes).map_err(|error| RuntimeError::Malformed {
        reason: error.to_string(),
    })
}

fn installed(java: PathBuf, full_version: &str, major: u32, component: &str) -> InstalledRuntime {
    InstalledRuntime {
        java,
        full_version: full_version.to_owned(),
        major,
        component: component.to_owned(),
    }
}

/// Where a runtime's Java executable sits.
fn executable_in(directory: &Path, platform: &str) -> PathBuf {
    home_of(directory, platform).join("bin").join(JAVA_BIN)
}

/// The JDK home inside a runtime's component directory.
///
/// A macOS runtime is a bundle, so its home sits three levels down. Everywhere
/// else the component directory is the home.
pub fn home_of(directory: &Path, platform: &str) -> PathBuf {
    if platform.starts_with("mac-os") {
        return directory.join("jre.bundle/Contents/Home");
    }
    directory.to_path_buf()
}

/// Writes one runtime file, verified.
async fn place(
    network: &impl Network,
    directory: &Path,
    name: &str,
    file: &RuntimeFile,
) -> Result<(), RuntimeError> {
    let target = safe_join(directory, name).ok_or_else(|| RuntimeError::Malformed {
        reason: format!("{name:?} is not a path inside the runtime"),
    })?;

    if file.kind == "directory" {
        return fs::create_dir_all(&target).map_err(|source| RuntimeError::Write {
            path: target,
            source,
        });
    }
    if file.kind == "link" {
        // Mojang's manifests list symlinks for the license files inside each
        // legal directory. A JVM runs without them, and creating links needs
        // privileges a desktop app does not have everywhere, so the entry is
        // skipped rather than failing the whole runtime download.
        return Ok(());
    }

    let Some(downloads) = file.downloads.as_ref() else {
        return Ok(());
    };
    let Some(bytes) = object_bytes(network, downloads).await? else {
        return Ok(());
    };

    write_atomic(&target, &bytes).map_err(|source| RuntimeError::Write {
        path: target.clone(),
        source,
    })?;
    if file.executable {
        make_executable(&target)?;
    }
    Ok(())
}

/// The bytes one manifest object stands for, decompressed when it is packed.
async fn object_bytes(
    network: &impl Network,
    downloads: &Downloads,
) -> Result<Option<Vec<u8>>, RuntimeError> {
    let Some(lzma) = &downloads.lzma else {
        return match &downloads.raw {
            Some(raw) => Ok(Some(
                verified(network, &raw.url, &raw.sha1, FetchKind::File).await?,
            )),
            None => Ok(None),
        };
    };

    let compressed = verified(network, &lzma.url, &lzma.sha1, FetchKind::File).await?;
    // Mojang stores the object behind `lzma` as an LZMA-alone stream, not an
    // XZ container; an XZ decoder rejects the first bytes of every one.
    let stream = xz2::stream::Stream::new_lzma_decoder(u64::MAX).map_err(|error| {
        RuntimeError::Malformed {
            reason: format!("cannot start the LZMA decoder: {error}"),
        }
    })?;
    let mut decoder = xz2::read::XzDecoder::new_stream(compressed.as_slice(), stream);
    let mut decoded = Vec::new();
    std::io::Read::read_to_end(&mut decoder, &mut decoded).map_err(|error| {
        RuntimeError::Malformed {
            reason: format!("{} does not decompress: {error}", lzma.url),
        }
    })?;
    if let Some(raw) = &downloads.raw
        && !verify(&decoded, &raw.sha1)
    {
        return Err(RuntimeError::Verify {
            path: raw.url.clone(),
        });
    }
    Ok(Some(decoded))
}

/// One object, fetched and checked against its hash.
async fn verified(
    network: &impl Network,
    url: &str,
    sha1: &str,
    kind: FetchKind,
) -> Result<Vec<u8>, RuntimeError> {
    network
        .fetch_with(url, kind, |response| {
            let bytes = response.map_err(|source| fetch_error(url, source))?;
            if !verify(&bytes, sha1) {
                return Err(RuntimeError::Verify { path: url.into() });
            }
            Ok(bytes)
        })
        .await
}

fn make_executable(path: &Path) -> Result<(), RuntimeError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)
            .map_err(|source| RuntimeError::Write {
                path: path.to_path_buf(),
                source,
            })?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).map_err(|source| RuntimeError::Write {
            path: path.to_path_buf(),
            source,
        })?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

/// A manifest path, checked to stay under the runtime directory.
pub(super) fn safe_join(root: &Path, name: &str) -> Option<PathBuf> {
    let mut relative = PathBuf::new();
    for component in Path::new(name).components() {
        match component {
            Component::Normal(part) => relative.push(part),
            _ => return None,
        }
    }
    Some(root.join(relative))
}

async fn runtime_entry(
    network: &impl Network,
    platform: &str,
    component: &str,
    major: u32,
) -> Result<Entry, RuntimeError> {
    network
        .fetch_with(RUNTIME_INDEX, FetchKind::UnverifiedMetadata, |response| {
            let bytes = response.map_err(|source| fetch_error(RUNTIME_INDEX, source))?;
            let mut index: Index =
                serde_json::from_slice(&bytes).map_err(|error| RuntimeError::Malformed {
                    reason: error.to_string(),
                })?;
            index
                .get_mut(platform)
                .and_then(|components| components.remove(component))
                .and_then(|entries| entries.into_iter().next())
                .ok_or(RuntimeError::Unsupported { major })
        })
        .await
}

fn fetch_error(url: &str, source: FetchError) -> RuntimeError {
    RuntimeError::Fetch {
        url: url.into(),
        source,
    }
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/install/src/runtime.rs"]
mod tests;
