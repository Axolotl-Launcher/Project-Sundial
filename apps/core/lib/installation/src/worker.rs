use std::cell::Cell;
use std::path::PathBuf;

use axlauncher_download_spi::{DownloadJob, ExtraDigest, FileDownloader};
use axlauncher_install::prepare::{Prepared, Progress};
use axlauncher_launch::{Java, JavaMode, SystemJava, Target, version};
use axlauncher_network::ReqwestNetwork;
use axlauncher_spi::{LoaderKind, LoaderRequest};
use axlauncher_storage::{DataDir, DataPath};
use axlauncher_worker::downloads::File;
use axlauncher_worker::installation::{Command, JavaPaths, Reply};
use axlauncher_worker::{Callbacks, Cancellation, Responder, serve_with_callbacks};

use crate::{loader, runtime, vanilla};

#[allow(
    unsafe_code,
    reason = "The launcher resolves a fixed versioned C ABI export."
)]
#[unsafe(no_mangle)]
pub extern "C" fn axlauncher_install_v1() -> i32 {
    let used = Cell::new(false);
    serve_with_callbacks(move |command, reply, cancel, callbacks| {
        let already_used = used.replace(true);
        handle_once(already_used, command, Relay { reply, callbacks }, cancel)
    })
}

async fn handle_once(
    already_used: bool,
    command: Command,
    relay: Relay,
    cancel: Cancellation,
) -> Result<(), String> {
    if already_used {
        return Err("An installation worker handles only one request.".into());
    }
    let reply = relay.reply;
    let resolved = install(command, relay, &cancel).await?;
    reply.send(Reply::Resolved(resolved))
}

struct Relay {
    reply: Responder<Reply>,
    callbacks: Callbacks,
}

impl Relay {
    fn progress(&self, progress: Progress) {
        let _ = self.reply.send(Reply::Progress(progress));
    }

    async fn register(&self, executable: PathBuf, major: u32) -> Result<JavaPaths, String> {
        self.callbacks
            .request::<Result<JavaPaths, String>>(|callback| {
                self.reply.send(Reply::RegisterJava {
                    callback,
                    major,
                    executable: executable.into(),
                })
            })
            .await?
    }
}

impl FileDownloader for Relay {
    async fn download(
        &self,
        job: DownloadJob<'_>,
        digest: Option<ExtraDigest>,
    ) -> Result<(), String> {
        let file = File::new(job, digest).map_err(|error| error.to_string())?;
        self.callbacks
            .request::<Result<(), String>>(|callback| {
                self.reply.send(Reply::Download { callback, file })
            })
            .await?
    }
}

async fn install(command: Command, relay: Relay, cancel: &Cancellation) -> Result<String, String> {
    let (data, target, mirrors, concurrency, java) =
        command.into_install().map_err(|error| error.to_string())?;
    let network =
        ReqwestNetwork::desktop_with_mirrors(mirrors).map_err(|error| error.to_string())?;
    let install = Install {
        data,
        network,
        relay,
        concurrency,
        cancel,
    };
    let java = install.java(java);
    let minecraft = target.game_version();
    match target.loader() {
        Some((kind, version)) => {
            install
                .loader(
                    kind,
                    LoaderRequest {
                        game_version: minecraft.to_owned(),
                        loader_version: version.map(str::to_owned),
                    },
                    &java,
                )
                .await
        }
        None => install.vanilla(minecraft, &java).await,
    }
}

struct Install<'a> {
    data: DataDir,
    network: ReqwestNetwork,
    relay: Relay,
    concurrency: usize,
    cancel: &'a Cancellation,
}

impl Install<'_> {
    fn java(&self, java: JavaPaths) -> SystemJava {
        let (registered, chosen) = java.into_paths();
        SystemJava::for_registered_paths(vec![self.data.path(DataPath::JavaVersions).to_owned()])
            .with_chosen(chosen)
            .with_registered(registered)
    }

    async fn vanilla(&self, id: &str, java: &SystemJava) -> Result<String, String> {
        let prepared = self
            .cancellable(vanilla::install(
                &self.network,
                &self.relay,
                self.data.game(),
                id,
                &Target::host(),
                self.concurrency,
                |progress| self.relay.progress(progress),
            ))
            .await?;
        ensure_complete(&prepared)?;
        let document = version::load(self.data.game(), id).map_err(|error| {
            format!("Cannot read the installed Minecraft {id} manifest: {error}")
        })?;
        self.ensure_java(id, &document, java).await?;
        Ok(id.to_owned())
    }

    async fn ensure_java(
        &self,
        id: &str,
        document: &axlauncher_launch::Version,
        java: &SystemJava,
    ) -> Result<(), String> {
        let Some(major) = document.java_version.as_ref().map(|java| java.major) else {
            return Ok(());
        };
        if java.executable_for(major, JavaMode::Windowed).is_some() {
            return Ok(());
        }
        self.provision(major).await.map(|_| ()).map_err(|error| {
            format!(
                "Minecraft {id} needs Java {major}, and its runtime could not be installed: {error}"
            )
        })
    }

    async fn loader(
        &self,
        kind: LoaderKind,
        request: LoaderRequest,
        java: &SystemJava,
    ) -> Result<String, String> {
        let provider = axlauncher_loader::provider(self.network.clone(), kind)
            .map_err(|error| error.to_string())?;
        let operation = loader::InstallRequest {
            network: &self.network,
            downloader: &self.relay,
            loader: &provider,
            java,
            game_root: self.data.game(),
            request,
            target: &Target::host(),
            concurrency: self.concurrency,
        };
        let report = |progress| self.relay.progress(progress);
        let installed = match operation.install_cancellable(self.cancel, report).await {
            Err(loader::LoaderInstallError::Java { required }) => {
                let refreshed = self.provision(required).await?;
                loader::InstallRequest {
                    java: &refreshed,
                    ..operation
                }
                .install_cancellable(self.cancel, report)
                .await
            }
            result => result,
        }
        .map_err(|error| error.to_string())?;
        Ok(installed.version_id)
    }

    async fn provision(&self, major: u32) -> Result<SystemJava, String> {
        self.relay.progress(Progress::Java);
        self.cancellable(async {
            let ready = runtime::install(
                &self.network,
                self.data.path(DataPath::JavaVersions),
                major,
                |stage| self.report_java(stage),
            )
            .await
            .map_err(|error| error.to_string())?;
            let refreshed = self.relay.register(ready.java, major).await?;
            Ok(self.java(refreshed))
        })
        .await
    }

    fn report_java(&self, stage: Progress) {
        self.relay.progress(match stage {
            Progress::Downloading { .. } => stage,
            _ => Progress::Java,
        });
    }

    async fn cancellable<T>(
        &self,
        operation: impl Future<Output = Result<T, String>>,
    ) -> Result<T, String> {
        tokio::select! {
            biased;
            () = self.cancel.cancelled() => Err("Installation cancelled.".into()),
            result = operation => result,
        }
    }
}

fn ensure_complete(prepared: &Prepared) -> Result<(), String> {
    match prepared.failures.first() {
        None => Ok(()),
        Some(first) => Err(format!(
            "Could not prepare {} file(s). {first}",
            prepared.failures.len()
        )),
    }
}
