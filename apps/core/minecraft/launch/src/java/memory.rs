// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Heap sizing and the defaults supplied to Java at launch.
//!
//! The workload targets and diminishing shares are based on PCL-CE's
//! [`GetRam`](https://github.com/PCL-Community/PCL-CE/blob/d530811c8e464d631e8d159e76c8f78d238812a9/Plain%20Craft%20Launcher%202/Pages/PageSetup/PageSetupLaunch.xaml.cs#L503)
//! (Apache-2.0). Axolotl implements the sizing in Rust and applies its own
//! host memory budget and initial heap policy.

use std::path::Path;

use axlauncher_domain::memory::MaximumHeap;
use axlauncher_storage::PhysicalMemory;

use super::runtime::JavaRuntime;
use crate::version::Version;

mod arguments;
pub(crate) use arguments::jvm_defaults;

const BYTES_PER_MIB: u64 = 1024 * 1024;
const MIB_PER_GIB: u32 = 1024;
const JAVA_32_BIT_MAX_MIB: u32 = MIB_PER_GIB;
const BASE_HEAP_GIB: f64 = 0.5;
const INITIAL_AVAILABLE_SHARE: f64 = 0.25;
const HOST_RESERVE_DIVISOR: u64 = 4;
const RAM_GRANULARITY_GIB: f64 = 0.1;
const STAGE_SHARES: [f64; 4] = [1.0, 0.7, 0.4, 0.15];
const VANILLA_TARGETS_GIB: [f64; 3] = [1.5, 2.5, 4.0];
const OPTIFINE_TARGETS_GIB: [f64; 3] = [1.5, 3.0, 5.0];
const MODDED_TARGETS_GIB: [f64; 3] = [1.5, 2.7, 4.5];
const MODS_PER_EXTRA_GIB: [f64; 4] = [150.0, 90.0, 50.0, 25.0];

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
        let java_32_bit = JavaRuntime::inspect(executable).is_some_and(|java| java.is_32_bit());
        match self {
            Self::Manual(heap) => HeapDefaults {
                initial_mib: None,
                maximum_mib: cap_for_java(heap.mib(), java_32_bit),
            },
            Self::Automatic { enabled_mods } => {
                let memory = PhysicalMemory::read();
                automatic_heap(
                    memory.available_bytes(),
                    memory.total_bytes(),
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
    total_bytes: u64,
    enabled_mods: Option<usize>,
    optifine: bool,
    java_32_bit: bool,
) -> HeapDefaults {
    let [minimum, first, second, third] = demand(enabled_mods, optifine);
    let bytes_per_gib = BYTES_PER_MIB * u64::from(MIB_PER_GIB);
    let available = round_tenths(available_bytes as f64 / bytes_per_gib as f64);
    // Leave room for the OS and for heap growth while the game is starting.
    let initial_budget = (available * INITIAL_AVAILABLE_SHARE).max(BASE_HEAP_GIB);
    let goal = staged_maximum(available, [first, second - first, third - second, third]);
    let estimated_mib = gib_to_mib(round_tenths(goal.max(minimum)));
    let maximum_mib = cap_for_java(
        estimated_mib.min(host_budget(available_bytes, total_bytes)),
        java_32_bit,
    );
    let initial_mib = gib_to_mib(minimum.min(initial_budget)).min(maximum_mib);
    HeapDefaults {
        initial_mib: Some(initial_mib),
        maximum_mib,
    }
}

fn staged_maximum(mut remaining: f64, stages: [f64; 4]) -> f64 {
    let mut maximum = 0.0;
    for (width, share) in stages.into_iter().zip(STAGE_SHARES) {
        let capacity = remaining * share;
        maximum += capacity.min(width);
        if capacity <= width {
            break;
        }
        remaining -= width / share;
        if remaining < RAM_GRANULARITY_GIB {
            break;
        }
    }
    maximum
}

fn host_budget(available_bytes: u64, total_bytes: u64) -> u32 {
    // Preserve a basic heap under transient pressure, within the physical limit.
    let available_mib = (available_bytes / BYTES_PER_MIB).max(u64::from(gib_to_mib(BASE_HEAP_GIB)));
    let physical_budget_mib = (total_bytes - total_bytes / HOST_RESERVE_DIVISOR) / BYTES_PER_MIB;
    let budget = if total_bytes == 0 {
        available_mib
    } else {
        available_mib.min(physical_budget_mib)
    };
    u32::try_from(budget).unwrap_or(u32::MAX)
}

fn cap_for_java(maximum_mib: u32, java_32_bit: bool) -> u32 {
    if java_32_bit {
        return maximum_mib.min(JAVA_32_BIT_MAX_MIB);
    }
    maximum_mib
}

fn demand(enabled_mods: Option<usize>, optifine: bool) -> [f64; 4] {
    let [first, second, third] = MODDED_TARGETS_GIB;
    if let Some(count) = enabled_mods {
        let mut demand = [BASE_HEAP_GIB, first, second, third];
        for (target, mods_per_gib) in demand.iter_mut().zip(MODS_PER_EXTRA_GIB) {
            *target += count as f64 / mods_per_gib;
        }
        return demand;
    }
    let [first, second, third] = if optifine {
        OPTIFINE_TARGETS_GIB
    } else {
        VANILLA_TARGETS_GIB
    };
    [BASE_HEAP_GIB, first, second, third]
}

fn round_tenths(value: f64) -> f64 {
    (value * 10.0).round_ties_even() / 10.0
}

fn gib_to_mib(value: f64) -> u32 {
    (value * f64::from(MIB_PER_GIB)).floor() as u32
}

#[cfg(test)]
#[path = "../../../../../../tests/apps/core/minecraft/launch/src/java/memory.rs"]
mod tests;
