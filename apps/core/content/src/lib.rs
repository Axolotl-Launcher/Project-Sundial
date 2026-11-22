// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Content model and providers.

use axlauncher_spi::{ContentProvider, ContentSummary, FetchError};

/// A provider whose catalogue is fixed in memory.
#[derive(Debug, Default)]
pub struct StaticProvider {
    items: Vec<ContentSummary>,
}

impl StaticProvider {
    pub fn with_items(items: Vec<ContentSummary>) -> Self {
        Self { items }
    }
}

impl ContentProvider for StaticProvider {
    async fn list(&self) -> Result<Vec<ContentSummary>, FetchError> {
        Ok(self.items.clone())
    }
}
