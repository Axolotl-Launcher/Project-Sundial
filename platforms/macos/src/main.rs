// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! macOS entry point. All UI construction lives in `superlogical_frontend`.
//!
//! The macOS bundle layout (Info.plist, icon, code signing) is produced around
//! this binary; see `packaging/macos`.

fn main() {
    superlogical_frontend::run();
}
