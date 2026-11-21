// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! Application state and persistence.

use superlogical_spi::{Id, InstanceRecord, StateStore, StorageError};

/// Keeps instances in memory; a database-backed store replaces it later.
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
