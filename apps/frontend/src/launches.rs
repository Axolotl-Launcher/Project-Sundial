// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! The desktop lifecycle of a Minecraft process.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axlauncher_core::install::{auth_agent, vanilla};
use axlauncher_core::launch::plan::YggdrasilAgent;
use axlauncher_core::launch::{
    FlowError, LaunchRequest, ProcessLauncher, SystemJava, Target, flow, plan::Session,
};
use axlauncher_core::network::ReqwestNetwork;
use axlauncher_core::network::auth::AuthHttp;
use axlauncher_core::state::accounts::{AccountId, LaunchIdentity, yggdrasil};
use axlauncher_core::storage::DataDir;
use axlauncher_ui::{AppContext, AsyncApp, Context, WeakEntity};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use tokio::sync::{oneshot, watch};

use crate::RootView;
use crate::panel::accounts::DesktopAccounts;

pub(crate) enum Player {
    Offline(Session),
    Online(Arc<DesktopAccounts>, AccountId),
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
            Ok(runtime) => runtime.block_on(run(request, player, vanilla, sender, resolved)),
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
        view.clear_completed_installation(root, version);
    }
    view.set_launch(&directory, version, outcome, cx);
}

async fn run(
    request: LaunchRequest,
    player: Player,
    vanilla: bool,
    sender: watch::Sender<bool>,
    resolved: oneshot::Sender<Arc<DesktopAccounts>>,
) -> Launch {
    if vanilla
        && let Err(error) =
            vanilla::check_files(&request.game_root, &request.version_id, &Target::host())
    {
        return Launch::NeedsInstall(format!("Install game files to continue. {error}"));
    }
    let session = match player {
        Player::Offline(session) => {
            drop(resolved);
            session
        }
        Player::Online(service, id) => {
            let result = online_session(&service, id, &request.game_root).await;
            let _ = resolved.send(service);
            match result {
                Ok(session) => session,
                Err(error) => return Launch::Failed(error),
            }
        }
    };
    let launcher = ProcessLauncher::reporting(move || {
        sender.send_replace(true);
    });
    match flow::launch_with_session(
        &SystemJava::from_environment(),
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
    game_root: &Path,
) -> Result<Session, String> {
    let network =
        AuthHttp::desktop().map_err(|_| "Could not initialize online sign-in.".to_owned())?;
    match service
        .resolve(id, &network)
        .await
        .map_err(|error| error.to_string())?
    {
        LaunchIdentity::Microsoft {
            account,
            access_token,
        } => Ok(Session::microsoft(
            account.name(),
            account.id().uuid().simple().to_string(),
            access_token,
        )),
        LaunchIdentity::Offline(account) => Ok(Session::offline_profile(
            account.name(),
            account.id().uuid().simple().to_string(),
        )),
        LaunchIdentity::Yggdrasil {
            account,
            access_token,
        } => {
            let server = account
                .server()
                .ok_or("The third-party account is missing its server.")?;
            let data = DataDir::new(
                game_root
                    .parent()
                    .ok_or("Could not determine the game data directory.")?,
            );
            let agent = third_party_agent(&network, server, &data).await?;
            Ok(Session::yggdrasil(
                account.name(),
                id.uuid().simple().to_string(),
                access_token,
                agent,
            ))
        }
    }
}

async fn third_party_agent(
    network: &AuthHttp,
    server: &yggdrasil::Server,
    data: &DataDir,
) -> Result<YggdrasilAgent, String> {
    let download = ReqwestNetwork::desktop()
        .map_err(|_| "Could not initialize the authentication agent download.")?;
    let (metadata, path) = tokio::join!(
        yggdrasil::launch_metadata(network, server),
        auth_agent::prepare(&download, data)
    );
    Ok(YggdrasilAgent::new(path?, server.url())
        .with_metadata(STANDARD.encode(metadata.map_err(|error| error.to_string())?)))
}
