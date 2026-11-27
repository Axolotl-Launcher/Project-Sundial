// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Install jobs: what the launcher is doing, and what it got through.
//!
//! One document per job under `jobs/`, so a job that outlives a restart is read
//! back from disk instead of lost with the process. The envelope is stable and
//! the internals are not:
//!
//! - `kind` and `status` are the envelope a UI lists;
//! - `state` is an opaque JSON value. A job's stages change as the launcher
//!   grows, and typing them here would make every such change a schema change;
//! - `items` are the per-file progress rows, nested because they are never read
//!   apart from their job.
//!
//! The reference keeps the same split, for the same reason.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use axlauncher_storage::write_atomic;
use serde::{Deserialize, Serialize};

use crate::document::{self, Existing};

/// The only job schema this build reads.
pub const JOBS_SCHEMA_VERSION: u32 = 1;

/// Where a job's status is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    /// Whether no further work will happen for this job.
    pub fn is_finished(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

/// One file a job is moving.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobItem {
    /// Stable within the job; what a resume matches against.
    pub id: String,
    /// The path the item lands at, relative to the game root.
    pub path: String,
    pub state: JobItemState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_bytes: Option<u64>,
    #[serde(default)]
    pub written_bytes: u64,
    /// The failure, when the item ended in one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Where one item is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobItemState {
    Pending,
    Running,
    Done,
    Failed,
    Skipped,
}

/// A job as the launcher knows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    /// What the job is for, in the launcher's vocabulary (`install`, `update`).
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    pub status: JobStatus,
    /// The job's internals, opaque here on purpose.
    #[serde(default)]
    pub state: serde_json::Value,
    /// Unix milliseconds.
    pub created: i64,
    pub modified: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished: Option<i64>,
    #[serde(default)]
    pub dismissed: bool,
    #[serde(default)]
    pub items: Vec<JobItem>,
}

impl Job {
    /// The items still to be worked on.
    pub fn outstanding(&self) -> impl Iterator<Item = &JobItem> {
        self.items
            .iter()
            .filter(|item| matches!(item.state, JobItemState::Pending | JobItemState::Running))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct JobDocument {
    schema_version: u32,
    job: Job,
}

#[derive(Debug)]
pub enum JobError {
    Read {
        path: PathBuf,
        source: io::Error,
    },
    Parse {
        path: PathBuf,
        source: serde_json::Error,
    },
    Write {
        path: PathBuf,
        source: io::Error,
    },
    RefusedSchema {
        path: PathBuf,
        found: u32,
    },
    RefusedUnreadable {
        path: PathBuf,
    },
    InvalidId {
        id: String,
    },
}

impl fmt::Display for JobError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(formatter, "cannot read {}: {source}", path.display())
            }
            Self::Parse { path, source } => {
                write!(formatter, "cannot parse {}: {source}", path.display())
            }
            Self::Write { path, source } => {
                write!(formatter, "cannot write {}: {source}", path.display())
            }
            Self::RefusedSchema { path, found } => write!(
                formatter,
                "{} uses schema {found}; this build reads schema {JOBS_SCHEMA_VERSION}",
                path.display()
            ),
            Self::RefusedUnreadable { path } => {
                write!(
                    formatter,
                    "{} is unreadable; refusing to replace it",
                    path.display()
                )
            }
            Self::InvalidId { id } => write!(formatter, "\"{id}\" is not a usable job id"),
        }
    }
}

impl std::error::Error for JobError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } | Self::Write { source, .. } => Some(source),
            Self::Parse { source, .. } => Some(source),
            Self::RefusedSchema { .. }
            | Self::RefusedUnreadable { .. }
            | Self::InvalidId { .. } => None,
        }
    }
}

/// The document's path for `id` under `jobs_root`.
///
/// The id becomes a file name, so it is restricted the way an instance id is:
/// a value that could escape the directory is rejected, not sanitised.
pub fn path(jobs_root: &Path, id: &str) -> Result<PathBuf, JobError> {
    let usable = !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
    if !usable {
        return Err(JobError::InvalidId { id: id.to_owned() });
    }
    Ok(axlauncher_storage::ManagedFile::JobDocument(id).path(jobs_root))
}

/// Reads the job `id` under `jobs_root`.
pub fn load(jobs_root: &Path, id: &str) -> Result<Option<Job>, JobError> {
    let path = path(jobs_root, id)?;
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(JobError::Read { path, source }),
    };

    let document: JobDocument =
        serde_json::from_slice(&bytes).map_err(|source| JobError::Parse {
            path: path.clone(),
            source,
        })?;
    if document.schema_version != JOBS_SCHEMA_VERSION {
        return Err(JobError::RefusedSchema {
            path,
            found: document.schema_version,
        });
    }
    Ok(Some(document.job))
}

/// Writes `job` under `jobs_root`, refusing to replace another schema's
/// document.
pub fn save(jobs_root: &Path, job: &Job) -> Result<(), JobError> {
    let path = path(jobs_root, &job.id)?;

    match document::inspect(&path).map_err(|source| JobError::Read {
        path: path.clone(),
        source,
    })? {
        Existing::Unreadable => return Err(JobError::RefusedUnreadable { path }),
        Existing::Schema(found) if found != JOBS_SCHEMA_VERSION => {
            return Err(JobError::RefusedSchema { path, found });
        }
        Existing::Missing | Existing::Schema(_) => {}
    }

    let document = JobDocument {
        schema_version: JOBS_SCHEMA_VERSION,
        job: job.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&document).map_err(|source| JobError::Write {
        path: path.clone(),
        source: io::Error::other(source),
    })?;
    write_atomic(&path, &bytes).map_err(|source| JobError::Write { path, source })
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/state/src/jobs.rs"]
mod tests;
