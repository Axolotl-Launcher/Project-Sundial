// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::io::{self, Seek};
use std::path::Path;

use axlauncher_core::download_engine::{DownloadEngine, DownloadJob};
use axlauncher_core::network::verify_reader;
use axlauncher_core::spi::Network;
use axlauncher_core::storage::{AtomicFile, open_for_read};
use axlauncher_modpack::Download;

pub(super) struct ManualFile {
    path: String,
    url: String,
    reason: Option<String>,
}

impl ManualFile {
    pub(super) fn path(&self) -> &str {
        &self.path
    }
    pub(super) fn url(&self) -> &str {
        &self.url
    }
    pub(super) fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
}

pub(super) async fn acquire(
    network: &impl Network,
    directory: &Path,
    file: &Download,
    local: Option<&Path>,
) -> Result<Option<ManualFile>, String> {
    let destination = directory.join(file.path());
    let result = match local {
        Some(local) if file.manual_url().is_some() => {
            copy_verified(local, &destination, file).map_err(|error| error.to_string())
        }
        _ if file.urls().is_empty() => return offer_manual(file, None),
        _ => download(network, &destination, file).await,
    };
    match result {
        Ok(()) => Ok(None),
        Err(error) => offer_manual(file, Some(error)),
    }
}

fn offer_manual(file: &Download, reason: Option<String>) -> Result<Option<ManualFile>, String> {
    let Some(url) = file.manual_url() else {
        return Err(format!(
            "{}: {}",
            file.path(),
            reason.as_deref().unwrap_or("no download source")
        ));
    };
    Ok(Some(ManualFile {
        path: file.path().into(),
        url: url.into(),
        reason,
    }))
}

async fn download(
    network: &impl Network,
    destination: &Path,
    file: &Download,
) -> Result<(), String> {
    let sources: Vec<_> = file.urls().iter().map(String::as_str).collect();
    DownloadEngine::shared()
        .download(
            network,
            DownloadJob {
                sources: &sources,
                destination,
                sha1: file.sha1(),
                sha1_alternatives: &[],
                size: file.size_hint(),
                batch: true,
            },
        )
        .await?;
    let input = open_for_read(destination).map_err(|error| error.to_string())?;
    file.verify(input).map_err(|error| error.to_string())
}

fn copy_verified(source: &Path, destination: &Path, download: &Download) -> io::Result<()> {
    let mut source = open_for_read(source)?;
    if !source.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "select a regular file",
        ));
    }
    let mut staged = AtomicFile::new(destination)?;
    io::copy(&mut source, staged.file())?;
    let file = staged.rewind()?;
    if let Some(expected) = download.sha1()
        && !verify_reader(&mut *file, expected)?
    {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "SHA-1 mismatch"));
    }
    file.rewind()?;
    download.verify(file)?;
    staged.commit(destination)
}
