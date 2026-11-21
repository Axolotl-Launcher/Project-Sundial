// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! Superlogical core: the domain systems, re-exported under one name.
//!
//! Each system is its own crate so the layering is enforced by Cargo rather
//! than by directory convention: a system can only call what it declares.
//! Applications depend on `superlogical_core` and never on a system directly.

pub use superlogical_content as content;
pub use superlogical_domain as domain;
pub use superlogical_install as install;
pub use superlogical_launch as launch;
pub use superlogical_mod_metadata as mod_metadata;
pub use superlogical_network as network;
pub use superlogical_observability as observability;
pub use superlogical_resources as resources;
pub use superlogical_scheduler as scheduler;
pub use superlogical_state as state;
pub use superlogical_storage as storage;
pub use superlogical_telemetry as telemetry;

pub const BRAND_NAME: &str = "Superlogical";
