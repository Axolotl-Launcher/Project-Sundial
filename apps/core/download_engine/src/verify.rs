// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::fs::File;
use std::io::{self, Seek};
use std::path::Path;
use std::sync::Arc;

use axlauncher_network::verify_reader_any;
use axlauncher_storage::AtomicFile;

use crate::DownloadJob;
use crate::transfer::TransferError;

pub(crate) type Verification = Arc<dyn Fn(&mut File) -> io::Result<()> + Send + Sync>;

pub(crate) fn has_digest(job: DownloadJob<'_>, verify: Option<&Verification>) -> bool {
    job.sha1.is_some() || !job.sha1_alternatives.is_empty() || verify.is_some()
}

pub(crate) async fn already_verified(
    job: DownloadJob<'_>,
    check: Option<&Verification>,
) -> Result<bool, TransferError> {
    if !has_digest(job, check) {
        return Ok(false);
    }
    let file = match axlauncher_storage::open_for_read(job.destination) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    match verify_file(file, job, check).await {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::InvalidData => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub(crate) async fn verify(
    staging: &mut AtomicFile,
    job: DownloadJob<'_>,
    check: Option<&Verification>,
) -> Result<(), TransferError> {
    if has_digest(job, check) {
        let file = staging.rewind()?.try_clone()?;
        return verify_file(file, job, check)
            .await
            .map_err(|error| verification_error(error, job.destination));
    }
    let length = staging.file().metadata()?.len();
    if job.size.is_some_and(|expected| expected != length) {
        return Err(TransferError::IntegrityMismatch(format!(
            "size mismatch for {}",
            job.destination.display()
        )));
    }
    Ok(())
}

fn verification_error(error: io::Error, destination: &Path) -> TransferError {
    if error.kind() == io::ErrorKind::InvalidData {
        return TransferError::IntegrityMismatch(format!("{error} for {}", destination.display()));
    }
    TransferError::Disk(error)
}

async fn verify_file(
    file: File,
    job: DownloadJob<'_>,
    check: Option<&Verification>,
) -> io::Result<()> {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return check_file(file, job.sha1, job.sha1_alternatives, check);
    };
    let sha1 = job.sha1.map(str::to_owned);
    let alternatives = job.sha1_alternatives.to_vec();
    let check = check.cloned();
    runtime
        .spawn_blocking(move || check_file(file, sha1.as_deref(), &alternatives, check.as_ref()))
        .await
        .map_err(io::Error::other)?
}

fn check_file(
    mut file: File,
    sha1: Option<&str>,
    alternatives: &[String],
    check: Option<&Verification>,
) -> io::Result<()> {
    if (sha1.is_some() || !alternatives.is_empty())
        && !verify_reader_any(&mut file, sha1, alternatives)?
    {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "SHA-1 mismatch"));
    }
    if let Some(check) = check {
        file.rewind()?;
        check(&mut file)?;
    }
    Ok(())
}
