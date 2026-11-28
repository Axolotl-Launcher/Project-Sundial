use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

const MAX_FRAME: usize = 16 * 1024 * 1024;

#[derive(Deserialize, Serialize)]
pub(super) enum Input<Command> {
    Request {
        id: u64,
        command: Command,
    },
    Callback {
        id: u64,
        callback: u64,
        response: serde_json::Value,
    },
    Cancel {
        id: u64,
    },
    Shutdown,
}

#[derive(Deserialize, Serialize)]
pub(super) enum Output<Reply> {
    Ready,
    StartupFailed(String),
    Event { id: u64, reply: Reply },
    Finished { id: u64, result: Result<(), String> },
    Retain(bool),
    Status(Reply),
}

pub enum Event<Reply> {
    Reply(Reply),
    Finished(Result<(), String>),
    Disconnected,
}

pub(super) fn encode(value: &impl Serialize) -> io::Result<Vec<u8>> {
    let mut frame = vec![0; size_of::<u32>()];
    serde_json::to_writer(&mut frame, value)
        .map_err(|_| invalid("could not encode a worker message"))?;
    let length = frame.len() - size_of::<u32>();
    if length > MAX_FRAME {
        return Err(invalid("worker message exceeds the frame limit"));
    }
    frame[..size_of::<u32>()].copy_from_slice(&(length as u32).to_le_bytes());
    Ok(frame)
}

pub(super) fn write(output: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    output.write_all(&encode(value)?)?;
    output.flush()
}

pub(super) fn read<T: DeserializeOwned>(input: &mut impl Read) -> io::Result<Option<T>> {
    let mut length = [0; size_of::<u32>()];
    if input.read(&mut length[..1])? == 0 {
        return Ok(None);
    }
    input.read_exact(&mut length[1..])?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(invalid("invalid worker message length"));
    }
    let mut frame = vec![0; length];
    input.read_exact(&mut frame)?;
    serde_json::from_slice(&frame)
        .map(Some)
        .map_err(|_| invalid("invalid worker message"))
}

fn invalid(message: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.to_string())
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/lib/src/protocol.rs"]
mod tests;
