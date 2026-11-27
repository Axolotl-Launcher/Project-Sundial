// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! User preferences for Minecraft's Java heap.

use serde::{Deserialize, Serialize};

/// A manually configured maximum Java heap in MiB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "HeapField")]
pub struct MaximumHeap {
    maximum_mib: u32,
}

impl MaximumHeap {
    /// A maximum large enough for the JVM itself to initialize.
    pub const MINIMUM_MIB: u32 = 16;

    pub const fn new(maximum_mib: u32) -> Option<Self> {
        if maximum_mib < Self::MINIMUM_MIB {
            return None;
        }
        Some(Self { maximum_mib })
    }

    pub const fn mib(self) -> u32 {
        self.maximum_mib
    }
}

impl Default for MaximumHeap {
    fn default() -> Self {
        Self { maximum_mib: 3072 }
    }
}

#[derive(Deserialize)]
struct HeapField {
    maximum_mib: u32,
}

impl TryFrom<HeapField> for MaximumHeap {
    type Error = &'static str;

    fn try_from(field: HeapField) -> Result<Self, Self::Error> {
        Self::new(field.maximum_mib).ok_or("maximum heap must be at least 16 MiB")
    }
}

/// The launcher-wide setting used by profiles with no override.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum MemoryAllocation {
    #[default]
    Automatic,
    Manual(MaximumHeap),
}

impl MemoryAllocation {
    pub const fn manual_heap(self) -> Option<MaximumHeap> {
        match self {
            Self::Automatic => None,
            Self::Manual(heap) => Some(heap),
        }
    }
}
