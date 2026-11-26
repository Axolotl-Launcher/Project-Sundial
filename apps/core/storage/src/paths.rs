use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::game_dir::GameDir;

const APP_DIRECTORY: &str = "AXLauncher";
const DATA_DIR_ENV: &str = "AXLAUNCHER_DATA_DIR";

/// The system's per-user application data directory.
pub fn user_data_dir() -> Option<PathBuf> {
    dirs::data_dir()
}

/// The temporary directory reported by the operating system.
pub fn user_temp_dir() -> PathBuf {
    std::env::temp_dir()
}

/// Locations that can be overridden for the current launcher session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Directory {
    AppData,
    AppCache,
    UserData,
    AppTemp,
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

impl DataDir {
    /// Startup uses an isolated root when set; otherwise it uses platform directories.
    pub fn from_environment() -> io::Result<Self> {
        Self::resolve(
            std::env::var_os(DATA_DIR_ENV),
            user_data_dir,
            dirs::cache_dir,
            user_temp_dir,
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
        Self {
            config: Arc::new(DirectoryConfig {
                defaults,
                overrides: Overrides::default(),
            }),
        }
    }

    /// `None` restores the default. Invalid input leaves the snapshot unchanged.
    pub fn set_directory(&mut self, directory: Directory, path: Option<&Path>) -> io::Result<()> {
        let path = path.map(absolute_directory).transpose()?;
        let overrides = &mut Arc::make_mut(&mut self.config).overrides;
        match directory {
            Directory::AppData => overrides.data = path,
            Directory::AppCache => overrides.cache = path,
            Directory::UserData => overrides.instances = path,
            Directory::AppTemp => overrides.temp = path,
        }
        Ok(())
    }

    pub fn root(&self) -> &Path {
        self.config
            .overrides
            .data
            .as_deref()
            .unwrap_or(&self.config.defaults.data)
    }

    pub fn caches(&self) -> PathBuf {
        self.config
            .overrides
            .cache
            .as_ref()
            .or(self.config.defaults.cache.as_ref())
            .cloned()
            .unwrap_or_else(|| self.root().join("caches"))
    }

    /// The launcher-managed user data directory: one folder per instance.
    pub fn instances(&self) -> PathBuf {
        self.config
            .overrides
            .instances
            .clone()
            .unwrap_or_else(|| self.root().join("profiles"))
    }

    /// The parent of private scratch directories.
    pub fn temp(&self) -> PathBuf {
        self.config
            .overrides
            .temp
            .as_ref()
            .or(self.config.defaults.temp.as_ref())
            .cloned()
            .unwrap_or_else(|| self.root().join("tmp"))
    }

    /// A private scratch directory removed when the returned handle is dropped.
    pub fn scratch(&self) -> io::Result<tempfile::TempDir> {
        let root = self.temp();
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&root)?;
        tempfile::Builder::new()
            .prefix("AXLauncher-")
            .tempdir_in(root)
    }

    /// The same game files and native archive cache serve installation and launch.
    pub fn game(&self) -> GameDir {
        GameDir::shared(
            self.root().join("meta"),
            self.caches().join("minecraft-natives"),
        )
    }

    pub fn java_versions(&self) -> PathBuf {
        self.root().join("java")
    }

    pub fn jobs(&self) -> PathBuf {
        self.root().join("jobs")
    }

    pub fn download_debug_file(&self) -> PathBuf {
        self.caches().join("download-debug.bin")
    }

    /// A unique cache directory that keeps the outputs of one development test.
    pub fn new_download_debug_batch(&self) -> io::Result<PathBuf> {
        let batches = self.caches().join("download-debug-batches");
        fs::create_dir_all(&batches)?;
        tempfile::Builder::new()
            .prefix("batch-")
            .tempdir_in(batches)
            .map(tempfile::TempDir::keep)
    }

    pub fn accounts(&self) -> PathBuf {
        self.root().join("accounts.json")
    }

    pub fn auth_agent(&self) -> PathBuf {
        self.game().root().join("authlib-injector-1.2.8.jar")
    }

    /// Account writers hold this across a read-modify-write operation.
    pub fn lock_accounts(&self) -> io::Result<fs::File> {
        fs::create_dir_all(self.root())?;
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root().join("accounts.lock"))?;
        file.lock()?;
        Ok(file)
    }
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

pub fn settings_path(data: &Path) -> PathBuf {
    data.join("settings.json")
}

pub fn settings_lock_path(data: &Path) -> PathBuf {
    data.join("settings.lock")
}

pub fn invalid_settings_path(data: &Path) -> PathBuf {
    data.join("settings.json.invalid")
}

pub fn javas_path(data: &Path) -> PathBuf {
    data.join("javas.json")
}

pub fn runtime_record(runtime: &Path) -> PathBuf {
    runtime.join(".axlauncher-complete")
}

pub fn job_document(jobs: &Path, id: &str) -> PathBuf {
    jobs.join(format!("{id}.json"))
}

pub fn download_debug_batch_file(batch: &Path, line: usize) -> PathBuf {
    batch.join(format!("{line}.bin"))
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/storage/src/paths.rs"]
mod tests;
