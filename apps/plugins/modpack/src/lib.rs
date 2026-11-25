// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Read-only inspection and planning for modpack distribution inputs.

#![forbid(unsafe_code)]

mod archive;
mod curseforge;
mod modrinth;
mod multimc;
mod paths;
pub use multimc::MultiMcExport;

use std::collections::BTreeSet;
use std::io::{self, BufRead, BufReader, Read};
use std::path::Path;
use std::sync::{Arc, Mutex};

use axlauncher_spi::LoaderKind;
use axlauncher_spi::auth::AuthNetwork;
use md5::{Digest as _, Md5};
use sha2::{Digest, Sha512};

use archive::{Archive, Entry};

/// A built-in, explicitly registered set of archive readers.
#[derive(Clone, Copy)]
pub struct ModpackPlugin {
    readers: &'static [&'static dyn PackReader],
}

trait PackReader: Sync {
    fn find_manifest(&self, archive: &Archive) -> Result<Option<ManifestLocation>, PackError>;
    fn read(&self, archive: Archive, manifest: ManifestLocation) -> Result<Pack, PackError>;
}

struct ManifestLocation {
    path: String,
    entry: Entry,
}

impl ModpackPlugin {
    /// Registers the archive formats shipped with this build.
    pub const fn builtin() -> Self {
        Self {
            readers: &[&modrinth::Reader, &curseforge::Reader, &multimc::Reader],
        }
    }

    /// Opens the selected file and retains the archive used for inspection.
    pub fn inspect(&self, path: &Path) -> Result<Pack, PackError> {
        let archive = Archive::open(path)?;
        let mut candidates = self.readers.iter().filter_map(|reader| {
            reader
                .find_manifest(&archive)
                .transpose()
                .map(|found| found.map(|location| (*reader, location)))
        });
        let (reader, manifest) = candidates
            .next()
            .transpose()?
            .ok_or(PackError::UnknownFormat)?;
        if candidates.next().transpose()?.is_some() {
            return Err(PackError::AmbiguousFormat);
        }
        reader.read(archive, manifest)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PackError {
    #[error("The archive does not contain a supported modpack manifest.")]
    UnknownFormat,
    #[error("The archive contains more than one supported modpack manifest.")]
    AmbiguousFormat,
    #[error("Unsupported modpack: {0}")]
    Unsupported(String),
    #[error("Invalid modpack: {0}")]
    Invalid(String),
    #[error(
        "CurseForge API key missing. Set CURSEFORGE_API_KEY in the workspace .env and rebuild, or set AXLAUNCHER_CURSEFORGE_API_KEY before starting the launcher."
    )]
    MissingCurseForgeKey,
    #[error("CurseForge metadata could not be resolved: {0}")]
    CurseForge(String),
    #[error("Could not read the modpack: {0}")]
    Io(#[from] io::Error),
    #[error("Could not read ZIP: {0}")]
    Zip(#[from] zip::result::ZipError),
}

/// The format identified by a registered archive reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PackSource {
    Modrinth,
    CurseForge,
    MultiMc,
}

impl PackSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Modrinth => "Modrinth",
            Self::CurseForge => "CurseForge",
            Self::MultiMc => "MultiMC",
        }
    }
}

/// Identity of an optional item within one inspected pack.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct OptionId(String);

impl OptionId {
    /// A stable key for presentation. Its contents are owned by the reader.
    pub fn key(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for OptionId {
    fn borrow(&self) -> &str {
        self.key()
    }
}

/// A selectable file and its user-facing name.
pub struct OptionalFile {
    id: OptionId,
    label: String,
}

impl OptionalFile {
    pub fn id(&self) -> &OptionId {
        &self.id
    }

    pub fn label(&self) -> &str {
        &self.label
    }
}

/// The exact game and loader builds required to play a pack.
#[derive(Clone)]
pub struct GameRequirements {
    minecraft: String,
    loader: Option<(LoaderKind, String)>,
}

impl GameRequirements {
    pub fn minecraft(&self) -> &str {
        &self.minecraft
    }

    pub fn loader(&self) -> Option<(LoaderKind, &str)> {
        self.loader
            .as_ref()
            .map(|(kind, version)| (*kind, version.as_str()))
    }
}

/// An inspected client pack. The reader retains format-specific file data.
pub struct Pack {
    source: PackSource,
    name: String,
    version: String,
    game: GameRequirements,
    required_count: usize,
    optional: Vec<OptionalFile>,
    files: PackFiles,
    bundled: BundledFiles,
}

enum PackFiles {
    Modrinth(modrinth::Files),
    CurseForge(curseforge::Files),
    Bundled,
}

/// Controls how the planner acquires files with no public download URL.
#[derive(Clone, Copy)]
pub struct PlanOptions {
    auto_download_restricted: bool,
}

impl Default for PlanOptions {
    fn default() -> Self {
        Self {
            auto_download_restricted: true,
        }
    }
}

impl PlanOptions {
    pub fn auto_download_restricted(self) -> bool {
        self.auto_download_restricted
    }

    pub fn with_auto_download_restricted(mut self, enabled: bool) -> Self {
        self.auto_download_restricted = enabled;
        self
    }
}

type RedirectPolicy = fn(&url::Url) -> bool;

struct PlannedDownloads {
    files: Vec<Download>,
    redirect_policy: RedirectPolicy,
    api_key: Option<String>,
}

impl Pack {
    pub fn source(&self) -> PackSource {
        self.source
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn game(&self) -> &GameRequirements {
        &self.game
    }

    pub fn required_count(&self) -> usize {
        self.required_count
    }

    pub fn optional_files(&self) -> &[OptionalFile] {
        &self.optional
    }

    pub fn bundled_count(&self) -> usize {
        self.bundled.entries.len()
    }

    /// A plan contains every required file and the optional items chosen in this preview.
    pub async fn plan(
        &self,
        selected: &BTreeSet<OptionId>,
        options: PlanOptions,
        network: &impl AuthNetwork,
    ) -> Result<InstallPlan, PackError> {
        let mut downloads = match &self.files {
            PackFiles::Modrinth(files) => files.plan(selected),
            PackFiles::CurseForge(files) => files.plan(selected, options, network).await?,
            PackFiles::Bundled => PlannedDownloads {
                files: Vec::new(),
                redirect_policy: |_| false,
                api_key: None,
            },
        };
        paths::check_tree(downloads.files.iter().map(Download::path))?;
        downloads.files.retain(|file| {
            self.bundled
                .entries
                .binary_search_by(|(path, _)| path.as_str().cmp(file.path()))
                .is_err()
        });
        paths::check_tree(
            downloads
                .files
                .iter()
                .map(Download::path)
                .chain(self.bundled.entries.iter().map(|(path, _)| path.as_str())),
        )?;
        Ok(InstallPlan {
            name: self.name.clone(),
            game: self.game.clone(),
            bundled: self.bundled.clone(),
            downloads,
        })
    }
}

/// The common input to installation, backed by the inspected archive.
pub struct InstallPlan {
    name: String,
    game: GameRequirements,
    bundled: BundledFiles,
    downloads: PlannedDownloads,
}

#[derive(Clone)]
struct BundledFiles {
    entries: Arc<[(String, Entry)]>,
    archive: Arc<Mutex<Archive>>,
}

impl BundledFiles {
    fn new(entries: Vec<(String, Entry)>, archive: Archive) -> Self {
        Self {
            entries: entries.into(),
            archive: Arc::new(Mutex::new(archive)),
        }
    }
}

impl InstallPlan {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn game(&self) -> &GameRequirements {
        &self.game
    }

    pub fn downloads(&self) -> &[Download] {
        &self.downloads.files
    }

    /// Data packs stored at the game root may still need to be enabled in a world.
    pub fn has_root_data_packs(&self) -> bool {
        self.downloads
            .files
            .iter()
            .any(|file| file.path().starts_with("datapacks/"))
            || self
                .bundled
                .entries
                .iter()
                .any(|(path, _)| path.starts_with("datapacks/"))
    }

    /// A transport can apply the reader's redirect policy without inspecting the source.
    pub fn redirect_policy(&self) -> RedirectPolicy {
        self.downloads.redirect_policy
    }

    /// The transfer client may send this credential only to the allowed origins.
    pub fn download_api_key(&self) -> Option<&str> {
        self.downloads.api_key.as_deref()
    }

    /// Bundled payloads stream through a host-supplied writer at their final relative paths.
    pub fn write_bundled_files(
        &self,
        mut write: impl FnMut(&str, &mut dyn Read) -> io::Result<()>,
    ) -> Result<(), PackError> {
        let mut archive = self
            .bundled
            .archive
            .lock()
            .map_err(|_| io::Error::other("archive lock poisoned"))?;
        for (path, entry) in &*self.bundled.entries {
            archive.stream(*entry, |input| write(path, input))?;
        }
        Ok(())
    }
}

/// One pinned client file with alternate sources and required integrity checks.
#[derive(Clone)]
pub struct Download {
    path: String,
    urls: Vec<String>,
    size_hint: Option<u64>,
    sha1: Option<String>,
    sha512: Option<[u8; 64]>,
    md5: Option<[u8; 16]>,
    manual_url: Option<String>,
}

impl Download {
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn urls(&self) -> &[String] {
        &self.urls
    }

    /// The manifest's estimate, used to schedule transfers.
    pub fn size_hint(&self) -> Option<u64> {
        self.size_hint
    }

    pub fn sha1(&self) -> Option<&str> {
        self.sha1.as_deref()
    }

    /// An official page where a user can obtain an unavailable download.
    pub fn manual_url(&self) -> Option<&str> {
        self.manual_url.as_deref()
    }

    /// Extra digests complement the transfer engine's SHA-1 check.
    pub fn verify(&self, input: impl Read) -> io::Result<()> {
        if let Some(expected) = self.sha512 {
            let mut digest = Sha512::new();
            hash(input, |bytes| digest.update(bytes))?;
            return check_hash(digest.finalize().as_slice() == expected, "SHA-512");
        }
        if let Some(expected) = self.md5 {
            let mut digest = Md5::new();
            hash(input, |bytes| digest.update(bytes))?;
            return check_hash(digest.finalize().as_slice() == expected, "MD5");
        }
        Ok(())
    }
}

fn check_hash(matches: bool, algorithm: &str) -> io::Result<()> {
    if matches {
        return Ok(());
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{algorithm} mismatch"),
    ))
}

fn hash(input: impl Read, mut update: impl FnMut(&[u8])) -> io::Result<()> {
    let mut input = BufReader::new(input);
    loop {
        let bytes = input.fill_buf()?;
        if bytes.is_empty() {
            return Ok(());
        }
        let count = bytes.len();
        update(bytes);
        input.consume(count);
    }
}

// Cargo Shear scans imports inside the crate but misses the mirrored tests.
#[cfg(test)]
use futures as test_futures;
#[cfg(test)]
use tempfile as test_tempfile;

#[cfg(test)]
#[path = "../../../../tests/apps/plugins/modpack/src/lib.rs"]
mod tests;
