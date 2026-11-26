// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

/// A snapshot of the host's physical memory, including reclaimable memory.
pub struct PhysicalMemory {
    available_bytes: u64,
    total_bytes: u64,
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/storage/src/memory.rs"]
mod tests;

impl PhysicalMemory {
    /// Refreshes RAM figures without collecting CPU or process information.
    pub fn read() -> Self {
        let mut system = sysinfo::System::new();
        system.refresh_memory_specifics(sysinfo::MemoryRefreshKind::new().with_ram());
        Self {
            available_bytes: system.available_memory(),
            total_bytes: system.total_memory(),
        }
    }

    pub fn available_bytes(&self) -> u64 {
        self.available_bytes
    }

    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }
}
