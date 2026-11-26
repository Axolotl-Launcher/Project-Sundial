//! Runs the launcher's embedded libraries in on-demand worker processes.

pub mod accounts;
pub mod downloads;

mod account_client;
mod bootstrap;
mod client;
mod native;
mod path;
mod protocol;
mod server;

pub use account_client::DesktopAccounts;
pub use bootstrap::Images;
pub use client::{Lease, Request, Service};
pub use native::Image;
pub use protocol::Event;
pub use server::{Cancellation, Responder, Retention, serve};
