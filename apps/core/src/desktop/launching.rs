// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Desktop launch orchestration across profiles, authentication and local files.

use std::path::Path;
use std::sync::{Arc, Mutex};

use axlauncher_domain::normalize_game_language;
use axlauncher_install::{loader, vanilla};
use axlauncher_launch::{
    FlowError, GameLanguage, LaunchCommand, LaunchRequest, ProcessLauncher, SystemJava, Target,
    flow::{self, VersionInspection},
    plan::Session,
};
use axlauncher_network::auth::AuthHttp;
use axlauncher_spi::{ServiceHost, auth::CredentialStore};
use axlauncher_state::{
    SettingsData,
    accounts::{AccountId, AccountService},
    javas,
    profile::DiscoveredProfile,
};
use axlauncher_storage::{DataDir, GameDir};

use crate::launch_accounts;

use super::{java, on_worker, sources::DownloadOptions};

pub const LAUNCHER_LANGUAGE: &str = "en_us";

pub enum Player<S: CredentialStore> {
    Offline(Session),
    Online {
        service: Arc<AccountService<S>>,
        id: AccountId,
        fallen_proxy: Option<ServiceHost>,
    },
}

#[derive(Debug)]
pub enum LaunchFailure {
    NeedsInstall(String),
    Failed(String),
}

/// The same profile and version selection for preview, inspection and launch.
pub struct LaunchJob {
    request: LaunchRequest,
    profile_id: String,
    data: DataDir,
    vanilla: bool,
}

impl LaunchJob {
    pub fn for_profile(
        data: &DataDir,
        profile: &DiscoveredProfile,
        player_name: String,
        settings: Option<&SettingsData>,
    ) -> Option<Self> {
        let game = profile.profile.game()?;
        let version = game.effective_version()?;
        let language = settings
            .and_then(|settings| normalize_game_language(&settings.default_game_language))
            .filter(|language| !language.is_empty())
            .unwrap_or_else(|| LAUNCHER_LANGUAGE.to_owned());
        Some(Self {
            request: LaunchRequest {
                game_root: if game.is_runtime_isolated() {
                    GameDir::isolated(&profile.directory)
                } else {
                    data.game()
                },
                instance_directory: profile.directory.clone(),
                version_id: version.to_owned(),
                java_preference: None,
                player_name,
                game_language: Some(GameLanguage::new(&game.minecraft, &language)),
            },
            profile_id: profile.profile.id().to_owned(),
            data: data.clone(),
            vanilla: game.is_vanilla() && version == game.minecraft,
        })
    }

    pub fn game_root(&self) -> &Path {
        self.request.game_root.root()
    }

    pub fn instance_directory(&self) -> &Path {
        &self.request.instance_directory
    }

    pub fn version_id(&self) -> &str {
        &self.request.version_id
    }

    pub fn inspect(&self) -> Result<VersionInspection, FlowError> {
        let (java, request) = self.configured_java()?;
        flow::inspect_version(&java, &request)
    }

    pub fn preview(&self, session: &Session) -> Result<LaunchCommand, FlowError> {
        let (java, request) = self.configured_java()?;
        flow::preview_with_session(&java, &request, session)
    }

    fn configured_java(&self) -> Result<(SystemJava, LaunchRequest), FlowError> {
        let saved =
            javas::load(self.data.root()).map_err(|error| FlowError::JavaConfiguration {
                source: std::io::Error::other(error),
            })?;
        let mut request = self.request.clone();
        request.java_preference = saved
            .profile_preference(&self.profile_id)
            .map(Path::to_path_buf);
        Ok((java::provider(&self.data, &saved), request))
    }

    /// A desktop worker reports account resolution and the actual child spawn.
    pub fn run<S: CredentialStore>(
        self,
        player: Player<S>,
        options: DownloadOptions,
        account_resolved: impl FnOnce(Arc<AccountService<S>>),
        started: impl Fn() + Send + Sync + 'static,
    ) -> Result<(), LaunchFailure> {
        on_worker(self.launch(player, options, account_resolved, started))
            .map_err(|error| LaunchFailure::Failed(error.to_string()))?
    }

    async fn launch<S: CredentialStore>(
        self,
        player: Player<S>,
        options: DownloadOptions,
        account_resolved: impl FnOnce(Arc<AccountService<S>>),
        started: impl Fn() + Send + Sync + 'static,
    ) -> Result<(), LaunchFailure> {
        let session = resolve_session(player, options, account_resolved)
            .await
            .map_err(LaunchFailure::Failed)?;
        let lock = vanilla::launch_lock(&self.request.game_root).map_err(LaunchFailure::Failed)?;
        self.check_files()?;
        let (java, request) = self.configured_java().map_err(launch_error)?;

        // Keep installers out until the child has opened the prepared files,
        // but do not hold the launch lock for the whole play session.
        let lock = Mutex::new(Some(lock));
        let launcher = ProcessLauncher::reporting(move || {
            drop(lock.lock().expect("launch guard").take());
            started();
        });
        flow::launch_with_session(&java, &launcher, request, &session)
            .await
            .map(|_| ())
            .map_err(launch_error)
    }

    fn check_files(&self) -> Result<(), LaunchFailure> {
        let check = if self.vanilla {
            vanilla::check_files
        } else {
            loader::check_files
        };
        check(&self.request.game_root, self.version_id(), &Target::host()).map_err(|error| {
            LaunchFailure::NeedsInstall(format!("Install game files to continue. {error}"))
        })
    }
}

async fn resolve_session<S: CredentialStore>(
    player: Player<S>,
    options: DownloadOptions,
    account_resolved: impl FnOnce(Arc<AccountService<S>>),
) -> Result<Session, String> {
    let (service, id, fallen_proxy) = match player {
        Player::Offline(session) => return Ok(session),
        Player::Online {
            service,
            id,
            fallen_proxy,
        } => (service, id, fallen_proxy),
    };
    let session = online_session(&service, id, options, fallen_proxy).await;
    account_resolved(service);
    session
}

async fn online_session<S: CredentialStore>(
    service: &AccountService<S>,
    id: AccountId,
    options: DownloadOptions,
    fallen_proxy: Option<ServiceHost>,
) -> Result<Session, String> {
    let auth = AuthHttp::desktop()
        .map_err(|_| "Could not initialize online sign-in.".to_owned())?
        .with_fallen_proxy(fallen_proxy.clone());
    launch_accounts::prepare(service, id, &auth, || {
        options
            .network()
            .map_err(|_| "Could not initialize the authentication agent download.".to_owned())
    })
    .await
    .map(|session| session.with_fallen_proxy(fallen_proxy))
}

fn launch_error(error: FlowError) -> LaunchFailure {
    let message = match error {
        FlowError::NoMatchingJava { required, .. } => format!(
            "Java {required} is required. Add a Java {required} installation or install the managed runtime."
        ),
        error => error.to_string(),
    };
    LaunchFailure::Failed(message)
}

#[cfg(test)]
#[path = "../../../../tests/apps/core/src/desktop/launching.rs"]
mod tests;
