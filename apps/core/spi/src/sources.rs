// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

/// Selects the source policy for a buffered request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchKind {
    Metadata,
    File,
}

/// An alternate URL that serves the same resource as the supplied URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadMirror {
    pub url: String,
    pub preferred: bool,
}
