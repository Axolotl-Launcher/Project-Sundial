// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

fn bounds(initial_mib: u32, maximum_mib: u32) -> HeapDefaults {
    HeapDefaults {
        initial_mib: Some(initial_mib),
        maximum_mib,
    }
}

#[test]
fn automatic_heap_uses_the_pcl_workload_and_available_memory_stages() {
    let eight_gib = 8 << 30;
    let vanilla = automatic_heap(eight_gib, None, false, false);
    assert_eq!(vanilla, bounds(512, 4300));
    assert_eq!(
        automatic_heap(eight_gib, None, true, false).maximum_mib,
        4812
    );
    assert_eq!(
        automatic_heap(eight_gib, Some(150), false, false),
        bounds(1536, 6348)
    );
    assert_eq!(
        automatic_heap(1 << 30, None, false, false).maximum_mib,
        1024
    );
}

#[test]
fn scarcity_and_32_bit_java_still_produce_valid_bounds() {
    assert_eq!(
        automatic_heap(2 << 30, Some(600), false, false),
        bounds(512, 4608)
    );
    assert_eq!(automatic_heap(0, None, false, false), bounds(512, 512));
    assert_eq!(
        automatic_heap(32 << 30, Some(180), false, true),
        bounds(1024, 1024)
    );
}

#[test]
fn manifest_bounds_replace_defaults_individually() {
    let heap = bounds(512, 2048);
    assert_eq!(
        jvm_defaults(&[], heap),
        [Some("-Xms512m".into()), Some("-Xmx2048m".into())]
    );
    assert_eq!(
        jvm_defaults(&["-Xms2g".into(), "-Xmx3g".into()], heap),
        [None, None]
    );
    assert_eq!(
        jvm_defaults(&["-XX:MaxHeapSize=384M".into()], heap),
        [Some("-Xms384M".into()), None]
    );
    assert_eq!(
        jvm_defaults(&["-Xms3g".into()], heap),
        [None, Some("-Xmx3g".into())]
    );
}

#[test]
fn explicit_jvm_ergonomics_keep_control_of_the_affected_bounds() {
    let heap = bounds(512, 3072);
    assert_eq!(
        jvm_defaults(&["-XX:MaxRAMPercentage=50".into()], heap),
        [None, None]
    );
    assert_eq!(
        jvm_defaults(&["-XX:InitialRAMPercentage=2".into()], heap),
        [None, Some("-Xmx3072m".into())]
    );
    assert_eq!(
        jvm_defaults(&["-XX:MaxRAM=512m".into()], heap),
        [None, None]
    );
    assert_eq!(
        jvm_defaults(&["-XX:SoftMaxHeapSize=4g".into()], heap),
        [Some("-Xms512m".into()), Some("-Xmx4g".into())]
    );
    assert_eq!(
        jvm_defaults(&["-XX:MinHeapSize=4g".into()], heap),
        [Some("-Xms4g".into()), Some("-Xmx4g".into())]
    );
}
