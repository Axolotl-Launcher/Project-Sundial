use std::fs::File;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use axlauncher_storage::DataDir;

use crate::installation::{Command, Reply};
use crate::{Image, Request, Service};

static POOL: OnceLock<Pool> = OnceLock::new();

struct Pool {
    image: Image,
    executable: Result<PathBuf, String>,
    state: Mutex<Registry>,
}

#[derive(Default)]
struct Registry {
    closed: bool,
    active: Vec<Arc<Service<Reply>>>,
}

pub(crate) fn configure(image: Image, executable: Result<PathBuf, String>) {
    let _ = POOL.set(Pool {
        image,
        executable,
        state: Mutex::default(),
    });
}

pub(crate) fn close() -> impl FnOnce() {
    let active = POOL.get().map(Pool::close).unwrap_or_default();
    move || {
        for worker in active {
            worker.wait_until_reaped();
        }
    }
}

pub(super) fn start(lock: Arc<File>) -> Result<Worker, String> {
    POOL.get()
        .ok_or("The embedded installation worker is unavailable.")?
        .start(lock)
}

impl Pool {
    fn start(&'static self, lock: Arc<File>) -> Result<Worker, String> {
        let mut registry = self.state.lock().expect("install workers");
        if registry.closed {
            return Err("Installation is shutting down.".into());
        }
        registry.active.retain(|worker| !worker.closed_and_reaped());
        let service =
            Arc::new(Service::new(self.image, self.executable.clone()).with_writer_lock(lock));
        registry.active.push(Arc::clone(&service));
        Ok(Worker {
            service,
            pool: self,
        })
    }

    fn close(&self) -> Vec<Arc<Service<Reply>>> {
        let mut registry = self.state.lock().expect("install workers");
        registry.closed = true;
        let active = std::mem::take(&mut registry.active);
        for worker in &active {
            worker.close();
        }
        active
    }
}

pub(super) struct Worker {
    service: Arc<Service<Reply>>,
    pool: &'static Pool,
}

impl Worker {
    pub fn request(&self, data: &DataDir, command: &Command) -> Result<Request<Reply>, String> {
        self.service.request(data, command)
    }

    pub fn close(&self) {
        self.service.close();
    }

    pub fn finish(&self) -> io::Result<()> {
        self.service.close_checked()
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.close();
        let mut registry = self.pool.state.lock().expect("install workers");
        if self.service.closed_and_reaped() {
            registry
                .active
                .retain(|worker| !Arc::ptr_eq(worker, &self.service));
        }
    }
}
