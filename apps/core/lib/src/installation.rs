//! The installation worker receives only the paths and public preferences it needs.

use std::path::PathBuf;

use axlauncher_install::prepare::Progress;
use axlauncher_network::MirrorSettings;
use axlauncher_spi::LoaderKind;
use axlauncher_storage::{DataDir, DataPath, Directory};
use serde::{Deserialize, Serialize};

use crate::downloads::File;

#[derive(Deserialize, Serialize)]
pub struct Target {
    game_version: String,
    loader: Option<(LoaderKind, Option<String>)>,
}

impl Target {
    pub fn new(game_version: String, loader: Option<(LoaderKind, Option<String>)>) -> Self {
        Self {
            game_version,
            loader,
        }
    }

    pub fn game_version(&self) -> &str {
        &self.game_version
    }

    pub fn loader(&self) -> Option<(LoaderKind, Option<&str>)> {
        self.loader
            .as_ref()
            .map(|(kind, build)| (*kind, build.as_deref()))
    }

    fn accepts(&self, version: &str) -> bool {
        axlauncher_domain::is_version_id(version)
            && (self.loader.is_some() || version == self.game_version)
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub struct NativePath(#[serde(with = "crate::path")] PathBuf);

impl From<PathBuf> for NativePath {
    fn from(path: PathBuf) -> Self {
        Self(path)
    }
}

impl From<NativePath> for PathBuf {
    fn from(path: NativePath) -> Self {
        path.0
    }
}

#[derive(Deserialize, Serialize)]
pub struct JavaPaths {
    registered: Vec<NativePath>,
    preferred: Vec<(u32, NativePath)>,
}

impl JavaPaths {
    pub fn new(registered: Vec<PathBuf>, preferred: Vec<(u32, PathBuf)>) -> Self {
        Self {
            registered: registered.into_iter().map(Into::into).collect(),
            preferred: preferred
                .into_iter()
                .map(|(major, path)| (major, path.into()))
                .collect(),
        }
    }

    pub fn into_paths(self) -> (Vec<PathBuf>, Vec<(u32, PathBuf)>) {
        (
            self.registered.into_iter().map(Into::into).collect(),
            self.preferred
                .into_iter()
                .map(|(major, path)| (major, path.into()))
                .collect(),
        )
    }
}

#[derive(Deserialize, Serialize)]
pub struct Command {
    data_root: NativePath,
    cache: NativePath,
    target: Target,
    mirrors: MirrorSettings,
    concurrency: usize,
    java: JavaPaths,
}

impl Command {
    pub fn new(
        data: &DataDir,
        target: Target,
        mirrors: MirrorSettings,
        concurrency: usize,
        java: JavaPaths,
    ) -> Self {
        Self {
            data_root: data.path(DataPath::Root).to_owned().into(),
            cache: data.path(DataPath::Cache).to_owned().into(),
            target,
            mirrors,
            concurrency,
            java,
        }
    }

    pub fn into_install(
        self,
    ) -> std::io::Result<(DataDir, Target, MirrorSettings, usize, JavaPaths)> {
        let mut data = DataDir::isolated(&self.data_root.0)?;
        data.set_directory(Directory::AppCache, Some(&self.cache.0))?;
        Ok((data, self.target, self.mirrors, self.concurrency, self.java))
    }

    pub(crate) fn uses_data(&self, data: &DataDir) -> bool {
        self.data_root.0 == data.path(DataPath::Root) && self.cache.0 == data.path(DataPath::Cache)
    }

    pub(crate) fn accepts(&self, version: &str) -> bool {
        self.target.accepts(version)
    }
}

pub use crate::installer_client::install;

#[derive(Deserialize, Serialize)]
pub enum Reply {
    Progress(Progress),
    Download {
        callback: u64,
        file: File,
    },
    RegisterJava {
        callback: u64,
        major: u32,
        executable: NativePath,
    },
    Resolved(String),
}
