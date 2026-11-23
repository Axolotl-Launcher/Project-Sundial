// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::io::{self, Read, Write};
use std::sync::Mutex;

const CHUNK_SIZE: usize = 8192;
const REDACTED: &[u8] = b"[redacted]";

/// Streams redacted bytes to the log, keeping possible token prefixes between reads.
pub(super) fn copy(
    mut stream: impl Read,
    sink: &Mutex<impl Write>,
    token: Option<&str>,
) -> io::Result<()> {
    let token = token
        .filter(|token| !token.is_empty())
        .map_or(&[][..], str::as_bytes);
    let mut buffer = [0; CHUNK_SIZE];
    let mut pending = Vec::with_capacity(CHUNK_SIZE + token.len());
    loop {
        let count = stream.read(&mut buffer)?;
        pending.extend_from_slice(&buffer[..count]);
        let mut sink = sink.lock().expect("console log lock");
        let written = write_chunk(&pending, token, count == 0, &mut *sink)?;
        sink.flush()?;
        if count == 0 {
            return Ok(());
        }
        pending.drain(..written);
    }
}

fn write_chunk(
    bytes: &[u8],
    token: &[u8],
    finished: bool,
    sink: &mut impl Write,
) -> io::Result<usize> {
    if token.is_empty() {
        sink.write_all(bytes)?;
        return Ok(bytes.len());
    }
    let mut written = 0;
    while let Some(offset) = bytes[written..]
        .windows(token.len())
        .position(|candidate| candidate == token)
    {
        let start = written + offset;
        sink.write_all(&bytes[written..start])?;
        sink.write_all(REDACTED)?;
        written = start + token.len();
    }
    let end = if finished {
        bytes.len()
    } else {
        bytes.len().saturating_sub(token.len() - 1).max(written)
    };
    sink.write_all(&bytes[written..end])?;
    Ok(end)
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/launch/src/process/console.rs"]
mod tests;
