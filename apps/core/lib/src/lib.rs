//! Runs the launcher's embedded libraries in on-demand worker processes.

pub mod accounts;
pub mod downloads;
pub mod installation;

mod account_client;
mod bootstrap;
mod client;
mod installer_client;
mod native;
mod path;
mod process;
mod protocol;
mod server;

pub use account_client::DesktopAccounts;
pub use bootstrap::Images;
pub use client::{Cancellation, Lease, Request, Service};
pub use native::Image;
pub use protocol::Event;
pub use server::{Callbacks, Responder, Retention, serve, serve_with_callbacks};
