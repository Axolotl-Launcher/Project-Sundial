// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use bytes::Bytes;

use crate::FetchError;

/// HTTP negotiation to use for a file transfer. The default is HTTP/1.1.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DownloadProtocol {
    Automatic,
    #[default]
    Http1,
    Http2,
}

/// A byte interval with an exclusive upper bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    pub start: u64,
    pub end: u64,
}

/// The interval the server returned and the full size of the representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResponseRange {
    pub interval: ByteRange,
    pub total: u64,
}

/// Response metadata and the body to consume incrementally.
pub struct DownloadResponse<Body> {
    pub length: Option<u64>,
    pub range: Option<ResponseRange>,
    /// A strong entity tag suitable for subsequent `If-Range` requests.
    pub validator: Option<String>,
    pub body: Body,
}

/// An incremental source of response bytes.
pub trait DownloadBody {
    fn next_chunk(&mut self) -> impl Future<Output = Result<Option<Bytes>, FetchError>>;
}

impl DownloadBody for Option<Bytes> {
    async fn next_chunk(&mut self) -> Result<Option<Bytes>, FetchError> {
        Ok(self.take())
    }
}
