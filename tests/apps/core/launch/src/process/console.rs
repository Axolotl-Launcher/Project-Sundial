// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use super::*;

#[test]
fn redaction_covers_tokens_split_across_reads_and_non_utf8_output() {
    let token = "access-token";
    let prefix = CHUNK_SIZE - 9;
    let mut input = vec![0xff; prefix];
    input.extend_from_slice(b"token:access-token:uuid\naccess-token");
    let sink = Mutex::new(Vec::new());
    copy(input.as_slice(), &sink, Some(token)).unwrap();
    let captured = sink.into_inner().unwrap();
    assert_eq!(&captured[..prefix], &input[..prefix]);
    assert_eq!(&captured[prefix..], b"token:[redacted]:uuid\n[redacted]");
}
