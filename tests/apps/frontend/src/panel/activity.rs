// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

#[test]
fn a_recorded_launch_renders_in_the_current_language() {
    let mut log = ActivityLog::new();
    log.record_launch(Path::new("{status}"), "1.21", &Launch::Running);
    let recorded = &log.entries[0].message;

    assert_eq!(
        recorded.render(LauncherLanguage::English),
        "{status} (1.21): Minecraft running"
    );
    assert_eq!(
        recorded.render(LauncherLanguage::SimplifiedChinese),
        "{status}（1.21）：Minecraft 正在运行"
    );
}
