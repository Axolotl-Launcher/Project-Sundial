// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::Inspection;

#[test]
fn a_changed_java_catalog_invalidates_only_older_diagnostics() {
    let mut inspection = Inspection {
        output: Some(Err("older result".into())),
        ..Inspection::default()
    };
    inspection.mark_java_refresh();
    inspection.invalidate_refreshed_catalog();
    assert!(inspection.output.is_none());

    inspection.output = Some(Err("older result".into()));
    inspection.mark_java_refresh();
    inspection.begin_request();
    inspection.output = Some(Err("newer result".into()));
    inspection.invalidate_refreshed_catalog();
    assert_eq!(
        inspection.output.and_then(Result::err).as_deref(),
        Some("newer result")
    );
}
