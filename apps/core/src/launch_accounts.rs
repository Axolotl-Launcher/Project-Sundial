// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Turns resolved accounts into launch sessions without handling sign-in or storage.

use axlauncher_download_spi::FileDownloader;
use axlauncher_launch::plan::{Session, YggdrasilAgent};
use axlauncher_storage::{DataDir, DataPath};
use axlauncher_worker::accounts::{Account, AccountId, Identity};

/// A session ready for Minecraft, with the verified third-party agent when needed.
pub(crate) async fn prepare(
    identity: Identity,
    id: AccountId,
    data: &DataDir,
    downloader: &impl FileDownloader,
) -> Result<Session, String> {
    let uuid = id.uuid().simple().to_string();
    match identity {
        Identity::Offline { name } => Ok(Session::offline_profile(name, uuid)),
        Identity::Microsoft {
            name,
            access_token,
            xuid,
        } => Ok(Session::microsoft(name, uuid, access_token).with_xuid(xuid)),
        Identity::Yggdrasil {
            name,
            access_token,
            url,
            metadata,
        } => {
            let path = axlauncher_install::auth_agent::prepare(downloader, data).await?;
            let agent = YggdrasilAgent::new(path, url).with_metadata(metadata);
            Ok(Session::yggdrasil(name, uuid, access_token, agent))
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
            let agent = data.path(DataPath::AuthAgent);
            let path = std::path::absolute(agent).unwrap_or_else(|_| agent.to_owned());
            Session::yggdrasil_preview(
                account.name(),
                uuid,
                YggdrasilAgent::new(path, server.url()),
            )
        }
    }
}
