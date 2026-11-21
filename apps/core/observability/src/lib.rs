// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! Command metrics and diagnostic capture.

/// One operation the launcher performed and how it went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandMetric {
    pub command: String,
    pub succeeded: bool,
}
