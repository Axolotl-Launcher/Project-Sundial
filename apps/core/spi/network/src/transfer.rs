// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use bytes::Bytes;

use crate::FetchError;

/// HTTP negotiation to use for a file transfer. The default is HTTP/1.1.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

/// An interval and an optional strong validator from a previous response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeRequest<'a> {
    pub interval: ByteRange,
    pub if_range: Option<&'a [u8]>,
}

/// The interval the server returned and the full size of the representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResponseRange {
    pub interval: ByteRange,
    pub total: u64,
}

/// The kind of entity tag the server supplied for a partial response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadEtag {
    Strong(Bytes),
    Unusable,
}

/// The effective resource and validator after following redirects.
pub struct DownloadRepresentation {
    pub resource: String,
    pub etag: Option<DownloadEtag>,
}

/// Response metadata and the body to consume incrementally.
pub struct DownloadResponse<Body> {
    pub length: Option<u64>,
    pub range: Option<ResponseRange>,
    pub representation: Option<DownloadRepresentation>,
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
