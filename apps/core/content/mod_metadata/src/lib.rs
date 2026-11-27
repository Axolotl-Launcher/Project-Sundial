// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Loaders and mod metadata.

/// Which mod loader a version targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Loader {
    Fabric,
    Forge,
    NeoForge,
    Quilt,
}
