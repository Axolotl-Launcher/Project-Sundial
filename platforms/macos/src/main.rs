// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! macOS entry point. All UI construction lives in `axlauncher_frontend`.
//!
//! The macOS bundle layout (Info.plist, icon, code signing) is produced around
//! this binary; see `packaging/macos`.

#[path = "../../entry.rs"]
mod entry;

fn main() -> std::process::ExitCode {
    entry::run()
}
