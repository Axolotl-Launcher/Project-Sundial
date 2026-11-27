use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use axlauncher_storage::DataDir;
use serde::{Serialize, de::DeserializeOwned};
use tokio::sync::{
    mpsc::{UnboundedSender, unbounded_channel},
    watch,
};

use crate::Image;
use crate::process::Process;
use crate::protocol::{self, Event, Input, Output};

mod cancellation;
mod lifetime;
mod pipe;
pub use cancellation::Cancellation;
pub use pipe::Request;
use pipe::{WriteFrame, write_requests};

const START_TIMEOUT: Duration = Duration::from_secs(10);
type Handshake = mpsc::Receiver<io::Result<()>>;

/// A single on-demand process slot for an embedded service.
pub struct Service<Reply> {
    image: Image,
    executable: Result<PathBuf, String>,
    slot: Mutex<Slot<Reply>>,
    closed: Arc<AtomicBool>,
}

struct Slot<Reply> {
    running: Option<Arc<Running<Reply>>>,
    leases: usize,
    start_failure: Option<String>,
    writer_lock: Option<Arc<File>>,
}

impl<Reply: DeserializeOwned + Send + 'static> Service<Reply> {
    pub(crate) fn new(image: Image, executable: Result<PathBuf, String>) -> Self {
        Self {
            image,
            executable,
            slot: Mutex::new(Slot {
                running: None,
                leases: 0,
                start_failure: None,
                writer_lock: None,
            }),
            closed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Starts the same executable that supplies the embedded images.
    pub fn for_executable(image: Image, executable: PathBuf) -> Self {
        Self::new(image, Ok(executable))
    }

    /// Starts the process if necessary. Call away from the UI thread.
    pub fn request(
        &self,
        data: &DataDir,
        command: impl Serialize,
    ) -> Result<Request<Reply>, String> {
        self.request_guarded(data, command, None)
    }

    pub(crate) fn with_writer_lock(mut self, lock: Arc<File>) -> Self {
        self.slot.get_mut().expect("worker slot").writer_lock = Some(lock);
        self
    }

    pub(crate) fn request_guarded(
        &self,
        data: &DataDir,
        command: impl Serialize,
        lock: Option<Arc<File>>,
    ) -> Result<Request<Reply>, String> {
        let mut slot = self.slot.lock().expect("worker slot");
        if self.closed.load(Ordering::Acquire) {
            return Err("The worker is closed.".into());
        }
        if let Some(error) = &slot.start_failure {
            return Err(error.clone());
        }
        if let Some(running) = &slot.running
            && let Some(request) = running.enqueue(&command, lock.clone())?
        {
            return Ok(request);
        }
        if let Some(previous) = &slot.running {
            previous
                .wait_for_exit()
                .map_err(|error| error.to_string())?;
        }
        let (running, handshake) = Running::start(
            self.image,
            data,
            self.executable.as_deref().map_err(Clone::clone)?,
            slot.leases,
            Arc::clone(&self.closed),
            slot.writer_lock.take(),
        )
        .map_err(|error| slot.remember_start_failure(error))?;
        slot.running = Some(Arc::clone(&running));
        if let Err(error) = wait_for_ready(handshake) {
            running.stop();
            let _ = running.wait_for_exit();
            return Err(slot.remember_start_failure(error));
        }
        if self.closed.load(Ordering::Acquire) {
            running.stop();
            return Err("The worker is closed.".into());
        }
        let request = running.enqueue(&command, lock);
        if !matches!(&request, Ok(Some(_))) {
            running.stop();
        }
        request?.ok_or_else(|| "The worker exited during startup.".into())
    }

    /// Schedules a request only if the original process can still receive it.
    pub fn notify_if_running(&self, command: impl Serialize) {
        if !self.closed.load(Ordering::Acquire)
            && let Ok(slot) = self.slot.try_lock()
            && let Some(running) = &slot.running
            && let Ok(Some(mut request)) = running.enqueue(&command, None)
        {
            request.finished = true;
        }
    }
}

impl<Reply> Slot<Reply> {
    fn remember_start_failure(&mut self, error: io::Error) -> String {
        let error = format!("Could not start the worker: {error}");
        if self.leases > 0 {
            self.start_failure = Some(error.clone());
        }
        error
    }
}

impl<Reply> Service<Reply> {
    pub fn status(&self) -> Option<(bool, Reply)>
    where
        Reply: Clone,
    {
        let slot = self.slot.try_lock().ok()?;
        slot.running.as_ref()?.status()
    }

    pub fn running_id(&self) -> Option<u32> {
        let slot = self.slot.try_lock().ok()?;
        slot.running.as_ref()?.running_id()
    }

    pub fn hold(&self) -> Lease<'_, Reply> {
        self.update_leases(|count| count + 1);
        Lease(self)
    }

    fn update_leases(&self, change: impl FnOnce(usize) -> usize) {
        let mut slot = self.slot.lock().expect("worker slot");
        slot.leases = change(slot.leases);
        if slot.leases == 0 {
            slot.start_failure = None;
        }
        if let Some(running) = &slot.running {
            running.set_leases(slot.leases);
        }
    }

    /// Permanently closes this slot without waiting on the UI thread.
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        if let Ok(slot) = self.slot.try_lock()
            && let Some(running) = &slot.running
        {
            running.stop();
        }
    }

    pub fn close_and_wait(&self) {
        if let Err(error) = self.close_checked() {
            eprintln!("Could not close an embedded worker: {error}");
        }
    }

    pub(crate) fn close_checked(&self) -> io::Result<()> {
        self.closed.store(true, Ordering::Release);
        let running = self.slot.lock().expect("worker slot").running.clone();
        let Some(running) = running else {
            return Ok(());
        };
        running.stop();
        running.wait_for_exit()
    }

    pub(crate) fn wait_until_reaped(&self) {
        self.close();
        let running = self.slot.lock().expect("worker slot").running.clone();
        if let Some(running) = running {
            running.wait_until_reaped();
        }
    }

    pub(crate) fn closed_and_reaped(&self) -> bool {
        self.closed.load(Ordering::Acquire)
            && self.slot.try_lock().is_ok_and(|slot| {
                slot.running
                    .as_ref()
                    .is_none_or(|running| running.is_reaped())
            })
    }
}

pub struct Lease<'a, Reply>(&'a Service<Reply>);

impl<Reply> Drop for Lease<'_, Reply> {
    fn drop(&mut self) {
        self.0.update_leases(|count| count - 1);
    }
}

impl<Reply> Drop for Service<Reply> {
    fn drop(&mut self) {
        self.close();
    }
}

struct Running<Reply> {
    pid: u32,
    writer: mpsc::Sender<WriteFrame>,
    state: Mutex<State<Reply>>,
    reaped: Mutex<Option<Result<(), String>>>,
    exited: watch::Sender<bool>,
    finished: std::sync::Condvar,
}

struct State<Reply> {
    requests: HashMap<u64, Pending<Reply>>,
    abandoned: Vec<Arc<File>>,
    next_id: u64,
    retained: bool,
    leases: usize,
    status: Option<Reply>,
    stopping: Option<Instant>,
}

struct Pending<Reply> {
    sender: UnboundedSender<Event<Reply>>,
    lock: Option<Arc<File>>,
}

impl<Reply: DeserializeOwned + Send + 'static> Running<Reply> {
    fn start(
        image: Image,
        data: &DataDir,
        executable: &Path,
        leases: usize,
        closed: Arc<AtomicBool>,
        writer_lock: Option<Arc<File>>,
    ) -> io::Result<(Arc<Self>, Handshake)> {
        let (scratch, path) = image.extract(data).map_err(|error| {
            io::Error::other(format!(
                "could not extract the bundled library to the launcher cache: {error}"
            ))
        })?;
        let (mut process, setup) = Process::start(image, executable, &path)?;
        let (input, output) = process.open_pipes();
        let (writer, outgoing) = mpsc::channel();
        let running = Arc::new(Self {
            pid: process.id(),
            writer,
            reaped: Mutex::new(None),
            exited: watch::Sender::new(false),
            finished: std::sync::Condvar::new(),
            state: Mutex::new(State {
                requests: HashMap::new(),
                abandoned: Vec::new(),
                next_id: 0,
                retained: false,
                leases,
                status: None,
                stopping: None,
            }),
        });
        let (ready, handshake) = mpsc::sync_channel(1);
        let reader = Arc::clone(&running);
        std::thread::spawn(move || reader.read_process(output, ready, setup));
        let reaper = Arc::clone(&running);
        std::thread::spawn(move || reaper.reap(process, scratch, &closed, writer_lock));
        let writer = Arc::clone(&running);
        std::thread::spawn(move || writer.write_process(input, outgoing));
        Ok((running, handshake))
    }

    fn read_process(
        &self,
        output: impl io::Read,
        ready: mpsc::SyncSender<io::Result<()>>,
        setup: io::Result<()>,
    ) {
        let mut output = BufReader::new(output);
        let handshake = setup.and_then(|()| read_handshake::<Reply>(&mut output));
        let started = handshake.is_ok();
        if ready.send(handshake).is_ok() && started {
            self.read_replies(&mut output);
        }
        self.failed();
    }

    fn write_process(&self, input: impl Write, outgoing: mpsc::Receiver<WriteFrame>) {
        if write_requests(input, outgoing).is_err() {
            self.failed();
        }
    }

    fn enqueue(
        &self,
        command: &impl Serialize,
        lock: Option<Arc<File>>,
    ) -> Result<Option<Request<Reply>>, String> {
        let mut state = self.state.lock().expect("worker state");
        if state.stopping.is_some() {
            return Ok(None);
        }
        let id = state.next_id;
        state.next_id = id.checked_add(1).expect("worker request ID");
        let frame =
            protocol::encode(&Input::Request { id, command }).map_err(|error| error.to_string())?;
        let (sender, receiver) = unbounded_channel();
        state.requests.insert(id, Pending { sender, lock });
        if self.writer.send(WriteFrame::Data(frame)).is_err() {
            state.requests.remove(&id);
            return Err("The worker pipe is closed.".into());
        }
        Ok(Some(Request {
            receiver,
            writer: self.writer.clone(),
            exited: self.exited.subscribe(),
            id,
            finished: false,
        }))
    }

    fn read_replies(&self, input: &mut impl io::Read) {
        while let Ok(Some(output)) = protocol::read(input) {
            match self.deliver_reply(output) {
                Some(()) => (),
                None => break,
            }
        }
    }

    fn deliver_reply(&self, output: Output<Reply>) -> Option<()> {
        let mut state = self.state.lock().expect("worker state");
        state.deliver(output)?;
        self.stop_if_idle(&mut state);
        Some(())
    }
}

impl<Reply> State<Reply> {
    fn deliver(&mut self, output: Output<Reply>) -> Option<()> {
        match output {
            Output::Ready | Output::StartupFailed(_) => return None,
            Output::Retain(retain) => self.retained = retain,
            Output::Status(status) => self.status = Some(status),
            Output::Event { id, reply } => {
                let _ = self.requests.get(&id)?.sender.send(Event::Reply(reply));
            }
            Output::Finished { id, result } => {
                let _ = self
                    .requests
                    .remove(&id)?
                    .sender
                    .send(Event::Finished(result));
            }
        }
        Some(())
    }
}

fn read_handshake<Reply: DeserializeOwned>(output: &mut impl io::Read) -> io::Result<()> {
    match protocol::read::<Output<Reply>>(output)? {
        Some(Output::Ready) => Ok(()),
        Some(Output::StartupFailed(error)) => Err(io::Error::other(error)),
        Some(_) => Err(io::Error::other(
            "unexpected message before the worker was ready",
        )),
        None => Err(io::Error::other(
            "worker exited before it was ready; restart the launcher if it was updated",
        )),
    }
}

fn wait_for_ready(handshake: Handshake) -> io::Result<()> {
    handshake
        .recv_timeout(START_TIMEOUT)
        .map_err(|error| io::Error::other(format!("waiting for the worker: {error}")))?
}
