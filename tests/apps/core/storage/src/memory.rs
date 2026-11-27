// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::PhysicalMemory;

#[test]
fn physical_memory_probe_reports_sane_host_capacity() {
    let memory = PhysicalMemory::read();
    assert!(memory.total_bytes() > 0);
    assert!(memory.available_bytes() <= memory.total_bytes());
}
