// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Account-based launch preparation across the state, install, and launch systems.

use axlauncher_launch::plan::{Session, YggdrasilAgent};
use axlauncher_spi::{
    Network,
    auth::{AuthNetwork, CredentialStore},
};
use axlauncher_state::accounts::{Account, AccountId, AccountService, LaunchIdentity, yggdrasil};
use axlauncher_storage::DataDir;
use base64::{Engine as _, engine::general_purpose::STANDARD};

/// A session ready for Minecraft, with third-party files prepared under the account service's root.
pub async fn prepare<S: CredentialStore, N: Network>(
    service: &AccountService<S>,
    id: AccountId,
    auth: &impl AuthNetwork,
    download: impl FnOnce() -> Result<N, String>,
) -> Result<Session, String> {
    let identity = service
        .resolve(id, auth)
        .await
        .map_err(|error| error.to_string())?;
    let uuid = id.uuid().simple().to_string();
    match identity {
        LaunchIdentity::Offline(account) => Ok(Session::offline_profile(account.name(), uuid)),
        LaunchIdentity::Microsoft {
            account,
            access_token,
            xuid,
        } => Ok(Session::microsoft(account.name(), uuid, access_token).with_xuid(xuid)),
        LaunchIdentity::Yggdrasil {
            account,
            access_token,
        } => {
            let server = account
                .server()
                .ok_or("The third-party account is missing its server.")?;
            let agent = agent(auth, &download()?, server, service.data_dir()).await?;
            Ok(Session::yggdrasil(
                account.name(),
                uuid,
                access_token,
                agent,
            ))
        }
    }
}

/// A local diagnostic session built without reading or refreshing credentials.
pub fn preview(account: &Account, data: &DataDir) -> Session {
    let uuid = account.id().uuid().simple().to_string();
    match account.id() {
        AccountId::Offline(_) => Session::offline_profile(account.name(), uuid),
        AccountId::Microsoft(_) => Session::microsoft_preview(account.name(), uuid),
        AccountId::Yggdrasil { .. } => {
            let server = account
                .server()
                .expect("validated third-party account record");
            let path = std::path::absolute(data.auth_agent()).unwrap_or_else(|_| data.auth_agent());
            Session::yggdrasil_preview(
                account.name(),
                uuid,
                YggdrasilAgent::new(path, server.url()),
            )
        }
    }
}

async fn agent(
    auth: &impl AuthNetwork,
    download: &impl Network,
    server: &yggdrasil::Server,
    data: &DataDir,
) -> Result<YggdrasilAgent, String> {
    let (metadata, path) = tokio::join!(
        yggdrasil::launch_metadata(auth, server),
        axlauncher_install::auth_agent::prepare(download, data)
    );
    Ok(YggdrasilAgent::new(path?, server.url())
        .with_metadata(STANDARD.encode(metadata.map_err(|error| error.to_string())?)))
}
