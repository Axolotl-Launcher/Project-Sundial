// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Service provider interfaces.
//!
//! A port is a trait a system implements and the rest of the program calls.
//! Consumers use the port; application wiring selects the concrete provider.
//! Desktop workflows wire their providers in `axlauncher_core::desktop`, so
//! swapping an implementation does not change the resource-owning systems.
//!
//! Traits here use return-position `impl Trait` rather than the `async_trait`
//! macro: the workspace's MSRV supports it, and it avoids a box per call.

pub mod auth;
pub mod desktop;
pub mod downloads;
pub mod mc_version;
mod sources;
mod transfer;

pub use axlauncher_domain::Id;

use std::path::{Path, PathBuf};
use std::time::Duration;

mod loader;
pub use loader::{
    LoaderError, LoaderFile, LoaderKind, LoaderProfile, LoaderRequest, LoaderVersion, ModLoader,
};
pub use sources::{DownloadMirror, FetchKind, ServiceHost};
pub use transfer::{
    ByteRange, DownloadBody, DownloadEtag, DownloadProtocol, DownloadRepresentation,
    DownloadResponse, RangeRequest, ResponseRange,
};

/// Fetches bytes over the network.
pub trait Network {
    /// Resolves a URL's body. The error is the provider's own type, boxed so
    /// callers need not name it.
    fn fetch(&self, url: &str) -> impl Future<Output = Result<Vec<u8>, FetchError>>;

    /// Fetches a resource and lets the caller accept or reject each candidate response.
    fn fetch_with<T, E>(
        &self,
        url: &str,
        _kind: FetchKind,
        accept: impl Fn(Result<Vec<u8>, FetchError>) -> Result<T, E>,
    ) -> impl Future<Output = Result<T, E>> {
        async move { accept(self.fetch(url).await) }
    }

    /// Supplies a mirror for a file transfer with the given integrity guarantee.
    fn download_mirror(&self, _url: &str, _has_digest: bool) -> Option<DownloadMirror> {
        None
    }

    /// Caps active file requests to a host, including requests using byte ranges.
    fn download_connection_limit(&self, _host: &str) -> std::num::NonZeroUsize {
        std::num::NonZeroUsize::MAX
    }

    /// Opens a file transfer. Providers that only fetch bodies may return a full response.
    fn open_download(
        &self,
        url: &str,
        _protocol: DownloadProtocol,
        _range: Option<RangeRequest<'_>>,
    ) -> impl Future<Output = Result<DownloadResponse<impl DownloadBody>, FetchError>> {
        async move {
            let body = bytes::Bytes::from(self.fetch(url).await?);
            Ok(DownloadResponse {
                length: Some(body.len() as u64),
                range: None,
                representation: None,
                body: Some(body),
            })
        }
    }
}

/// Why a fetch failed, reduced to what a caller can act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// Worth retrying: a timeout, a 5xx, a dropped connection.
    Retryable(String),
    /// A temporary HTTP failure with an optional server-supplied minimum delay.
    HttpRetryable {
        status: u16,
        retry_after: Option<Duration>,
    },
    /// Not worth retrying: a malformed URL, no transport configured.
    Fatal(String),
    /// The server rejected the request and may have supplied an explanation.
    ///
    /// The body is kept because an API can put meaning in it: a version that
    /// does not exist and a request the server rejected can both arrive as a
    /// 4xx, and only the body tells them apart. Dropping it would leave a
    /// caller unable to say which happened.
    Rejected {
        /// The HTTP status the server answered with.
        status: u16,
        body: Vec<u8>,
    },
}

impl std::fmt::Display for FetchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Retryable(reason) => write!(formatter, "retryable: {reason}"),
            Self::HttpRetryable { status, .. } => write!(formatter, "HTTP {status}"),
            Self::Fatal(reason) => formatter.write_str(reason),
            Self::Rejected { status, body } if body.is_empty() => {
                write!(formatter, "HTTP {status}")
            }
            Self::Rejected { status, body } => {
                write!(formatter, "HTTP {status}: {}", body_excerpt(body))
            }
        }
    }
}

impl std::error::Error for FetchError {}

/// How much of a rejected body a message quotes.
const BODY_EXCERPT_LEN: usize = 200;

/// The start of a rejected body, made printable, so a message stays a message.
fn body_excerpt(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(body);
    let trimmed = text.trim();
    let mut excerpt: String = trimmed.chars().take(BODY_EXCERPT_LEN).collect();
    if trimmed.chars().count() > BODY_EXCERPT_LEN {
        excerpt.push('…');
    }
    excerpt
}

/// Reads and writes bytes under the launcher's data directory.
pub trait Storage {
    fn read(&self, path: &str) -> Result<Vec<u8>, StorageError>;
    fn write(&self, path: &str, bytes: &[u8]) -> Result<(), StorageError>;
}

/// Why a storage operation failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorageError {
    /// The path does not exist.
    NotFound(String),
    /// The path exists but the operation was refused.
    Denied(String),
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(reason) => write!(formatter, "not found: {reason}"),
            Self::Denied(reason) => write!(formatter, "denied: {reason}"),
        }
    }
}

impl std::error::Error for StorageError {}

/// Offers content a user can install.
pub trait ContentProvider {
    /// The items this provider lists, as the launcher would show them.
    fn list(&self) -> impl Future<Output = Result<Vec<ContentSummary>, FetchError>>;
}

/// What a provider knows about one item before it is installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentSummary {
    pub id: Id,
    pub name: String,
}

/// Carries an install plan to the code that writes it to disk.
pub trait Installer {
    fn install(&self, plan: InstallPlan) -> impl Future<Output = Result<(), InstallError>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPlan {
    pub instance: Id,
    pub steps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    /// A step failed; the plan can be retried from the start.
    StepFailed {
        /// Which step failed.
        step: String,
        /// What it reported.
        reason: String,
    },
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StepFailed { step, reason } => {
                write!(formatter, "step \"{step}\" failed: {reason}")
            }
        }
    }
}

impl std::error::Error for InstallError {}

/// Starts a prepared process and reports when it exits.
pub trait Launcher {
    fn launch(&self, command: LaunchCommand) -> impl Future<Output = Result<(), LaunchError>>;

    /// The console output of this launch may contain the supplied session token.
    fn launch_redacting(
        &self,
        command: LaunchCommand,
        _token: Option<&str>,
    ) -> impl Future<Output = Result<(), LaunchError>> {
        self.launch(command)
    }
}

/// Which of the platform's Java executables to run.
///
/// Windows ships two: `javaw.exe` opens no console window, which is what the
/// game wants, while `java.exe` keeps stdout available for a probe or helper
/// whose output is read. Elsewhere they are the same binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JavaMode {
    /// No console window; for the game itself.
    Windowed,
    /// Console attached; for probes and helpers whose output is read.
    Console,
}

/// Locates the Java runtime the game will run on.
///
/// Discovery differs per platform — the variable names and search locations are
/// not shared — so callers ask this port instead of inspecting the environment
/// themselves.
pub trait Java {
    /// Resolves the executable to run.
    ///
    /// `preferred` is the user's configured path and wins when it exists.
    /// Otherwise the provider falls back to what this platform offers, and
    /// `None` means nothing was found. The result suits `mode`.
    fn executable(&self, preferred: Option<&Path>, mode: JavaMode) -> Option<PathBuf>;

    /// Resolves an executable whose runtime reports `major`.
    ///
    /// A version document states the major version its classes need. Returning
    /// a runtime of a different major starts the game on a JVM that cannot load
    /// it, and the failure surfaces as a class-file error instead of a message
    /// about Java. A provider answers `None` when it cannot find a matching
    /// runtime, so the caller can say which version is missing rather than
    /// starting the game on the wrong one.
    fn executable_for(&self, major: u32, mode: JavaMode) -> Option<PathBuf>;
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct LaunchCommand {
    pub program: String,
    pub args: Vec<String>,
    /// The directory the game's working files are resolved against.
    pub working_directory: PathBuf,
    /// Variables added to, or removed from, the inherited environment. A `None`
    /// value removes the name.
    pub environment: Vec<(String, Option<String>)>,
}

impl std::fmt::Debug for LaunchCommand {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LaunchCommand")
            .field("program", &self.program)
            .field("argument_count", &self.args.len())
            .field("working_directory", &self.working_directory)
            .finish_non_exhaustive()
    }
}

/// Why a launch did not reach a successful exit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchError {
    /// The program could not be started, with the reason the platform gave.
    NotFound(String),
    /// The game ran and exited non-zero; `None` means it was killed.
    Exited { code: Option<i32> },
    /// The child exited after console output was captured to a launch-specific file.
    ExitedWithLog { code: Option<i32>, log: PathBuf },
    /// The process ran, but its console output could not be fully captured.
    ConsoleOutput(String),
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(reason) => write!(formatter, "cannot start the program: {reason}"),
            Self::Exited { code: Some(code) } => write!(formatter, "exited with code {code}"),
            Self::Exited { code: None } => write!(formatter, "was terminated without an exit code"),
            Self::ExitedWithLog { code, log } => write!(
                formatter,
                "{}. See {}",
                Self::Exited { code: *code },
                log.display()
            ),
            Self::ConsoleOutput(reason) => {
                write!(formatter, "could not capture console output: {reason}")
            }
        }
    }
}

impl std::error::Error for LaunchError {}

/// Persists and loads application state.
///
/// Saving takes `&mut self`: a store that silently discards a write is worse
/// than one that cannot be written to at all.
pub trait StateStore {
    fn load_instance(&self, id: Id) -> Result<Option<InstanceRecord>, StorageError>;
    fn save_instance(&mut self, instance: &InstanceRecord) -> Result<(), StorageError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceRecord {
    pub id: Id,
    pub name: String,
}

/// Records what the launcher did.
pub trait Telemetry {
    fn record(&self, event: Event);
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub name: String,
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/spi/src/lib.rs"]
mod tests;
