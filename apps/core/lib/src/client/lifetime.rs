use std::fs::File;
use std::io::{self, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::Event;
use crate::process::Process;

use super::{Running, State, WriteFrame};

const STOP_TIMEOUT: Duration = Duration::from_secs(5);
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(1);
const WAIT_INTERVAL: Duration = Duration::from_millis(20);

impl<Reply> Running<Reply> {
    pub(super) fn reap(
        &self,
        mut process: Process,
        scratch: tempfile::TempDir,
        closed: &AtomicBool,
        writer_lock: Option<Arc<File>>,
    ) {
        self.wait_process(&mut process, closed);
        let cleanup = scratch.close().and_then(|()| process.check_exit());
        drop(process);
        drop(writer_lock);
        let mut state = self.state.lock().expect("worker state");
        self.exited.send_replace(true);
        state.abandoned.clear();
        *self.reaped.lock().expect("worker reaper") =
            Some(cleanup.map_err(|error| error.to_string()));
        self.finished.notify_all();
    }

    fn wait_process(&self, process: &mut Process, closed: &AtomicBool) {
        let mut reported = false;
        loop {
            match process.poll_exit(self.should_kill(closed)) {
                Ok(true) => return,
                Err(error) => log_reaper_error(&mut reported, error),
                _ => (),
            }
            std::thread::sleep(WAIT_INTERVAL);
        }
    }

    fn should_kill(&self, closed: &AtomicBool) -> bool {
        if closed.load(Ordering::Acquire) {
            self.stop();
        }
        self.stop_expired()
    }

    pub(super) fn status(&self) -> Option<(bool, Reply)>
    where
        Reply: Clone,
    {
        let state = self.state.try_lock().ok()?;
        Some((state.stopping.is_none(), state.status.clone()?))
    }

    pub(super) fn running_id(&self) -> Option<u32> {
        self.state
            .lock()
            .ok()?
            .stopping
            .is_none()
            .then_some(self.pid)
    }

    pub(super) fn set_leases(&self, leases: usize) {
        let mut state = self.state.lock().expect("worker state");
        state.leases = leases;
        self.stop_if_idle(&mut state);
    }

    pub(super) fn wait_for_exit(&self) -> io::Result<()> {
        let reaped = self.reaped.lock().expect("worker reaper");
        let (reaped, _) = self
            .finished
            .wait_timeout_while(reaped, STOP_TIMEOUT + CLEANUP_TIMEOUT, |reaped| {
                reaped.is_none()
            })
            .expect("wait for worker");
        if let Some(result) = &*reaped {
            result.clone().map_err(io::Error::other)
        } else {
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the worker did not finish exiting",
            ))
        }
    }

    pub(super) fn is_reaped(&self) -> bool {
        self.reaped.try_lock().is_ok_and(|reaped| reaped.is_some())
    }

    pub(super) fn wait_until_reaped(&self) {
        let exit = self.reaped.lock().expect("worker reaper");
        drop(
            self.finished
                .wait_while(exit, |exit| exit.is_none())
                .expect("wait for worker"),
        );
    }

    fn stop_expired(&self) -> bool {
        self.state
            .lock()
            .expect("worker state")
            .stopping
            .is_some_and(|started| started.elapsed() >= STOP_TIMEOUT)
    }

    pub(super) fn stop_if_idle(&self, state: &mut State<Reply>) {
        if state.requests.is_empty() && !state.retained && state.leases == 0 {
            self.stop_locked(state);
        }
    }

    pub(super) fn stop(&self) {
        self.stop_locked(&mut self.state.lock().expect("worker state"));
    }

    fn stop_locked(&self, state: &mut State<Reply>) {
        if state.stopping.is_none() {
            state.stopping = Some(Instant::now());
            let _ = self.writer.send(WriteFrame::Close);
        }
    }

    pub(super) fn failed(&self) {
        let mut state = self.state.lock().expect("worker state");
        self.stop_locked(&mut state);
        let exited = *self.exited.borrow();
        let State {
            requests,
            abandoned,
            ..
        } = &mut *state;
        for (_, pending) in requests.drain() {
            abandoned.extend(pending.lock.filter(|_| !exited));
            let _ = pending.sender.send(Event::Disconnected);
        }
    }
}

fn log_reaper_error(reported: &mut bool, error: io::Error) {
    if !std::mem::replace(reported, true) {
        let _ = writeln!(
            io::stderr(),
            "Could not confirm embedded worker exit: {error}"
        );
    }
}
