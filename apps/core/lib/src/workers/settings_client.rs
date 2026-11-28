use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use axlauncher_storage::{DataDir, DataPath};

use crate::instance_settings::{Edit, Reply};
use crate::{Cancellation, Event, Image, Service};

type Worker = Arc<Service<Reply>>;
type ScopeKey = (PathBuf, PathBuf, PathBuf);
type ScopeSlots = HashMap<ScopeKey, Worker>;

static WORKERS: OnceLock<Workers> = OnceLock::new();

struct Workers {
    image: Image,
    executable: Result<PathBuf, String>,
    slots: Mutex<Option<ScopeSlots>>,
}

impl Workers {
    fn for_data(&self, data: &DataDir) -> Result<Worker, String> {
        let mut slots = self.slots.lock().expect("settings workers");
        let slots = slots.as_mut().ok_or("The settings worker is closed.")?;
        slots.retain(|_, worker| Arc::strong_count(worker) > 1 || !worker.is_reaped());
        let root = |path| Path::to_owned(data.path(path));
        let worker = slots
            .entry((
                root(DataPath::Root),
                root(DataPath::Profiles),
                root(DataPath::Cache),
            ))
            .or_insert_with(|| Arc::new(Service::new(self.image, self.executable.clone())));
        Ok(Arc::clone(worker))
    }
}

pub(crate) fn configure(image: Image, executable: Result<PathBuf, String>) {
    let _ = WORKERS.set(Workers {
        image,
        executable,
        slots: Mutex::new(Some(HashMap::new())),
    });
}

pub(crate) fn take_workers() -> Vec<Worker> {
    let Some(workers) = WORKERS.get() else {
        return Vec::new();
    };
    workers
        .slots
        .lock()
        .expect("settings workers")
        .take()
        .unwrap_or_default()
        .into_values()
        .collect()
}

/// One edit is validated by a worker that never receives paths or documents.
pub async fn validate(data: &DataDir, edit: &Edit, cancel: &Cancellation) -> Result<Reply, String> {
    if !cancel.is_pending() {
        return Err("The settings edit was cancelled.".into());
    }
    let worker = WORKERS
        .get()
        .ok_or("The embedded settings worker is unavailable.")?
        .for_data(data)?;
    let mut request = worker.request(data, edit)?;
    tokio::select! {
        biased;
        () = cancel.cancelled() => Err("The settings edit was cancelled.".into()),
        reply = receive(async || request.next().await) => reply,
    }
}

async fn receive(mut next: impl AsyncFnMut() -> Option<Event<Reply>>) -> Result<Reply, String> {
    let mut validated = None;
    loop {
        match next().await {
            Some(Event::Reply(reply)) if validated.is_none() => validated = Some(reply),
            Some(Event::Finished(Ok(()))) if validated.is_some() => {
                return Ok(validated.expect("settings worker reply"));
            }
            Some(Event::Finished(Err(error))) => return Err(error),
            _ => return Err("The settings worker sent an invalid reply.".into()),
        }
    }
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/lib/src/settings_client.rs"]
mod tests;
