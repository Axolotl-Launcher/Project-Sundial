// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

/// The file name Java uses for `mode` on this platform, for tests elsewhere in
/// the crate.
pub(crate) fn executable_name(mode: JavaMode) -> &'static str {
    name(mode)
}
