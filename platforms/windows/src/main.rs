// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Windows entry point. All UI construction lives in `axlauncher_frontend`.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[path = "../../entry.rs"]
mod entry;

fn main() -> std::process::ExitCode {
    entry::run()
}
