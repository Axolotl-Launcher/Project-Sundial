// SPDX-FileCopyrightText: 2026 The Superlogical Authors
// SPDX-License-Identifier: GPL-3.0-only

//! HTTP and protocol clients.

use superlogical_spi::{FetchError, Network};

/// A network provider that has not been given a transport yet.
///
/// Replacing this with a real HTTP client is the only change a caller sees.
#[derive(Debug, Default)]
pub struct UnimplementedNetwork;

impl Network for UnimplementedNetwork {
    async fn fetch(&self, _url: &str) -> Result<Vec<u8>, FetchError> {
        Err(FetchError::Fatal("no transport configured".into()))
    }
}
