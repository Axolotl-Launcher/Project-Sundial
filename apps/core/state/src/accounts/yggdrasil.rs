// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Authentication for third-party Minecraft profile servers.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use axlauncher_spi::auth::{AuthNetwork, Body, Request, Response, TransportError};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use url::Url;
use uuid::Uuid;

use super::AccountId;

const USER_AGENT: &str = "Axolotl Launcher";
pub(super) const SESSION_TTL: Duration = Duration::from_secs(300);
const CHOICE_TTL: Duration = Duration::from_secs(300);

/// A normalized HTTPS authentication server and its display name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Server {
    url: String,
    name: String,
}

impl Server {
    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub(super) fn account_id(&self, profile: Uuid) -> AccountId {
        let digest = Sha256::digest(&self.url);
        let server = Uuid::from_bytes(digest[..16].try_into().expect("UUID-sized hash prefix"));
        AccountId::Yggdrasil { profile, server }
    }
}

/// A player profile offered by the authentication server.
#[derive(Clone, Deserialize, Serialize)]
pub struct Profile {
    #[serde(serialize_with = "uuid::serde::simple::serialize")]
    id: Uuid,
    name: String,
}

impl Profile {
    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    fn valid(&self) -> bool {
        !self.id.is_nil() && !self.name.trim().is_empty()
    }
}

/// Credentials checked by a Yggdrasil server, awaiting persistence by the account service.
pub struct VerifiedAccount {
    pub(super) server: Server,
    pub(super) profile: Profile,
    pub(super) tokens: Tokens,
}

/// A sign-in can finish immediately or require a player profile choice.
pub enum SignIn {
    Complete(VerifiedAccount),
    Choose(ProfileChoice),
}

/// A short-lived authentication result that can bind one offered profile.
pub struct ProfileChoice {
    server: Server,
    tokens: Tokens,
    profiles: Vec<Profile>,
    created: Instant,
}

impl ProfileChoice {
    pub fn profiles(&self) -> &[Profile] {
        &self.profiles
    }

    pub async fn finish(
        &self,
        network: &impl AuthNetwork,
        id: Uuid,
    ) -> Result<VerifiedAccount, YggdrasilError> {
        if self.created.elapsed() >= CHOICE_TTL {
            return Err(YggdrasilError::Expired);
        }
        let profile = self
            .profiles
            .iter()
            .find(|profile| profile.id == id)
            .ok_or(YggdrasilError::WrongProfile)?;
        let (profile, tokens) = refresh(network, &self.server, &self.tokens, Some(profile)).await?;
        Ok(VerifiedAccount {
            server: self.server.clone(),
            profile,
            tokens,
        })
    }
}

#[derive(Deserialize, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub(super) struct Tokens {
    pub access_token: String,
    client_token: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Reply {
    #[serde(flatten)]
    tokens: Tokens,
    selected_profile: Option<Profile>,
    #[serde(default)]
    available_profiles: Vec<Profile>,
}

/// An input, connection or account error with no server-provided secret text.
#[derive(Debug, thiserror::Error)]
pub enum YggdrasilError {
    #[error("Enter an HTTPS Yggdrasil API URL without credentials, a query, or a fragment.")]
    InvalidUrl,
    #[error("Enter your third-party username and password.")]
    MissingLogin,
    #[error("Could not reach the third-party sign-in server.")]
    Unavailable,
    #[error("The third-party server returned an invalid response.")]
    InvalidResponse,
    #[error("The third-party server rejected the request (HTTP {0}).")]
    Rejected(u16),
    #[error("Check the third-party username and password, then try again.")]
    InvalidLogin,
    #[error("The third-party account has no Minecraft profile.")]
    NoProfiles,
    #[error("The selected profile was not returned by this server.")]
    WrongProfile,
    #[error("Profile selection expired. Sign in again.")]
    Expired,
    #[error("Sign in to the third-party account again.")]
    SignInAgain,
}

/// Signs in using the API root entered in the launcher.
pub async fn sign_in(
    network: &impl AuthNetwork,
    root: &str,
    login: &str,
    password: &str,
) -> Result<SignIn, YggdrasilError> {
    if login.trim().is_empty() || password.is_empty() {
        return Err(YggdrasilError::MissingLogin);
    }
    let url = normalize(root)?;
    let (server, _) = metadata(network, &url).await?;
    let client_token = Uuid::new_v4().simple().to_string();
    let response = post(
        network,
        &server,
        "authenticate",
        json!({
            "agent": {"name": "Minecraft", "version": 1},
            "username": login.trim(), "password": password, "clientToken": client_token,
        }),
    )
    .await?;
    if matches!(response.status, 401 | 403) {
        return Err(YggdrasilError::InvalidLogin);
    }
    let reply = decode_reply(&response, &client_token)?;
    if let Some(profile) = reply.selected_profile {
        return Ok(SignIn::Complete(VerifiedAccount {
            server,
            profile,
            tokens: reply.tokens,
        }));
    }
    choose_profile(network, server, reply.tokens, reply.available_profiles).await
}

async fn choose_profile(
    network: &impl AuthNetwork,
    server: Server,
    tokens: Tokens,
    profiles: Vec<Profile>,
) -> Result<SignIn, YggdrasilError> {
    let choice = ProfileChoice {
        server,
        tokens,
        profiles,
        created: Instant::now(),
    };
    match choice.profiles.as_slice() {
        [] => Err(YggdrasilError::NoProfiles),
        [profile] => choice
            .finish(network, profile.id)
            .await
            .map(SignIn::Complete),
        _ => Ok(SignIn::Choose(choice)),
    }
}

/// The public server metadata for authlib-injector, from the same HTTPS root.
pub async fn launch_metadata(
    network: &impl AuthNetwork,
    server: &Server,
) -> Result<Vec<u8>, YggdrasilError> {
    let (_, raw) = metadata(network, &normalize(server.url())?).await?;
    Ok(raw)
}

pub(super) async fn validate(
    network: &impl AuthNetwork,
    server: &Server,
    tokens: &Tokens,
    profile_id: Uuid,
) -> Result<Tokens, YggdrasilError> {
    let response = post(network, server, "validate", json!(tokens)).await?;
    match response.status {
        200..=299 => Ok(tokens.clone()),
        401 | 403 => {
            let (profile, tokens) = refresh(network, server, tokens, None).await?;
            if profile.id != profile_id {
                return Err(YggdrasilError::WrongProfile);
            }
            Ok(tokens)
        }
        status => Err(YggdrasilError::Rejected(status)),
    }
}

async fn refresh(
    network: &impl AuthNetwork,
    server: &Server,
    tokens: &Tokens,
    profile: Option<&Profile>,
) -> Result<(Profile, Tokens), YggdrasilError> {
    let mut payload = json!(tokens);
    if let Some(profile) = profile {
        payload["selectedProfile"] = json!(profile);
    }
    let response = post(network, server, "refresh", payload).await?;
    if matches!(response.status, 401 | 403) {
        return Err(YggdrasilError::SignInAgain);
    }
    let reply = decode_reply(&response, &tokens.client_token)?;
    let selected = reply.selected_profile.ok_or(YggdrasilError::WrongProfile)?;
    if profile.is_some_and(|requested| requested.id != selected.id) {
        return Err(YggdrasilError::WrongProfile);
    }
    Ok((selected, reply.tokens))
}

fn normalize(root: &str) -> Result<Url, YggdrasilError> {
    let url =
        Url::parse(root.trim().trim_end_matches('/')).map_err(|_| YggdrasilError::InvalidUrl)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(YggdrasilError::InvalidUrl);
    }
    Ok(url)
}

async fn metadata(
    network: &impl AuthNetwork,
    url: &Url,
) -> Result<(Server, Vec<u8>), YggdrasilError> {
    let response = send(network, Request::new(url.as_str(), USER_AGENT)).await?;
    check_status(response.status)?;
    let value: Value =
        serde_json::from_slice(&response.body).map_err(|_| YggdrasilError::InvalidResponse)?;
    if !value.get("meta").is_some_and(Value::is_object) {
        return Err(YggdrasilError::InvalidResponse);
    }
    let name = value["meta"]["serverName"]
        .as_str()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| url.host_str().expect("normalized server"))
        .chars()
        .filter(|character| !character.is_control())
        .take(80)
        .collect();
    let server = Server {
        url: url.as_str().trim_end_matches('/').to_owned(),
        name,
    };
    Ok((server, response.body))
}

async fn post(
    network: &impl AuthNetwork,
    server: &Server,
    action: &str,
    payload: Value,
) -> Result<Response, YggdrasilError> {
    let root = normalize(server.url())?;
    let endpoint = format!(
        "{}/authserver/{action}",
        root.as_str().trim_end_matches('/')
    );
    send(
        network,
        Request::new(&endpoint, USER_AGENT).with_body(Body::Json(payload.to_string())),
    )
    .await
}

async fn send(
    network: &impl AuthNetwork,
    request: Request<'_>,
) -> Result<Response, YggdrasilError> {
    network.send(request).await.map_err(|error| match error {
        TransportError::Unavailable => YggdrasilError::Unavailable,
        TransportError::InvalidResponse => YggdrasilError::InvalidResponse,
    })
}

fn decode_reply(response: &Response, client_token: &str) -> Result<Reply, YggdrasilError> {
    check_status(response.status)?;
    let reply: Reply =
        serde_json::from_slice(&response.body).map_err(|_| YggdrasilError::InvalidResponse)?;
    let mut ids = HashSet::new();
    if reply.tokens.access_token.is_empty()
        || reply.tokens.client_token != client_token
        || reply
            .selected_profile
            .iter()
            .chain(&reply.available_profiles)
            .any(|profile| !profile.valid())
        || reply
            .available_profiles
            .iter()
            .any(|profile| !ids.insert(profile.id))
    {
        return Err(YggdrasilError::InvalidResponse);
    }
    Ok(reply)
}

fn check_status(status: u16) -> Result<(), YggdrasilError> {
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(YggdrasilError::Rejected(status))
    }
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/state/src/accounts/yggdrasil.rs"]
mod tests;
