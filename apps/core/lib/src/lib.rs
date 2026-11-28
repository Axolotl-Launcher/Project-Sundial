//! Runs the launcher's embedded libraries in on-demand worker processes.

pub mod accounts;
pub mod downloads;
#[path = "workers/installation.rs"]
pub mod installation;
pub mod instance_settings;
#[path = "workers/java_discovery.rs"]
pub mod java_discovery;

mod account_client;
mod bootstrap;
mod client;
mod installer_client;
mod native;
mod path;
mod process;
mod protocol;
mod server;
#[path = "workers/settings_client.rs"]
mod settings_client;

pub use account_client::DesktopAccounts;
pub use bootstrap::Images;
pub use client::{Cancellation, Lease, Request, Service};
pub use native::Image;
pub use protocol::Event;
pub use server::{Callbacks, Responder, Retention, serve, serve_with_callbacks};
