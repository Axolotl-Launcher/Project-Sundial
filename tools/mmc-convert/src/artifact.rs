// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::{
    io,
    path::{Path, PathBuf},
};

use axlauncher_core::launch::version::maven_path;
use axlauncher_core::storage::AtomicFile;
use axlauncher_modpack::MultiMcExport;
use serde::Deserialize;

use crate::Result;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LocalArtifact {
    name: String,
    #[serde(rename = "MMC-hint")]
    hint: String,
    #[serde(rename = "MMC-filename")]
    filename: Option<String>,
}

impl LocalArtifact {
    pub(super) fn name(&self) -> &str {
        &self.name
    }

    pub(super) fn maven_path(&self) -> Result<PathBuf> {
        if !self
            .name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b":._+-@".contains(&byte))
            || self.name.split([':', '@']).any(|part| part.ends_with('.'))
        {
            return Err(format!("invalid MultiMC artifact coordinate {}", self.name).into());
        }
        maven_path(&self.name)
            .ok_or_else(|| format!("invalid MultiMC artifact coordinate {}", self.name).into())
    }

    pub(super) fn copy(
        &self,
        export: &mut MultiMcExport,
        directory: &str,
        destination: &Path,
    ) -> Result<u64> {
        if self.hint != "local" {
            return Err(format!("MultiMC artifact {} must be bundled locally", self.name).into());
        }
        let layout = self.maven_path()?;
        let basename = layout
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .ok_or("artifact has no file name")?;
        let basename = self.filename.as_deref().unwrap_or(basename);
        if basename.contains('/') {
            return Err(format!("invalid MultiMC artifact name {basename:?}").into());
        }
        let mut staged = AtomicFile::new(destination)?;
        let path = format!("{directory}/{basename}");
        let mut size = 0;
        export.stream_artifact(&path, |input| {
            size = io::copy(input, staged.file())?;
            Ok(())
        })?;
        staged.commit(destination)?;
        Ok(size)
    }
}
