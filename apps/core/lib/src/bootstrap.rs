use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;
use std::thread::JoinHandle;

use axlauncher_storage::WorkerLibrary;

use crate::account_client;
use crate::accounts::Reply;
use crate::client::Service;
use crate::native::{self, Image, WORKER_ARGUMENT};

#[derive(Clone, Copy)]
pub struct Images {
    pub downloads: Image,
    pub accounts: Image,
    pub install: Image,
}

impl Images {
    pub const fn new(
        downloads: &'static [u8],
        accounts: &'static [u8],
        install: &'static [u8],
    ) -> Self {
        Self {
            downloads: Image::new(WorkerLibrary::Downloads, downloads),
            accounts: Image::new(WorkerLibrary::Accounts, accounts),
            install: Image::new(WorkerLibrary::Install, install),
        }
    }

    /// Registers bundled images without starting workers.
    pub fn configure(self) {
        let executable =
            worker_executable().map_err(|error| format!("Could not locate the launcher: {error}"));
        self.configure_services(executable);
    }

    /// Registers images when the launcher is invoked through a specific executable.
    pub fn configure_for_executable(self, executable: PathBuf) {
        self.configure_services(Ok(executable));
    }

    fn configure_services(self, executable: Result<PathBuf, String>) {
        crate::downloads::configure(self.downloads, executable.clone());
        account_client::configure(self.accounts, executable.clone());
        crate::installer_client::configure(self.install, executable);
    }

    /// Drains embedded workers off-thread, including ones already closing.
    pub fn begin_shutdown() -> JoinHandle<()> {
        let accounts = account_client::take_workers();
        let wait_installs = crate::installer_client::close();
        std::thread::spawn(move || {
            drain_workers(accounts);
            wait_installs();
        })
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
        let Some(image) = [self.downloads, self.accounts, self.install]
            .into_iter()
            .find(|image| image.matches(&kind))
        else {
            return crate::server::startup_failed("unknown embedded worker");
        };
        native::run(image, Path::new(&path)).unwrap_or_else(crate::server::startup_failed)
    }
}

fn drain_workers(accounts: Vec<Arc<Service<Reply>>>) {
    crate::downloads::close();
    for worker in &accounts {
        worker.close();
    }
    for worker in accounts {
        worker.close_and_wait();
    }
    crate::downloads::wait_closed();
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
