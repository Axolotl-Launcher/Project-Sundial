use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use axlauncher_storage::{DataDir, DataPath};

use crate::modrinth::{
    CatalogError, Command, Operation, Payload, Project, ProjectKey, Reply, Search, SearchPage,
    Version,
};
use crate::{Event, Image, Lease, Service};

type Worker = Arc<Service<Reply>>;
type Slots = HashMap<[PathBuf; 3], Worker>;
type Cooldown = tokio::sync::Mutex<Option<Instant>>;

static WORKERS: OnceLock<Workers> = OnceLock::new();

struct Workers {
    image: Image,
    executable: Result<PathBuf, String>,
    slots: Mutex<Option<Slots>>,
    cooldown: Cooldown,
}

impl Workers {
    fn new(image: Image, executable: Result<PathBuf, String>) -> Self {
        Self {
            image,
            executable,
            slots: Mutex::new(Some(HashMap::new())),
            cooldown: Cooldown::default(),
        }
    }

    fn for_data(&self, data: &DataDir) -> Result<Worker, CatalogError> {
        let mut slots = self.slots.lock().expect("Modrinth workers");
        let slots = slots.as_mut().ok_or(CatalogError::WorkerUnavailable)?;
        slots.retain(|_, worker| Arc::strong_count(worker) > 1 || !worker.is_reaped());
        let scope = [DataPath::Root, DataPath::Cache, DataPath::Profiles]
            .map(|path| data.path(path).to_owned());
        let worker = slots
            .entry(scope)
            .or_insert_with(|| Arc::new(Service::new(self.image, self.executable.clone())));
        Ok(Arc::clone(worker))
    }

    fn take(&self) -> Vec<Worker> {
        self.slots
            .lock()
            .expect("Modrinth workers")
            .take()
            .unwrap_or_default()
            .into_values()
            .collect()
    }
}

pub(crate) fn configure(image: Image, executable: Result<PathBuf, String>) {
    let _ = WORKERS.set(Workers::new(image, executable));
}

pub(crate) fn take_workers() -> Vec<Worker> {
    WORKERS.get().map_or_else(Vec::new, Workers::take)
}

/// Catalog access bound to a launcher data snapshot. Poll requests off the UI thread.
#[derive(Clone)]
pub struct DesktopModrinth {
    data: DataDir,
    worker: Worker,
    cooldown: &'static Cooldown,
}

impl DesktopModrinth {
    /// Resolves the scoped worker slot without starting a process.
    pub fn new(data: DataDir) -> Result<Self, CatalogError> {
        let workers = WORKERS.get().ok_or(CatalogError::WorkerUnavailable)?;
        let worker = workers.for_data(&data)?;
        Ok(Self {
            data,
            worker,
            cooldown: &workers.cooldown,
        })
    }

    /// Keeps a started worker alive for a group of requests in this scope.
    pub fn hold(&self) -> ModrinthLease<'_> {
        ModrinthLease {
            _lease: self.worker.hold(),
        }
    }

    pub async fn search(&self, search: Search) -> Result<SearchPage, CatalogError> {
        match self.execute(Operation::Search(search)).await? {
            Payload::Search(page) => Ok(page),
            _ => Err(CatalogError::WorkerProtocol),
        }
    }

    pub async fn get_project(&self, key: ProjectKey) -> Result<Project, CatalogError> {
        match self.execute(Operation::Project(key)).await? {
            Payload::Project(project) => Ok(project),
            _ => Err(CatalogError::WorkerProtocol),
        }
    }

    pub async fn list_versions(&self, key: ProjectKey) -> Result<Vec<Version>, CatalogError> {
        match self.execute(Operation::Versions(key)).await? {
            Payload::Versions(versions) => Ok(versions),
            _ => Err(CatalogError::WorkerProtocol),
        }
    }

    async fn execute(&self, operation: Operation) -> Result<Payload, CatalogError> {
        operation.validate()?;
        let mut cooldown = self.cooldown.lock().await;
        check_cooldown(*cooldown, Instant::now())?;
        let mut request = self
            .worker
            .request(&self.data, Command::V1(operation))
            .map_err(|_| CatalogError::WorkerUnavailable)?;
        let (delay, result) = receive(async || request.next().await).await?.into_parts();
        *cooldown = delay.map(|seconds| Instant::now() + Duration::from_secs(seconds.into()));
        result
    }
}

/// An optional process lease for a group of Modrinth requests.
pub struct ModrinthLease<'a> {
    _lease: Lease<'a, Reply>,
}

fn check_cooldown(cooldown: Option<Instant>, now: Instant) -> Result<(), CatalogError> {
    if let Some(until) = cooldown.filter(|until| *until > now) {
        let remaining = until - now;
        let seconds = remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0);
        return Err(CatalogError::RateLimited {
            retry_after_seconds: u32::try_from(seconds).unwrap_or(u32::MAX),
        });
    }
    Ok(())
}

async fn receive(
    mut next: impl AsyncFnMut() -> Option<Event<Reply>>,
) -> Result<Reply, CatalogError> {
    let reply = match next().await {
        Some(Event::Reply(reply)) => reply,
        Some(Event::Disconnected) | None => return Err(CatalogError::WorkerUnavailable),
        _ => return Err(CatalogError::WorkerProtocol),
    };
    match next().await {
        Some(Event::Finished(Ok(()))) => Ok(reply),
        Some(Event::Disconnected) | None => Err(CatalogError::WorkerUnavailable),
        _ => Err(CatalogError::WorkerProtocol),
    }
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/lib/src/modrinth_client.rs"]
mod tests;
