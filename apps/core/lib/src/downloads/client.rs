//! Desktop-side routing and diagnostics for embedded file transfers.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use axlauncher_download_spi::{
    DownloadJob, DownloadOutcome, ExtraDigest, FileDownloader, MissingFile, TransferStats, batch,
};
use axlauncher_storage::DataDir;
use futures::Stream;

use crate::{Cancellation, Event, Image, Lease, Request, Service};

use super::{File, Reply, Route, Transfer};

static WORKER: OnceLock<Service<Reply>> = OnceLock::new();

pub(crate) fn configure(image: Image, executable: Result<PathBuf, String>) {
    let _ = WORKER.set(Service::new(image, executable));
}

pub(crate) fn close() {
    if let Some(worker) = WORKER.get() {
        worker.close();
    }
}

pub(crate) fn wait_closed() {
    if let Some(worker) = WORKER.get() {
        worker.wait_until_reaped();
    }
}

/// Reads cached worker telemetry without starting or keeping it alive.
pub fn stats() -> TransferStats {
    match WORKER.get().and_then(Service::status) {
        Some((true, stats)) => stats,
        Some((false, mut stats)) => {
            stats.active_connections = 0;
            stats
        }
        None => TransferStats::default(),
    }
}

#[derive(Clone)]
pub struct Downloads {
    data: DataDir,
    route: Route,
}

impl Downloads {
    pub fn new(data: DataDir, route: Route) -> Self {
        Self { data, route }
    }

    /// Keeps the shared process available across a multi-file operation without starting it.
    pub fn hold(&self) -> DownloadLease {
        DownloadLease {
            _lease: WORKER.get().map(Service::hold),
        }
    }

    pub(crate) fn start_guarded(
        &self,
        file: File,
        writer_lock: Arc<std::fs::File>,
    ) -> Result<Request<Reply>, String> {
        let worker = WORKER
            .get()
            .ok_or("The embedded download worker is unavailable.")?;
        worker.request_guarded(
            &self.data,
            Transfer::with_file(self.route.clone(), file),
            Some(writer_lock),
        )
    }

    /// Drain a relayed write to the genuine terminal reply or confirmed process exit.
    pub(crate) async fn finish_guarded(
        mut request: Request<Reply>,
        cancel: Cancellation,
    ) -> Result<(), String> {
        let reply = tokio::select! {
            biased;
            () = cancel.cancelled() => {
                request.cancel();
                request.next().await
            }
            reply = request.next() => reply,
        };
        drain_transfer(request, reply).await
    }
}

async fn drain_transfer(
    mut request: Request<Reply>,
    mut reply: Option<Event<Reply>>,
) -> Result<(), String> {
    let mut invalid = false;
    while matches!(reply, Some(Event::Reply(_))) {
        invalid = true;
        request.cancel();
        reply = request.next().await;
    }
    if let Some(Event::Finished(result)) = reply {
        return if invalid {
            Err("The download worker sent an invalid reply.".into())
        } else {
            result
        };
    }
    request.wait_for_process().await;
    Err("The download worker exited without a terminal reply.".into())
}

pub struct DownloadLease {
    _lease: Option<Lease<'static, Reply>>,
}

impl FileDownloader for Downloads {
    async fn download(
        &self,
        job: DownloadJob<'_>,
        digest: Option<ExtraDigest>,
    ) -> Result<(), String> {
        let worker = WORKER
            .get()
            .ok_or("The embedded download worker is unavailable.")?;
        let transfer =
            Transfer::new(self.route.clone(), job, digest).map_err(|error| error.to_string())?;
        let mut response = worker.request(&self.data, transfer)?;
        match response.next().await {
            Some(Event::Finished(result)) => result,
            _ => Err("The download worker sent an invalid reply.".into()),
        }
    }

    fn batch<'a>(
        &'a self,
        files: &'a [MissingFile],
        concurrency: usize,
    ) -> impl Stream<Item = DownloadOutcome> + 'a {
        let hold = self.hold();
        use futures::StreamExt;
        batch(self, files, concurrency).map(move |outcome| {
            let _ = &hold;
            outcome
        })
    }
}
