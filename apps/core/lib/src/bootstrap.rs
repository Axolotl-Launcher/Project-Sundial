use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::thread::JoinHandle;

use axlauncher_storage::WorkerLibrary;

use crate::account_client;
use crate::accounts::Reply;
use crate::client::Service;
use crate::java_discovery;
use crate::native::{self, Image, WORKER_ARGUMENT};
use crate::{instance_settings, settings_client};

#[derive(Clone, Copy)]
pub struct Images {
    pub downloads: Image,
    pub accounts: Image,
    pub java_discovery: Image,
    pub instance_settings: Image,
}

impl Images {
    pub const fn new(
        downloads: &'static [u8],
        accounts: &'static [u8],
        java_discovery: &'static [u8],
        instance_settings: &'static [u8],
    ) -> Self {
        Self {
            downloads: Image::new(WorkerLibrary::Downloads, downloads),
            accounts: Image::new(WorkerLibrary::Accounts, accounts),
            java_discovery: Image::new(WorkerLibrary::JavaDiscovery, java_discovery),
            instance_settings: Image::new(WorkerLibrary::InstanceSettings, instance_settings),
        }
    }

    /// Registers bundled images without starting workers.
    pub fn configure(self) {
        let executable =
            worker_executable().map_err(|error| format!("Could not locate the launcher: {error}"));
        crate::downloads::configure(self.downloads, executable.clone());
        java_discovery::configure(self.java_discovery, executable.clone());
        settings_client::configure(self.instance_settings, executable.clone());
        account_client::configure(self.accounts, executable);
    }

    /// Drains embedded workers off-thread, including ones already closing.
    pub fn begin_shutdown() -> JoinHandle<()> {
        let accounts = account_client::take_workers();
        let settings = settings_client::take_workers();
        std::thread::spawn(move || drain_workers(accounts, settings))
    }

    /// Recognizes worker mode before opening a GUI or reading user settings.
    pub fn dispatch(self) -> Option<ExitCode> {
        let mut arguments = std::env::args_os().skip(1);
        if arguments.next().as_deref() != Some(OsStr::new(WORKER_ARGUMENT)) {
            return None;
        }
        let result = self.run_worker(arguments);
        Some(if result == 0 {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        })
    }

    fn run_worker(self, mut arguments: impl Iterator<Item = std::ffi::OsString>) -> i32 {
        let (Some(kind), Some(path), None) = (arguments.next(), arguments.next(), arguments.next())
        else {
            return crate::server::startup_failed("invalid worker arguments");
        };
        let Some(image) = [
            self.downloads,
            self.accounts,
            self.java_discovery,
            self.instance_settings,
        ]
        .into_iter()
        .find(|image| image.matches(&kind)) else {
            return crate::server::startup_failed("unknown embedded worker");
        };
        native::run(image, Path::new(&path)).unwrap_or_else(crate::server::startup_failed)
    }
}

fn drain_workers(
    accounts: Vec<Arc<Service<Reply>>>,
    settings: Vec<Arc<Service<instance_settings::Reply>>>,
) {
    crate::downloads::close();
    java_discovery::close();
    for worker in &accounts {
        worker.close();
    }
    for worker in &settings {
        worker.close();
    }
    for worker in accounts {
        worker.close_and_wait();
    }
    for worker in settings {
        worker.close_and_wait();
    }
    crate::downloads::wait_closed();
    java_discovery::wait_closed();
}

/// A launcher that starts the binary through a loader sets this, because
/// `/proc/self/exe` then names the loader rather than this image.
const EXECUTABLE_VARIABLE: &str = "AXLAUNCHER_EXECUTABLE";

/// Linux must exec the running image, even if the package has replaced its path.
fn worker_executable() -> io::Result<PathBuf> {
    if let Some(executable) = std::env::var_os(EXECUTABLE_VARIABLE) {
        return Ok(PathBuf::from(executable));
    }
    if cfg!(target_os = "linux") {
        return Ok(PathBuf::from("/proc/self/exe"));
    }
    std::env::current_exe()
}

#[cfg(all(test, target_os = "linux"))]
#[path = "../../../../tests/apps/core/lib/src/bootstrap.rs"]
mod tests;
use std::io;
