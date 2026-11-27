// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Desktop commands for global and per-instance heap preferences.

use std::path::Path;

use axlauncher_domain::memory::{MaximumHeap, MemoryAllocation};
use axlauncher_state::profile::{self, Profile, ProfileUpdateError};
use axlauncher_state::{self, SettingsData, SettingsError};

/// Saves the launcher's allocation mode and returns the updated settings.
pub fn set_global(
    root: &Path,
    allocation: MemoryAllocation,
) -> Result<SettingsData, SettingsError> {
    axlauncher_state::update_settings(root, |settings| settings.set_memory(allocation))
}

/// Saves the maximum heap override for the given instance, or restores inheritance.
pub fn set_instance(
    directory: &Path,
    id: &str,
    override_heap: Option<MaximumHeap>,
) -> Result<Profile, ProfileUpdateError> {
    profile::set_memory(directory, id, override_heap)
}
