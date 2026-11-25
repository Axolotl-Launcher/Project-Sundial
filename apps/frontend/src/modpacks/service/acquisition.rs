// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::BTreeMap;
use std::io::{self, Seek};
use std::path::{Path, PathBuf};

use super::{PackState, install_runtime};
use axlauncher_core::download_engine::{DEFAULT_CONCURRENCY, DownloadEngine, DownloadJob};
use axlauncher_core::network::{ReqwestNetwork, verify_reader};
use axlauncher_core::spi::Network;
use axlauncher_core::state::profile::{Game, Profile, StagedProfile};
use axlauncher_core::storage::{AtomicFile, DataDir, open_for_read};
use axlauncher_modpack::{Download, InstallPlan};
use futures::{StreamExt, stream};

pub(super) struct StagedPack {
    plan: InstallPlan,
    staging: StagedProfile,
    remaining: Vec<usize>,
    manual: Vec<ManualFile>,
    ready: bool,
}

impl StagedPack {
    pub(super) fn new(plan: InstallPlan, data: &DataDir) -> Result<Self, String> {
        let remaining = (0..plan.downloads().len()).collect();
        Ok(Self {
            plan,
            staging: StagedProfile::new(&data.instances()).map_err(|error| error.to_string())?,
            remaining,
            manual: Vec::new(),
            ready: false,
        })
    }

    pub(super) fn is_ready(&self) -> bool {
        self.ready
    }

    pub(super) fn manual_files(&self) -> Vec<ManualFile> {
        self.manual.clone()
    }

    pub(super) fn prepare(
        &mut self,
        local: &BTreeMap<String, PathBuf>,
        concurrency: usize,
        report: &impl Fn(PackState),
    ) -> Result<(), String> {
        self.prepare_files(local, concurrency, report)
            .inspect_err(|_| {
                report(PackState::Failed);
            })
    }

    fn prepare_files(
        &mut self,
        local: &BTreeMap<String, PathBuf>,
        concurrency: usize,
        report: &impl Fn(PackState),
    ) -> Result<(), String> {
        if !self.manual.is_empty() {
            report(PackState::Supplying);
            self.manual = supply_manual_files(
                std::mem::take(&mut self.manual),
                &self.plan,
                self.staging.directory(),
                local,
            );
        }
        if !self.remaining.is_empty() {
            let network = ReqwestNetwork::desktop_with_download_policy(
                self.plan.redirect_policy(),
                self.plan.download_key(),
            )
            .map_err(|error| error.to_string())?;
            install_runtime()?.block_on(download_remaining(self, &network, concurrency, report))?;
        }
        if !self.manual.is_empty() {
            report(PackState::ManualFiles);
            return Ok(());
        }
        if !self.ready {
            report(PackState::Copying);
            self.write_bundled()?;
            self.ready = true;
        }
        report(PackState::Ready);
        Ok(())
    }

    fn write_bundled(&self) -> Result<(), String> {
        self.plan
            .write_bundled_files(|path, input| {
                let destination = self.staging.directory().join(path);
                let mut file = AtomicFile::new(&destination)?;
                io::copy(input, file.file())?;
                file.commit(&destination)
            })
            .map_err(|error| error.to_string())
    }

    pub(super) fn has_root_data_packs(&self) -> bool {
        self.plan.has_root_data_packs()
    }

    pub(super) fn publish(self, game: Game, version: String) -> Result<PathBuf, String> {
        let profile = Profile::new(self.plan.name(), Some(game));
        self.staging
            .publish_installed(profile, &version)
            .map_err(|error| error.to_string())
    }
}

async fn download_remaining(
    pack: &mut StagedPack,
    network: &impl Network,
    concurrency: usize,
    report: &impl Fn(PackState),
) -> Result<(), String> {
    let remaining = std::mem::take(&mut pack.remaining);
    let total = remaining.len();
    let limit = if concurrency == 0 {
        DEFAULT_CONCURRENCY
    } else {
        concurrency
    };
    let directory = pack.staging.directory();
    let files = pack.plan.downloads();
    let mut jobs = stream::iter(
        remaining
            .into_iter()
            .map(|index| async move { (index, acquire(network, directory, &files[index]).await) }),
    )
    .buffer_unordered(limit);
    let report_count = |processed| report(PackState::Downloading { processed, total });
    report_count(0);
    let mut processed = 0;
    let mut failure = None;
    while let Some((index, result)) = jobs.next().await {
        match result {
            Ok(file) => pack.manual.extend(file),
            Err(error) => {
                pack.remaining.push(index);
                failure.get_or_insert(error);
            }
        }
        processed += 1;
        if processed == total || processed.is_multiple_of((total / 100).max(1)) {
            report_count(processed);
        }
    }
    failure.map_or(Ok(()), Err)
}

#[derive(Clone)]
pub(in crate::modpacks) struct ManualFile {
    path: String,
    url: String,
    reason: Option<String>,
}

impl ManualFile {
    pub(in crate::modpacks) fn path(&self) -> &str {
        &self.path
    }
    pub(in crate::modpacks) fn url(&self) -> &str {
        &self.url
    }
    pub(in crate::modpacks) fn reason(&self) -> Option<&str> {
        self.reason.as_deref()
    }
}

async fn acquire(
    network: &impl Network,
    directory: &Path,
    file: &Download,
) -> Result<Option<ManualFile>, String> {
    let destination = directory.join(file.path());
    if file.urls().is_empty() {
        return offer_manual(file, None);
    }
    match download(network, &destination, file).await {
        Ok(()) => Ok(None),
        Err(error) => offer_manual(file, Some(error)),
    }
}

fn supply_manual_files(
    files: Vec<ManualFile>,
    plan: &InstallPlan,
    directory: &Path,
    local: &BTreeMap<String, PathBuf>,
) -> Vec<ManualFile> {
    files
        .into_iter()
        .filter_map(|file| supply_file(file, plan, directory, local))
        .collect()
}

fn supply_file(
    mut file: ManualFile,
    plan: &InstallPlan,
    directory: &Path,
    local: &BTreeMap<String, PathBuf>,
) -> Option<ManualFile> {
    let Some(source) = local.get(file.path()) else {
        return Some(file);
    };
    let download = plan
        .downloads()
        .iter()
        .find(|download| download.path() == file.path())
        .expect("pending files come from this plan");
    match copy_verified(source, &directory.join(file.path()), download) {
        Ok(()) => None,
        Err(error) => {
            file.reason = Some(error.to_string());
            Some(file)
        }
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
    let engine = DownloadEngine::shared();
    let job = DownloadJob {
        sources: &sources,
        destination,
        sha1: file.sha1(),
        sha1_alternatives: &[],
        size: file.size_hint(),
        batch: true,
    };
    if file.has_additional_digest() {
        let digests = file.clone();
        return engine
            .download_verified(network, job, move |input| digests.verify(input))
            .await;
    }
    engine.download(network, job).await
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

#[cfg(test)]
use zip as test_zip;

#[cfg(test)]
#[path = "../../../../../tests/apps/frontend/src/modpacks/service/acquisition.rs"]
mod tests;
