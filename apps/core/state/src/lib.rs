// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Application state and persistence.
//!
//! Every document this crate owns carries `schema_version`. A version this
//! build does not write is left untouched and read-only: converting another
//! format belongs to a separate tool (see AGENTS.md), and a newer build's data
//! must not be damaged by an older one.

mod document;
pub mod files;
pub mod javas;
pub mod jobs;
pub mod profile;
mod settings;
#[cfg(test)]
#[path = "../../../../tests/apps/core/state/src/test_support.rs"]
mod test_support;

pub use settings::{
    LoadedSettings, SETTINGS_SCHEMA_VERSION, SettingsData, SettingsError, SettingsOrigin,
    SourcePreference, ThemeChoice, load_settings, save_settings, update_settings,
};

use axlauncher_spi::{Id, InstanceRecord, StateStore, StorageError};

/// An ephemeral provider for the ID-based state port.
#[derive(Debug, Default)]
pub struct MemoryState {
    instances: std::collections::BTreeMap<u64, InstanceRecord>,
}

impl StateStore for MemoryState {
    fn load_instance(&self, id: Id) -> Result<Option<InstanceRecord>, StorageError> {
        Ok(self.instances.get(&id.get()).cloned())
    }

    fn save_instance(&mut self, instance: &InstanceRecord) -> Result<(), StorageError> {
        self.instances.insert(instance.id.get(), instance.clone());
        Ok(())
    }
}
