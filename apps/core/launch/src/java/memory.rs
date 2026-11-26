// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Heap sizing and the defaults supplied to Java at launch.

use std::path::Path;

use axlauncher_domain::memory::MaximumHeap;
use axlauncher_storage::PhysicalMemory;

use super::runtime::JavaRuntime;
use crate::version::Version;

mod arguments;
pub(crate) use arguments::jvm_defaults;

const BYTES_PER_MIB: u64 = 1024 * 1024;
const MIB_PER_GIB: u32 = 1024;
const BASE_HEAP_GIB: f64 = 0.5;
const INITIAL_AVAILABLE_SHARE: f64 = 0.25;

/// The selected heap policy and the current workload when sizing automatically.
#[derive(Debug, Clone, Copy)]
pub enum HeapPolicy {
    /// `Some(count)` identifies a mod-capable instance, even without installed mods.
    Automatic {
        enabled_mods: Option<usize>,
    },
    Manual(MaximumHeap),
}

/// Heap sizes that may be supplied when the version does not specify them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeapDefaults {
    initial_mib: Option<u32>,
    maximum_mib: u32,
}

impl HeapPolicy {
    pub(crate) fn resolve(self, executable: &Path, version: &Version) -> HeapDefaults {
        match self {
            Self::Manual(heap) => HeapDefaults {
                initial_mib: None,
                maximum_mib: heap.mib(),
            },
            Self::Automatic { enabled_mods } => {
                let available = PhysicalMemory::read().available_bytes();
                let java_32_bit =
                    JavaRuntime::inspect(executable).is_some_and(|java| java.is_32_bit());
                automatic_heap(
                    available,
                    enabled_mods,
                    version.uses_optifine(),
                    java_32_bit,
                )
            }
        }
    }
}

fn automatic_heap(
    available_bytes: u64,
    enabled_mods: Option<usize>,
    optifine: bool,
    java_32_bit: bool,
) -> HeapDefaults {
    let [minimum, first, second, third] = demand(enabled_mods, optifine);
    let bytes_per_gib = BYTES_PER_MIB * u64::from(MIB_PER_GIB);
    let mut available = round_tenths(available_bytes as f64 / bytes_per_gib as f64);
    // Leave room for the OS and for heap growth while the game is starting.
    let initial_budget = (available * INITIAL_AVAILABLE_SHARE).max(BASE_HEAP_GIB);
    let mut maximum = 0.0;
    for (width, weight) in [
        (first, 1.0),
        (second - first, 0.7),
        (third - second, 0.4),
        (third, 0.15),
    ] {
        let assigned = (available * weight).min(width);
        maximum += assigned;
        available -= width / weight;
        if available < 0.1 {
            break;
        }
    }
    let mut maximum_mib = gib_to_mib(round_tenths(maximum.max(minimum)));
    if java_32_bit {
        maximum_mib = maximum_mib.min(MIB_PER_GIB);
    }
    let initial_mib = gib_to_mib(minimum.min(initial_budget)).min(maximum_mib);
    HeapDefaults {
        initial_mib: Some(initial_mib),
        maximum_mib,
    }
}

// PCL-CE assigns diminishing shares of available memory as workload goals are met.
fn demand(enabled_mods: Option<usize>, optifine: bool) -> [f64; 4] {
    if let Some(count) = enabled_mods {
        let count = count as f64;
        return [
            BASE_HEAP_GIB + count / 150.0,
            1.5 + count / 90.0,
            2.7 + count / 50.0,
            4.5 + count / 25.0,
        ];
    }
    if optifine {
        return [BASE_HEAP_GIB, 1.5, 3.0, 5.0];
    }
    [BASE_HEAP_GIB, 1.5, 2.5, 4.0]
}

fn round_tenths(value: f64) -> f64 {
    (value * 10.0).round_ties_even() / 10.0
}

fn gib_to_mib(value: f64) -> u32 {
    (value * f64::from(MIB_PER_GIB)).floor() as u32
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/launch/src/java/memory.rs"]
mod tests;
