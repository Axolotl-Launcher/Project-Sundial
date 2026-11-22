// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Axolotl Launcher core: the domain systems, re-exported under one name.
//!
//! Each system is its own crate so the layering is enforced by Cargo rather
//! than by directory convention: a system can only call what it declares.
//! Applications depend on `axlauncher_core` and never on a system directly.

pub use axlauncher_content as content;
pub use axlauncher_domain as domain;
pub use axlauncher_install as install;
pub use axlauncher_launch as launch;
pub use axlauncher_loader as loader;
pub use axlauncher_mod_metadata as mod_metadata;
pub use axlauncher_network as network;
pub use axlauncher_observability as observability;
pub use axlauncher_resources as resources;
pub use axlauncher_scheduler as scheduler;
pub use axlauncher_state as state;
pub use axlauncher_storage as storage;
pub use axlauncher_telemetry as telemetry;

pub const BRAND_NAME: &str = "Axolotl Launcher";
