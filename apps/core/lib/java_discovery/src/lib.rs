//! Enumerates system-wide Java candidates for the desktop discovery service.

mod scan;
#[cfg(windows)]
mod windows;
mod worker;
