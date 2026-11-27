use std::collections::HashMap;
use std::io::{self, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use axlauncher_storage::DataDir;
use serde::{Serialize, de::DeserializeOwned};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

use crate::Image;
use crate::protocol::{self, Event, Input, Output};

const START_TIMEOUT: Duration = Duration::from_secs(10);
const STOP_TIMEOUT: Duration = Duration::from_secs(5);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(1);
const WAIT_INTERVAL: Duration = Duration::from_millis(20);
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
        let mut slot = self.slot.lock().expect("worker slot");
        if self.closed.load(Ordering::Acquire) {
            return Err("The worker is closed.".into());
        }
        if let Some(error) = &slot.start_failure {
            return Err(error.clone());
        }
        if let Some(running) = &slot.running
            && let Some(request) = running.enqueue(&command)?
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
        let request = running.enqueue(&command);
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
            && let Ok(Some(mut request)) = running.enqueue(&command)
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
        self.closed.store(true, Ordering::Release);
        let running = self.slot.lock().expect("worker slot").running.clone();
        let Some(running) = running else { return };
        running.stop();
        if let Err(error) = running.wait_for_exit() {
            eprintln!("Could not close an embedded worker: {error}");
        }
    }

    pub(crate) fn closed_and_reaped(&self) -> bool {
        self.closed.load(Ordering::Acquire) && self.is_reaped()
    }

    pub(crate) fn is_reaped(&self) -> bool {
        self.slot.try_lock().is_ok_and(|slot| {
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

/// Reply stream and cancellation handle for one in-flight request.
pub struct Request<Reply> {
    receiver: UnboundedReceiver<Event<Reply>>,
    writer: mpsc::Sender<WriteFrame>,
    id: u64,
    finished: bool,
}

impl<Reply> Request<Reply> {
    pub async fn next(&mut self) -> Option<Event<Reply>> {
        let event = self.receiver.recv().await;
        self.finished |= matches!(event, Some(Event::Finished(_)) | None);
        event
    }

    pub fn cancel(&self) {
        if !self.finished
            && let Ok(frame) = protocol::encode(&Input::<()>::Cancel { id: self.id })
        {
            let _ = self.writer.send(WriteFrame::Data(frame));
        }
    }
}

impl<Reply> Drop for Request<Reply> {
    fn drop(&mut self) {
        self.cancel();
    }
}

enum WriteFrame {
    Data(Vec<u8>),
    Close,
}

struct Running<Reply> {
    pid: u32,
    writer: mpsc::Sender<WriteFrame>,
    state: Mutex<State<Reply>>,
    reaped: Mutex<bool>,
    finished: std::sync::Condvar,
}

struct State<Reply> {
    requests: HashMap<u64, UnboundedSender<Event<Reply>>>,
    next_id: u64,
    retained: bool,
    leases: usize,
    status: Option<Reply>,
    stopping: Option<Instant>,
}

impl<Reply: DeserializeOwned + Send + 'static> Running<Reply> {
    fn start(
        image: Image,
        data: &DataDir,
        executable: &Path,
        leases: usize,
        closed: Arc<AtomicBool>,
    ) -> io::Result<(Arc<Self>, Handshake)> {
        let (scratch, path) = image.extract(data).map_err(|error| {
            io::Error::other(format!(
                "could not extract the bundled library to the launcher cache: {error}"
            ))
        })?;
        let mut child = image.spawn(executable, &path)?;
        let input = child.stdin.take().expect("piped worker input");
        let output = child.stdout.take().expect("piped worker output");
        let (writer, outgoing) = mpsc::channel();
        let running = Arc::new(Self {
            pid: child.id(),
            writer,
            reaped: Mutex::new(false),
            finished: std::sync::Condvar::new(),
            state: Mutex::new(State {
                requests: HashMap::new(),
                next_id: 0,
                retained: false,
                leases,
                status: None,
                stopping: None,
            }),
        });
        let (ready, handshake) = mpsc::sync_channel(1);
        let reader = Arc::clone(&running);
        std::thread::spawn(move || reader.read_process(output, ready));
        let reaper = Arc::clone(&running);
        std::thread::spawn(move || reaper.reap(child, scratch, &closed));
        let writer = Arc::clone(&running);
        std::thread::spawn(move || writer.write_process(input, outgoing));
        Ok((running, handshake))
    }

    fn read_process(&self, output: impl io::Read, ready: mpsc::SyncSender<io::Result<()>>) {
        let mut output = BufReader::new(output);
        let handshake = read_handshake::<Reply>(&mut output);
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

    fn enqueue(&self, command: &impl Serialize) -> Result<Option<Request<Reply>>, String> {
        let mut state = self.state.lock().expect("worker state");
        if state.stopping.is_some() {
            return Ok(None);
        }
        let id = state.next_id;
        state.next_id = id.checked_add(1).expect("worker request ID");
        let frame =
            protocol::encode(&Input::Request { id, command }).map_err(|error| error.to_string())?;
        let (sender, receiver) = unbounded_channel();
        self.writer
            .send(WriteFrame::Data(frame))
            .map_err(|_| "The worker pipe is closed.".to_owned())?;
        state.requests.insert(id, sender);
        Ok(Some(Request {
            receiver,
            writer: self.writer.clone(),
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

    fn reap(&self, mut child: Child, scratch: tempfile::TempDir, closed: &AtomicBool) {
        let exited = loop {
            match child.try_wait() {
                Ok(None) if self.keep_running(closed) => std::thread::sleep(WAIT_INTERVAL),
                status => break matches!(status, Ok(Some(_))),
            }
        };
        if !exited {
            let _ = child.kill();
        }
        let _ = child.wait();
        let _ = scratch.close();
        *self.reaped.lock().expect("worker reaper") = true;
        self.finished.notify_all();
    }
}

impl<Reply> State<Reply> {
    fn deliver(&mut self, output: Output<Reply>) -> Option<()> {
        match output {
            Output::Ready | Output::StartupFailed(_) => return None,
            Output::Retain(retain) => self.retained = retain,
            Output::Status(status) => self.status = Some(status),
            Output::Event { id, reply } => {
                let _ = self.requests.get(&id)?.send(Event::Reply(reply));
            }
            Output::Finished { id, result } => {
                let _ = self.requests.remove(&id)?.send(Event::Finished(result));
            }
        }
        Some(())
    }
}

impl<Reply> Running<Reply> {
    fn keep_running(&self, closed: &AtomicBool) -> bool {
        if closed.load(Ordering::Acquire) {
            self.stop();
        }
        !self.stop_expired()
    }

    fn status(&self) -> Option<(bool, Reply)>
    where
        Reply: Clone,
    {
        let state = self.state.try_lock().ok()?;
        Some((state.stopping.is_none(), state.status.clone()?))
    }

    fn running_id(&self) -> Option<u32> {
        self.state
            .lock()
            .ok()?
            .stopping
            .is_none()
            .then_some(self.pid)
    }

    fn set_leases(&self, leases: usize) {
        let mut state = self.state.lock().expect("worker state");
        state.leases = leases;
        self.stop_if_idle(&mut state);
    }

    fn wait_for_exit(&self) -> io::Result<()> {
        let reaped = self.reaped.lock().expect("worker reaper");
        let (reaped, _) = self
            .finished
            .wait_timeout_while(reaped, STOP_TIMEOUT + CLEANUP_TIMEOUT, |reaped| !*reaped)
            .expect("wait for worker");
        if *reaped {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the worker did not finish exiting",
            ))
        }
    }

    fn is_reaped(&self) -> bool {
        self.reaped.try_lock().is_ok_and(|reaped| *reaped)
    }

    fn stop_expired(&self) -> bool {
        self.state
            .lock()
            .expect("worker state")
            .stopping
            .is_some_and(|started| started.elapsed() >= STOP_TIMEOUT)
    }

    fn stop_if_idle(&self, state: &mut State<Reply>) {
        if state.requests.is_empty() && !state.retained && state.leases == 0 {
            self.stop_locked(state);
        }
    }

    fn stop(&self) {
        self.stop_locked(&mut self.state.lock().expect("worker state"));
    }

    fn stop_locked(&self, state: &mut State<Reply>) {
        if state.stopping.is_none() {
            state.stopping = Some(Instant::now());
            let _ = self.writer.send(WriteFrame::Close);
        }
    }

    fn failed(&self) {
        let mut state = self.state.lock().expect("worker state");
        self.stop_locked(&mut state);
        for (_, sender) in state.requests.drain() {
            let _ = sender.send(Event::Finished(Err(
                "The worker exited or sent an invalid reply. Try again.".into(),
            )));
        }
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

fn write_requests(mut input: impl Write, outgoing: mpsc::Receiver<WriteFrame>) -> io::Result<()> {
    for message in outgoing {
        match message {
            WriteFrame::Data(frame) => input.write_all(&frame)?,
            WriteFrame::Close => return protocol::write(&mut input, &Input::<()>::Shutdown),
        }
    }
    Ok(())
}
