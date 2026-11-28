// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Linux entry point. All UI construction lives in `axlauncher_frontend`.

use axlauncher_frontend as frontend;

#[path = "../../entry.rs"]
mod entry;

fn main() -> std::process::ExitCode {
    entry::run()
}
