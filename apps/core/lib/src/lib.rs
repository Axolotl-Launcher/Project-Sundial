//! Runs the launcher's embedded libraries in on-demand worker processes.

pub mod accounts;
pub mod downloads;
pub mod instance_settings;

mod account_client;
mod bootstrap;
mod client;
mod native;
mod path;
mod protocol;
mod server;
mod settings_client;

pub use account_client::DesktopAccounts;
pub use bootstrap::Images;
pub use client::{Lease, Request, Service};
pub use native::Image;
pub use protocol::Event;
pub use server::{Cancellation, Responder, Retention, serve};
