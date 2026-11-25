// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The desktop lifecycle of a Minecraft process.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axlauncher_core::install::{loader, vanilla};
use axlauncher_core::launch::{
    FlowError, LaunchRequest, ProcessLauncher, Target, flow, plan::Session,
};
use axlauncher_core::network::auth::AuthHttp;
use axlauncher_core::network::{MirrorSettings, ReqwestNetwork};
use axlauncher_core::{launch_accounts, state::accounts::AccountId};
use axlauncher_ui::{AppContext, AsyncApp, Context, WeakEntity};
use tokio::sync::{oneshot, watch};

use crate::RootView;
use crate::panel::accounts::DesktopAccounts;

pub(crate) enum Player {
    Offline(Session),
    Online {
        service: Arc<DesktopAccounts>,
        id: AccountId,
        fallen_proxy: bool,
    },
}

pub(crate) enum Launch {
    Starting,
    Running,
    Exited,
    NeedsInstall(String),
    Failed(String),
}

impl Launch {
    pub(crate) fn is_active(&self) -> bool {
        matches!(self, Self::Starting | Self::Running)
    }

    pub(crate) fn message(&self) -> &str {
        match self {
            Self::Starting => "Checking files and starting Minecraft",
            Self::Running => "Minecraft running",
            Self::Exited => "Minecraft closed",
            Self::NeedsInstall(error) | Self::Failed(error) => error,
        }
    }
}

pub(crate) fn start(
    request: LaunchRequest,
    player: Player,
    vanilla: bool,
    data_root: PathBuf,
    mirrors: MirrorSettings,
    cx: &mut Context<'_, RootView>,
) {
    let root = request.game_root.clone();
    let directory = request.instance_directory.clone();
    let version = request.version_id.clone();
    let (sender, mut started) = watch::channel(false);
    let (resolved, account) = oneshot::channel();
    let task = cx.background_spawn(async move {
        match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime.block_on(run(
                request, player, vanilla, &data_root, mirrors, sender, resolved,
            )),
            Err(error) => Launch::Failed(error.to_string()),
        }
    });
    cx.spawn(async move |view, cx| {
        if let Ok(service) = account.await {
            let _ = view.update(cx, |view, cx| view.sync_launch_accounts(&service, cx));
        }
        if started.changed().await.is_ok() {
            show_running(&view, &directory, &version, cx);
        }
        let outcome = task.await;
        let _ = view.update(cx, |view, cx| {
            finish_launch(view, &root, directory, &version, outcome, cx)
        });
    })
    .detach();
}

fn show_running(view: &WeakEntity<RootView>, directory: &Path, version: &str, cx: &mut AsyncApp) {
    let _ = view.update(cx, |view, cx| {
        view.set_launch(directory, version, Launch::Running, cx)
    });
}

fn finish_launch(
    view: &mut RootView,
    root: &Path,
    directory: PathBuf,
    version: &str,
    outcome: Launch,
    cx: &mut Context<'_, RootView>,
) {
    if matches!(outcome, Launch::NeedsInstall(_)) {
        view.clear_completed_installation(root, &directory);
    }
    view.set_launch(&directory, version, outcome, cx);
}

async fn run(
    request: LaunchRequest,
    player: Player,
    vanilla: bool,
    data_root: &Path,
    mirrors: MirrorSettings,
    sender: watch::Sender<bool>,
    resolved: oneshot::Sender<Arc<DesktopAccounts>>,
) -> Launch {
    let session = match player {
        Player::Offline(session) => {
            drop(resolved);
            session
        }
        Player::Online {
            service,
            id,
            fallen_proxy,
        } => {
            let result = online_session(&service, id, mirrors, fallen_proxy).await;
            let _ = resolved.send(service);
            match result {
                Ok(session) => session,
                Err(error) => return Launch::Failed(error),
            }
        }
    };
    let lock = match vanilla::launch_lock(&request.game_root) {
        Ok(lock) => std::sync::Mutex::new(Some(lock)),
        Err(error) => return Launch::Failed(error),
    };
    let check_files = if vanilla {
        vanilla::check_files
    } else {
        loader::check_files
    };
    if let Err(error) = check_files(&request.game_root, &request.version_id, &Target::host()) {
        return Launch::NeedsInstall(format!("Install game files to continue. {error}"));
    }
    let launcher = ProcessLauncher::reporting(move || {
        drop(lock.lock().expect("launch guard").take());
        sender.send_replace(true);
    });
    match flow::launch_with_session(
        &crate::java::managed(data_root),
        &launcher,
        request,
        &session,
    )
    .await
    {
        Ok(_) => Launch::Exited,
        Err(FlowError::NoMatchingJava { required, .. }) => Launch::Failed(format!(
            "Java {required} is required. Set JAVA_HOME or PATH to a Java {required} installation and reopen the launcher."
        )),
        Err(error) => Launch::Failed(error.to_string()),
    }
}

async fn online_session(
    service: &DesktopAccounts,
    id: AccountId,
    mirrors: MirrorSettings,
    fallen_proxy: bool,
) -> Result<Session, String> {
    let network = AuthHttp::desktop()
        .map_err(|_| "Could not initialize online sign-in.".to_owned())?
        .with_fallen_proxy(fallen_proxy);
    launch_accounts::prepare(service, id, &network, || {
        ReqwestNetwork::desktop_with_mirrors(mirrors)
            .map_err(|_| "Could not initialize the authentication agent download.".to_owned())
    })
    .await
    .map(|session| session.with_fallen_proxy(fallen_proxy))
}
