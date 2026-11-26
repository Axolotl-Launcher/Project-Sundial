// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::cell::RefCell;
use std::fs;

use axlauncher_spi::{
    ByteRange, DownloadEtag, DownloadRepresentation, DownloadResponse, RangeRequest, ResponseRange,
};
use bytes::Bytes;

use super::retry_tests::{ALTERNATE, Body, OLD_TAG, PRIMARY, SIZE, Script, job, paused, whole};
use super::{DownloadEngine, test_tempfile};

const NEW_TAG: &[u8] = b"\"new\"";

pub(super) fn representation(resource: &str, tag: Option<&'static [u8]>) -> DownloadRepresentation {
    DownloadRepresentation {
        resource: resource.into(),
        etag: tag.map(|tag| DownloadEtag::Strong(Bytes::from_static(tag))),
    }
}

fn partial(
    request: RangeRequest<'_>,
    representation: DownloadRepresentation,
) -> DownloadResponse<Body> {
    let mut response = whole(
        vec![b'a'; (request.interval.end - request.interval.start) as usize],
        false,
    );
    response.range = Some(ResponseRange {
        interval: request.interval,
        total: SIZE,
    });
    response.representation = Some(representation);
    response
}

#[test]
fn a_changed_tag_or_redirect_target_restarts_with_clean_bytes() {
    for (resource, tag) in [
        (PRIMARY, NEW_TAG),
        (ALTERNATE, OLD_TAG),
        ("https://primary.invalid/file?signature=renewed", OLD_TAG),
    ] {
        assert_representation_restart(resource, tag);
    }
}

fn assert_representation_restart(resource: &str, tag: &'static [u8]) {
    paused(async {
        let root = test_tempfile::tempdir().unwrap();
        let destination = root.path().join("file");
        let calls = RefCell::new(Vec::new());
        let network = Script(|_: &str, range: Option<RangeRequest<'_>>| {
            calls
                .borrow_mut()
                .push(range.map(|range| range.interval.start));
            Ok(changed_representation(range, resource, tag))
        });
        DownloadEngine::default()
            .download(&network, job(&destination, &[PRIMARY], SIZE))
            .await
            .unwrap();
        assert_eq!(calls.into_inner(), [Some(0), Some(SIZE / 2), None]);
        assert_eq!(fs::read(destination).unwrap(), vec![b'b'; SIZE as usize]);
    });
}

fn changed_representation(
    range: Option<RangeRequest<'_>>,
    resource: &str,
    tag: &'static [u8],
) -> DownloadResponse<Body> {
    let Some(range) = range else {
        return whole(vec![b'b'; SIZE as usize], false);
    };
    if range.interval.start == 0 {
        return partial(range, representation(PRIMARY, Some(OLD_TAG)));
    }
    assert_eq!(range.if_range, Some(OLD_TAG));
    partial(range, representation(resource, Some(tag)))
}

#[test]
fn size_only_transfers_use_a_complete_response_without_a_strong_tag() {
    for etag in [None, Some(DownloadEtag::Unusable)] {
        assert_whole_without_validator(etag);
    }
}

fn assert_whole_without_validator(etag: Option<DownloadEtag>) {
    paused(async {
        let root = test_tempfile::tempdir().unwrap();
        let destination = root.path().join("file");
        let calls = RefCell::new(Vec::new());
        let network = Script(|_: &str, range: Option<RangeRequest<'_>>| {
            calls.borrow_mut().push(range.map(|range| range.interval));
            Ok(unvalidated_response(range, etag.clone()))
        });
        DownloadEngine::default()
            .download(&network, job(&destination, &[PRIMARY], SIZE))
            .await
            .unwrap();
        assert_eq!(
            calls.into_inner(),
            [
                Some(ByteRange {
                    start: 0,
                    end: SIZE / 2
                }),
                None
            ]
        );
        assert_eq!(fs::read(destination).unwrap(), vec![b'b'; SIZE as usize]);
    });
}

fn unvalidated_response(
    range: Option<RangeRequest<'_>>,
    etag: Option<DownloadEtag>,
) -> DownloadResponse<Body> {
    let Some(range) = range else {
        return whole(vec![b'b'; SIZE as usize], false);
    };
    partial(
        range,
        DownloadRepresentation {
            resource: PRIMARY.into(),
            etag,
        },
    )
}

#[test]
fn a_trusted_digest_allows_ranges_without_an_etag() {
    paused(async {
        let root = test_tempfile::tempdir().unwrap();
        let destination = root.path().join("file");
        let calls = RefCell::new(Vec::new());
        let network = Script(|_: &str, range: Option<RangeRequest<'_>>| {
            let range = range.expect("digest-verified range");
            assert_eq!(range.if_range, None);
            calls.borrow_mut().push(range.interval.start);
            Ok(partial(range, representation(PRIMARY, None)))
        });
        DownloadEngine::default()
            .download_verified(
                &network,
                job(&destination, &[PRIMARY], SIZE),
                verify_pattern,
            )
            .await
            .unwrap();
        assert_eq!(calls.into_inner(), [0, SIZE / 2]);
        assert_eq!(fs::read(destination).unwrap(), vec![b'a'; SIZE as usize]);
    });
}

fn verify_pattern(file: &mut fs::File) -> std::io::Result<()> {
    use std::io::{Error, ErrorKind, Read};

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if bytes == vec![b'a'; SIZE as usize] {
        return Ok(());
    }
    Err(Error::new(ErrorKind::InvalidData, "wrong content"))
}

#[test]
fn range_protocol_errors_trigger_a_single_whole_file_transition() {
    for failure in [
        RangeFailure::Malformed,
        RangeFailure::Unsatisfiable,
        RangeFailure::WrongTotal,
    ] {
        assert_range_protocol_fallback(failure);
    }
}

#[derive(Clone, Copy)]
enum RangeFailure {
    Malformed,
    Unsatisfiable,
    WrongTotal,
}

fn assert_range_protocol_fallback(failure: RangeFailure) {
    paused(async {
        let root = test_tempfile::tempdir().unwrap();
        let destination = root.path().join("file");
        let calls = RefCell::new(Vec::new());
        let network = Script(|_: &str, range: Option<RangeRequest<'_>>| {
            calls
                .borrow_mut()
                .push(range.map(|request| request.interval.start));
            range.map_or_else(
                || Ok(whole(vec![b'b'; SIZE as usize], false)),
                |range| failure.respond(range),
            )
        });
        DownloadEngine::default()
            .download(&network, job(&destination, &[PRIMARY], SIZE))
            .await
            .unwrap();
        assert_eq!(calls.into_inner(), [Some(0), Some(SIZE / 2), None]);
        assert_eq!(fs::read(destination).unwrap(), vec![b'b'; SIZE as usize]);
    });
}

impl RangeFailure {
    fn respond(
        self,
        range: RangeRequest<'_>,
    ) -> Result<DownloadResponse<Body>, axlauncher_spi::FetchError> {
        let mut response = partial(range, representation(PRIMARY, Some(OLD_TAG)));
        if range.interval.start == 0 {
            return Ok(response);
        }
        let status = match self {
            Self::Malformed => 206,
            Self::Unsatisfiable => 416,
            Self::WrongTotal => {
                response.range.as_mut().unwrap().total += 1;
                return Ok(response);
            }
        };
        Err(axlauncher_spi::FetchError::Rejected {
            status,
            body: Vec::new(),
        })
    }
}

#[test]
fn a_conditional_partial_response_can_omit_its_etag() {
    paused(async {
        let root = test_tempfile::tempdir().unwrap();
        let destination = root.path().join("file");
        let network = Script(|_: &str, range: Option<RangeRequest<'_>>| {
            let range = range.expect("conditional range");
            let tag = (range.interval.start == 0).then_some(OLD_TAG);
            assert_eq!(range.if_range, tag.is_none().then_some(OLD_TAG));
            Ok(partial(range, representation(PRIMARY, tag)))
        });
        DownloadEngine::default()
            .download(&network, job(&destination, &[PRIMARY], SIZE))
            .await
            .unwrap();
        assert_eq!(fs::read(destination).unwrap(), vec![b'a'; SIZE as usize]);
    });
}

#[test]
fn a_whole_file_digest_allows_rotating_redirect_signatures() {
    paused(async {
        let root = test_tempfile::tempdir().unwrap();
        let destination = root.path().join("file");
        let network = Script(|_: &str, range: Option<RangeRequest<'_>>| {
            let range = range.expect("digest-verified range");
            let signature = range.interval.start;
            assert_eq!(range.if_range, (signature != 0).then_some(OLD_TAG));
            let resource = format!("https://primary.invalid/file?signature={signature}");
            Ok(partial(range, representation(&resource, Some(OLD_TAG))))
        });
        DownloadEngine::default()
            .download_verified(
                &network,
                job(&destination, &[PRIMARY], SIZE),
                verify_pattern,
            )
            .await
            .unwrap();
        assert_eq!(fs::read(destination).unwrap(), vec![b'a'; SIZE as usize]);
    });
}
