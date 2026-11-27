// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

#[test]
fn a_rejected_body_is_quoted_briefly() {
    let long = "x".repeat(BODY_EXCERPT_LEN + 50);
    let error = FetchError::Rejected {
        status: 400,
        body: long.into_bytes(),
    };
    let text = error.to_string();

    assert!(text.starts_with("HTTP 400: "), "{text}");
    assert!(text.ends_with('…'), "{text}");
    assert!(
        text.chars().count() < BODY_EXCERPT_LEN + 40,
        "{}",
        text.chars().count()
    );
}

#[test]
fn a_rejected_body_reads_as_the_answer_it_was() {
    let error = FetchError::Rejected {
        status: 400,
        body: b"no loader version found".to_vec(),
    };

    assert_eq!(error.to_string(), "HTTP 400: no loader version found");
}
