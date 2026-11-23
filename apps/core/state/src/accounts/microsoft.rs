// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Microsoft OAuth and Minecraft: Java Edition account verification.

use std::time::{Duration, Instant};

use axlauncher_spi::auth::{AuthNetwork, Body, Request, Response, TransportError};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use url::Url;
use uuid::Uuid;

mod oauth;
pub use oauth::{BrowserLogin, CALLBACK_PATH, CALLBACK_PORT, DeviceLogin};

// The public OAuth application registration used by Axolotl Launcher.
const CLIENT_ID: &str = "c7104738-eff0-4fc9-a4ad-59e2c72ec691";
const USER_AGENT: &str = "Axolotl Launcher";
const XBOX_AUTH: &str = "https://user.auth.xboxlive.com/user/authenticate";
const XSTS_AUTH: &str = "https://xsts.auth.xboxlive.com/xsts/authorize";
const MINECRAFT_TOKEN: &str = "https://api.minecraftservices.com/launcher/login";
const LICENSE: &str = "https://api.minecraftservices.com/entitlements/license";
const PROFILE: &str = "https://api.minecraftservices.com/minecraft/profile";
const REFRESH_MARGIN: Duration = Duration::from_secs(300);

/// A failed stage of Microsoft or Minecraft authentication.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum MicrosoftError {
    #[error("Could not reach {0}. Check your connection and try again.")]
    Unavailable(&'static str),
    #[error("{0} returned an invalid response.")]
    InvalidResponse(&'static str),
    #[error("{service} rejected the request (HTTP {status}).")]
    Rejected { service: &'static str, status: u16 },
    #[error("Set up an Xbox profile for this Microsoft account and try again.")]
    MissingXboxProfile,
    #[error("Check the account’s Xbox family or regional settings and try again.")]
    XboxRestricted,
    #[error("This account does not have access to Minecraft: Java Edition.")]
    NoLicense,
    #[error("Create a Minecraft: Java Edition profile and sign in again.")]
    MissingMinecraftProfile,
    #[error("Sign in with Microsoft again to use this account.")]
    SignInAgain,
    #[error("Microsoft sign-in was declined.")]
    Denied,
    #[error("Microsoft sign-in expired. Start again to get a new code.")]
    Expired,
    #[error("Sign in with the Microsoft account that owns this Minecraft profile.")]
    WrongAccount,
}

/// A verified Minecraft profile and its current credentials.
pub struct VerifiedAccount {
    pub(super) profile: Profile,
    pub(super) session: Tokens,
}

#[derive(Deserialize)]
pub(super) struct Profile {
    pub id: Uuid,
    pub name: String,
}

pub(super) struct Tokens {
    pub access: String,
    pub refresh: String,
    pub xuid: Option<String>,
    expires: Instant,
}

impl Tokens {
    pub(super) fn is_current(&self) -> bool {
        self.expires.saturating_duration_since(Instant::now()) > REFRESH_MARGIN
    }

    pub(super) fn is_valid(&self) -> bool {
        self.expires > Instant::now()
    }
}

#[derive(Deserialize)]
struct OAuthToken {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
}

#[derive(Deserialize)]
struct XboxToken {
    #[serde(rename = "Token")]
    token: String,
}

#[derive(Deserialize)]
struct XstsToken {
    #[serde(rename = "Token")]
    token: String,
    #[serde(rename = "DisplayClaims")]
    claims: XstsClaims,
}

#[derive(Deserialize)]
struct XstsClaims {
    xui: Vec<UserClaim>,
}

#[derive(Deserialize)]
struct UserClaim {
    uhs: String,
    xid: Option<String>,
}

#[derive(Deserialize)]
struct GameToken {
    access_token: String,
    expires_in: Option<u64>,
}

#[derive(Deserialize)]
struct Entitlements {
    items: Vec<Entitlement>,
}

#[derive(Deserialize)]
struct Entitlement {
    name: String,
}

async fn verified(
    network: &impl AuthNetwork,
    oauth: OAuthToken,
) -> Result<VerifiedAccount, MicrosoftError> {
    let session = exchange(network, oauth, None).await?;
    let mut license_url = Url::parse(LICENSE).expect("valid Minecraft license endpoint");
    license_url
        .query_pairs_mut()
        .append_pair("requestId", &Uuid::new_v4().to_string());
    let license = Request::new(license_url.as_str(), USER_AGENT).with_bearer(&session.access);
    let rights: Entitlements = get_json(network, license, "Minecraft license service").await?;
    if !rights
        .items
        .iter()
        .any(|item| item.name == "game_minecraft")
    {
        return Err(MicrosoftError::NoLicense);
    }
    let request = Request::new(PROFILE, USER_AGENT).with_bearer(&session.access);
    let profile: Profile = get_json(network, request, "Minecraft profile service")
        .await
        .map_err(|error| {
            if matches!(error, MicrosoftError::Rejected { status: 404, .. }) {
                return MicrosoftError::MissingMinecraftProfile;
            }
            error
        })?;
    if profile.id.is_nil() || profile.name.is_empty() {
        return Err(MicrosoftError::InvalidResponse("Minecraft profile service"));
    }
    Ok(VerifiedAccount { profile, session })
}

pub(super) async fn refresh(
    network: &impl AuthNetwork,
    token: &str,
) -> Result<Tokens, MicrosoftError> {
    let oauth = oauth::refresh(network, token).await?;
    exchange(network, oauth, Some(token)).await
}

async fn exchange(
    network: &impl AuthNetwork,
    oauth: OAuthToken,
    previous: Option<&str>,
) -> Result<Tokens, MicrosoftError> {
    let refresh = oauth
        .refresh_token
        .filter(|token| !token.is_empty())
        .or_else(|| previous.map(str::to_owned))
        .ok_or(MicrosoftError::InvalidResponse("Microsoft"))?;
    if oauth.access_token.is_empty() || oauth.expires_in == 0 {
        return Err(MicrosoftError::InvalidResponse("Microsoft"));
    }
    let xsts = authorize_xbox(network, &oauth.access_token).await?;
    let claim = xsts
        .claims
        .xui
        .first()
        .filter(|claim| !claim.uhs.is_empty())
        .ok_or(MicrosoftError::InvalidResponse("Xbox authorization"))?;
    let request = json_request(
        MINECRAFT_TOKEN,
        json!({
            "platform": "PC_LAUNCHER",
            "xtoken": format!("XBL3.0 x={};{}", claim.uhs, xsts.token),
        }),
    );
    let token: GameToken = get_json(network, request, "Minecraft sign-in").await?;
    let lifetime = token.expires_in.unwrap_or(oauth.expires_in);
    if token.access_token.is_empty() || lifetime == 0 {
        return Err(MicrosoftError::InvalidResponse("Minecraft sign-in"));
    }
    let expires = Instant::now()
        .checked_add(Duration::from_secs(lifetime))
        .ok_or(MicrosoftError::InvalidResponse("Minecraft sign-in"))?;
    Ok(Tokens {
        access: token.access_token,
        refresh,
        xuid: claim.xid.as_deref().and_then(xuid),
        expires,
    })
}

fn xuid(input: &str) -> Option<String> {
    let value = input.parse::<u64>().ok()?;
    (value != 0).then(|| value.to_string())
}

async fn authorize_xbox(
    network: &impl AuthNetwork,
    access: &str,
) -> Result<XstsToken, MicrosoftError> {
    let request = json_request(
        XBOX_AUTH,
        json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={access}"),
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT",
        }),
    );
    let user: XboxToken = get_json(network, request, "Xbox Live").await?;
    let request = json_request(
        XSTS_AUTH,
        json!({
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT",
            "Properties": { "SandboxId": "RETAIL", "UserTokens": [user.token] },
        }),
    );
    let response = send(network, request, "Xbox authorization").await?;
    if response.status == 401 {
        let body: Value = serde_json::from_slice(&response.body).unwrap_or_default();
        match body["XErr"].as_u64() {
            Some(2_148_916_233) => return Err(MicrosoftError::MissingXboxProfile),
            Some(2_148_916_235..=2_148_916_238) => return Err(MicrosoftError::XboxRestricted),
            _ => (),
        }
    }
    decode(response, "Xbox authorization")
}

fn json_request(url: &str, body: Value) -> Request<'_> {
    Request::new(url, USER_AGENT).with_body(Body::Json(body.to_string()))
}

async fn get_json<T: DeserializeOwned>(
    network: &impl AuthNetwork,
    request: Request<'_>,
    service: &'static str,
) -> Result<T, MicrosoftError> {
    decode(send(network, request, service).await?, service)
}

async fn send(
    network: &impl AuthNetwork,
    request: Request<'_>,
    service: &'static str,
) -> Result<Response, MicrosoftError> {
    network.send(request).await.map_err(|error| match error {
        TransportError::Unavailable => MicrosoftError::Unavailable(service),
        TransportError::InvalidResponse => MicrosoftError::InvalidResponse(service),
    })
}

fn decode<T: DeserializeOwned>(
    response: Response,
    service: &'static str,
) -> Result<T, MicrosoftError> {
    if !(200..300).contains(&response.status) {
        return Err(MicrosoftError::Rejected {
            service,
            status: response.status,
        });
    }
    serde_json::from_slice(&response.body).map_err(|_| MicrosoftError::InvalidResponse(service))
}
