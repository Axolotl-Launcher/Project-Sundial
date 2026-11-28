//! Host ownership of the game lock and the installation's relayed writers.

use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;

use axlauncher_install::prepare::Progress;
use axlauncher_storage::{DataDir, GameAccess};
use tokio::task::JoinSet;

use crate::downloads::Downloads;
use crate::{Cancellation, Event, Request};

use crate::installation::{Command, JavaPaths, Reply};

type TransferResult = (u64, Result<(), String>);

mod workers;
pub(crate) use workers::{close, configure};

/// Runs one install in a new worker and keeps the game lock through both drains.
pub async fn install(
    data: &DataDir,
    command: Command,
    downloads: Downloads,
    cancel: Cancellation,
    register: impl FnMut(PathBuf, u32) -> Result<JavaPaths, String>,
    report: impl FnMut(Progress),
) -> Result<String, String> {
    if !command.uses_data(data) {
        return Err("The installation request does not match the host's game layout.".into());
    }
    let lock = data
        .game()
        .lock(GameAccess::Install)
        .map(Arc::new)
        .map_err(|error| format!("Cannot lock shared game files: {error}"))?;
    let worker = workers::start(Arc::clone(&lock))?;
    let _lease = downloads.hold();
    let mut relays = Relays {
        downloads,
        lock,
        cancel: Cancellation::default(),
        tasks: JoinSet::new(),
    };
    let result = match worker.request(data, &command) {
        Ok(mut request) => {
            relays
                .receive(&mut request, &command, cancel, register, report)
                .await
        }
        Err(error) => Err(error),
    };
    worker.close();
    relays.drain().await;
    let cleanup = worker
        .finish()
        .map_err(|error| format!("Could not clean up the installation worker: {error}"));
    result.and_then(|version| cleanup.map(|()| version))
}

struct Relays {
    downloads: Downloads,
    lock: Arc<File>,
    cancel: Cancellation,
    tasks: JoinSet<TransferResult>,
}

impl Relays {
    async fn receive(
        &mut self,
        request: &mut Request<Reply>,
        command: &Command,
        cancel: Cancellation,
        mut register: impl FnMut(PathBuf, u32) -> Result<JavaPaths, String>,
        mut report: impl FnMut(Progress),
    ) -> Result<String, String> {
        let mut resolved = None;
        loop {
            tokio::select! {
                biased;
                () = cancel.cancelled() => return Err("Installation cancelled.".into()),
                transfer = self.tasks.join_next(), if !self.tasks.is_empty() => {
                    let (callback, result) = transfer.expect("active relay").map_err(|error| error.to_string())?;
                    request.respond(callback, result)?;
                }
                event = request.next() => match event {
                    Some(Event::Reply(Reply::Progress(progress))) if resolved.is_none() => report(progress),
                    Some(Event::Reply(Reply::Download { callback, file })) if resolved.is_none() => {
                        self.download(request, callback, file)?;
                    }
                    Some(Event::Reply(Reply::RegisterJava { callback, major, executable })) if resolved.is_none() => {
                        request.respond(callback, register(executable.into(), major))?;
                    }
                    Some(Event::Reply(Reply::Resolved(version))) if resolved.is_none() && command.accepts(&version) => {
                        resolved = Some(version);
                    }
                    Some(Event::Finished(result)) => {
                        result?;
                        if !self.tasks.is_empty() {
                            return Err("Installation finished with downloads still pending.".into());
                        }
                        return resolved.ok_or_else(|| "Installation did not return a resolved version.".into());
                    }
                    Some(Event::Reply(_)) => return Err("Invalid installation worker reply.".into()),
                    Some(Event::Disconnected) | None => return Err("Installation worker exited without finishing.".into()),
                }
            }
        }
    }

    fn download(
        &mut self,
        request: &Request<Reply>,
        callback: u64,
        file: crate::downloads::File,
    ) -> Result<(), String> {
        let transfer = match self.downloads.start_guarded(file, Arc::clone(&self.lock)) {
            Ok(transfer) => transfer,
            Err(error) => return request.respond(callback, Err::<(), _>(error)),
        };
        let cancel = self.cancel.clone();
        self.tasks
            .spawn(async move { (callback, Downloads::finish_guarded(transfer, cancel).await) });
        Ok(())
    }

    async fn drain(&mut self) {
        self.cancel.cancel();
        while self.tasks.join_next().await.is_some() {}
    }
}
