// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! User-managed resources: favourites, history, fingerprints.

use axlauncher_domain::Id;

/// A resource the user marked as worth keeping at hand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Favorite {
    pub item: Id,
}
