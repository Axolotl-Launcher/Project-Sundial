use std::io::{self, Write};
use std::sync::mpsc;

use serde::Serialize;
use tokio::sync::{mpsc::UnboundedReceiver, watch};

use crate::protocol::{self, Event, Input};

/// Reply stream and cancellation handle for one in-flight request.
pub struct Request<Reply> {
    pub(super) receiver: UnboundedReceiver<Event<Reply>>,
    pub(super) writer: mpsc::Sender<WriteFrame>,
    pub(super) exited: watch::Receiver<bool>,
    pub(super) id: u64,
    pub(super) finished: bool,
}

impl<Reply> Request<Reply> {
    pub async fn next(&mut self) -> Option<Event<Reply>> {
        let event = self.receiver.recv().await;
        self.finished |= !matches!(event, Some(Event::Reply(_)));
        event
    }

    pub fn cancel(&self) {
        if !self.finished
            && let Ok(frame) = protocol::encode(&Input::<()>::Cancel { id: self.id })
        {
            let _ = self.writer.send(WriteFrame::Data(frame));
        }
    }

    pub fn respond(&self, callback: u64, response: impl Serialize) -> Result<(), String> {
        let response = serde_json::to_value(response).map_err(|error| error.to_string())?;
        let frame = protocol::encode(&Input::<()>::Callback {
            id: self.id,
            callback,
            response,
        })
        .map_err(|error| error.to_string())?;
        self.writer
            .send(WriteFrame::Data(frame))
            .map_err(|_| "The worker pipe is closed.".into())
    }

    /// A lost pipe is not proof the worker stopped writing.
    pub(crate) async fn wait_for_process(&mut self) {
        let lost = self.exited.wait_for(|exited| *exited).await.is_err();
        if lost {
            std::future::pending::<()>().await;
        }
    }
}

impl<Reply> Drop for Request<Reply> {
    fn drop(&mut self) {
        self.cancel();
    }
}

pub(super) enum WriteFrame {
    Data(Vec<u8>),
    Close,
}

pub(super) fn write_requests(
    mut input: impl Write,
    outgoing: mpsc::Receiver<WriteFrame>,
) -> io::Result<()> {
    for message in outgoing {
        match message {
            WriteFrame::Data(frame) => input.write_all(&frame)?,
            WriteFrame::Close => return protocol::write(&mut input, &Input::<()>::Shutdown),
        }
    }
    Ok(())
}
