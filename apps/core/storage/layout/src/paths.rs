use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::game_dir::GameDir;

const APP_DIRECTORY: &str = "AXLauncher";
const DATA_DIR_ENV: &str = "AXLAUNCHER_DATA_DIR";

#[derive(Clone, Copy)]
pub enum WorkerLibrary {
    Downloads,
    Accounts,
    JavaDiscovery,
    InstanceSettings,
    Install,
}

impl WorkerLibrary {
    pub fn name(self) -> &'static str {
        match self {
            Self::Downloads => "downloads",
            Self::Accounts => "accounts",
            Self::JavaDiscovery => "java-discovery",
            Self::InstanceSettings => "instance-settings",
            Self::Install => "install",
        }
    }

    pub fn path(self, scratch: &Path) -> PathBuf {
        scratch.join(
            std::env::consts::DLL_PREFIX.to_owned() + self.name() + std::env::consts::DLL_SUFFIX,
        )
    }
}

/// Locations that can be overridden for the current launcher session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Directory {
    AppData,
    AppCache,
    UserData,
    AppTemp,
}

/// A fixed location resolved against the active data-directory snapshot.
#[derive(Clone, Copy)]
pub enum DataPath {
    Root,
    Cache,
    Profiles,
    Temp,
    JavaVersions,
    DownloadProbe,
    AuthAgent,
}

/// Selects the root for a private, automatically removed directory.
#[derive(Clone, Copy)]
pub enum ScratchSpace {
    Task,
    Worker,
}

/// Resolved launcher directories that a worker can keep for its entire operation.
#[derive(Debug, Clone)]
pub struct DataDir {
    config: Arc<DirectoryConfig>,
}

#[derive(Debug, Clone)]
struct DirectoryConfig {
    defaults: Defaults,
    overrides: Overrides,
    paths: ResolvedPaths,
}

#[derive(Debug, Clone)]
struct Defaults {
    data: PathBuf,
    cache: Option<PathBuf>,
    temp: Option<PathBuf>,
}

#[derive(Debug, Clone, Default)]
struct Overrides {
    data: Option<PathBuf>,
    cache: Option<PathBuf>,
    instances: Option<PathBuf>,
    temp: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct ResolvedPaths {
    data: PathBuf,
    caches: PathBuf,
    instances: PathBuf,
    temp: PathBuf,
    game: GameDir,
    java_versions: PathBuf,
    download_debug_file: PathBuf,
    auth_agent: PathBuf,
}

impl ResolvedPaths {
    fn new(defaults: &Defaults, overrides: &Overrides) -> Self {
        let data = overrides.data.as_ref().unwrap_or(&defaults.data).clone();
        let caches = overrides
            .cache
            .as_ref()
            .or(defaults.cache.as_ref())
            .cloned()
            .unwrap_or_else(|| data.join("caches"));
        let temp = overrides
            .temp
            .as_ref()
            .or(defaults.temp.as_ref())
            .cloned()
            .unwrap_or_else(|| data.join("tmp"));
        let instances = overrides
            .instances
            .clone()
            .unwrap_or_else(|| data.join("profiles"));
        let game = GameDir::shared(data.join("meta"), caches.join("minecraft-natives"));
        Self {
            java_versions: data.join("java"),
            download_debug_file: caches.join("download-debug.bin"),
            auth_agent: game.root().join("authlib-injector-1.2.8.jar"),
            data,
            caches,
            instances,
            temp,
            game,
        }
    }
}

impl DataDir {
    /// The local configuration location for device-user bindings.
    pub fn device_user_config() -> Option<PathBuf> {
        dirs::config_local_dir().map(|config| config.join(APP_DIRECTORY).join("device-users"))
    }

    /// Startup uses an isolated root when set; otherwise it uses platform directories.
    pub fn from_environment() -> io::Result<Self> {
        Self::resolve(
            std::env::var_os(DATA_DIR_ENV),
            dirs::data_dir,
            dirs::cache_dir,
            std::env::temp_dir,
        )
    }

    fn resolve(
        root: Option<OsString>,
        user_data: impl FnOnce() -> Option<PathBuf>,
        user_cache: impl FnOnce() -> Option<PathBuf>,
        user_temp: impl FnOnce() -> PathBuf,
    ) -> io::Result<Self> {
        if let Some(root) = root.filter(|root| !root.is_empty()) {
            return Self::isolated(root);
        }
        let data = user_data().ok_or_else(|| missing_directory("data"))?;
        let cache = user_cache().ok_or_else(|| missing_directory("cache"))?;
        let defaults = Defaults {
            data: absolute_directory(&data.join(APP_DIRECTORY))?,
            cache: Some(absolute_directory(&cache.join(APP_DIRECTORY))?),
            temp: Some(absolute_directory(&user_temp())?),
        };
        Ok(Self::new(defaults))
    }

    /// Explicit roots keep data, cache, instances, and scratch files together.
    pub fn isolated(root: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self::new(Defaults {
            data: absolute_directory(root.as_ref())?,
            cache: None,
            temp: None,
        }))
    }

    fn new(defaults: Defaults) -> Self {
        let overrides = Overrides::default();
        let paths = ResolvedPaths::new(&defaults, &overrides);
        Self {
            config: Arc::new(DirectoryConfig {
                defaults,
                overrides,
                paths,
            }),
        }
    }

    /// `None` restores the default. Invalid input leaves the snapshot unchanged.
    pub fn set_directory(&mut self, directory: Directory, path: Option<&Path>) -> io::Result<()> {
        let path = path.map(absolute_directory).transpose()?;
        let config = Arc::make_mut(&mut self.config);
        match directory {
            Directory::AppData => config.overrides.data = path,
            Directory::AppCache => config.overrides.cache = path,
            Directory::UserData => config.overrides.instances = path,
            Directory::AppTemp => config.overrides.temp = path,
        }
        config.paths = ResolvedPaths::new(&config.defaults, &config.overrides);
        Ok(())
    }

    pub fn path(&self, location: DataPath) -> &Path {
        let paths = &self.config.paths;
        match location {
            DataPath::Root => &paths.data,
            DataPath::Cache => &paths.caches,
            DataPath::Profiles => &paths.instances,
            DataPath::Temp => &paths.temp,
            DataPath::JavaVersions => &paths.java_versions,
            DataPath::DownloadProbe => &paths.download_debug_file,
            DataPath::AuthAgent => &paths.auth_agent,
        }
    }

    /// A private scratch directory removed when the returned handle is dropped.
    pub fn scratch(&self, space: ScratchSpace) -> io::Result<tempfile::TempDir> {
        match space {
            ScratchSpace::Task => private_scratch(self.path(DataPath::Temp)),
            ScratchSpace::Worker => private_scratch(&self.path(DataPath::Cache).join("workers")),
        }
    }

    /// The same game files and native archive cache serve installation and launch.
    pub fn game(&self) -> &GameDir {
        &self.config.paths.game
    }

    /// A unique cache directory that keeps the outputs of one development test.
    pub fn new_download_debug_batch(&self) -> io::Result<PathBuf> {
        let batches = self.path(DataPath::Cache).join("download-debug-batches");
        fs::create_dir_all(&batches)?;
        tempfile::Builder::new()
            .prefix("batch-")
            .tempdir_in(batches)
            .map(tempfile::TempDir::keep)
    }
}

fn private_scratch(root: &Path) -> io::Result<tempfile::TempDir> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(root)?;
    let mut scratch = tempfile::Builder::new();
    scratch.prefix("AXLauncher-");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        scratch.permissions(fs::Permissions::from_mode(0o700));
    }
    scratch.tempdir_in(root)
}

fn missing_directory(kind: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!(
            "Cannot determine the system {kind} directory. Set {DATA_DIR_ENV} or choose a data directory."
        ),
    )
}

fn absolute_directory(path: &Path) -> io::Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Enter a directory.",
        ));
    }
    let path = std::path::absolute(path)?;
    match fs::metadata(&path) {
        Ok(metadata) if !metadata.is_dir() => Err(io::Error::new(
            io::ErrorKind::NotADirectory,
            format!("{} is not a directory.", path.display()),
        )),
        Ok(_) => Ok(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(path),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/storage/layout/src/paths.rs"]
mod tests;
