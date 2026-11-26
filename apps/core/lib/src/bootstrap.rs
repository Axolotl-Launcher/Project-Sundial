use std::ffi::OsStr;
use std::path::Path;
use std::process::ExitCode;

use axlauncher_storage::WorkerLibrary;

use crate::native::{self, Image, WORKER_ARGUMENT};

#[derive(Clone, Copy)]
pub struct Images {
    pub downloads: Image,
    pub accounts: Image,
}

impl Images {
    pub const fn new(downloads: &'static [u8], accounts: &'static [u8]) -> Self {
        Self {
            downloads: Image::new(WorkerLibrary::Downloads, downloads),
            accounts: Image::new(WorkerLibrary::Accounts, accounts),
        }
    }

    /// Registers bundled images without starting workers.
    pub fn configure(self) {
        crate::downloads::configure(self.downloads);
        crate::account_client::configure(self.accounts);
    }

    /// Drains active workers before the desktop exits.
    pub fn close_workers() {
        crate::DesktopAccounts::close_active();
        crate::downloads::close();
    }

    /// Recognizes worker mode before opening a GUI or reading user settings.
    pub fn dispatch(self) -> Option<ExitCode> {
        let mut arguments = std::env::args_os().skip(1);
        if arguments.next().as_deref() != Some(OsStr::new(WORKER_ARGUMENT)) {
            return None;
        }
        let (Some(kind), Some(path), None) = (arguments.next(), arguments.next(), arguments.next())
        else {
            return Some(ExitCode::FAILURE);
        };
        let outcome = [self.downloads, self.accounts]
            .into_iter()
            .find(|image| image.matches(&kind))
            .map(|image| native::run(image, Path::new(&path)));
        Some(if matches!(outcome, Some(Ok(0))) {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        })
    }
}
