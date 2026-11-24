// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Public-client authorization code and device code grants.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

use super::{
    AuthNetwork, Body, CLIENT_ID, Deserialize, Duration, Instant, MicrosoftError, OAuthToken,
    Request, USER_AGENT, Url, Uuid, VerifiedAccount, decode, send, verified,
};

const REDIRECT_URI: &str = "http://127.0.0.1:53682/oauth/microsoft/callback";
pub const CALLBACK_PORT: u16 = 53682;
pub const CALLBACK_PATH: &str = "/oauth/microsoft/callback";
const AUTHORIZE: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/authorize";
const DEVICE_CODE: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode";
const TOKEN: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0/token";
const SCOPE: &str = "XboxLive.signin offline_access";
const MICROSOFT: &str = "Microsoft";

/// One PKCE attempt, including the state required by its loopback listener.
pub struct BrowserLogin {
    verifier: String,
    state: String,
    url: Url,
}

impl BrowserLogin {
    pub fn begin() -> Self {
        let verifier = nonce();
        let state = nonce();
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
        let mut url = Url::parse(AUTHORIZE).expect("valid Microsoft authorization endpoint");
        url.query_pairs_mut().extend_pairs([
            ("client_id", CLIENT_ID),
            ("response_type", "code"),
            ("redirect_uri", REDIRECT_URI),
            ("response_mode", "query"),
            ("scope", SCOPE),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
            ("state", &state),
            ("prompt", "select_account"),
        ]);
        Self {
            verifier,
            state,
            url,
        }
    }

    pub fn url(&self) -> &str {
        self.url.as_str()
    }

    pub fn state(&self) -> &str {
        &self.state
    }

    /// Exchanges the code accepted by the loopback listener for a verified account.
    pub async fn finish(
        self,
        network: &impl AuthNetwork,
        code: &str,
    ) -> Result<VerifiedAccount, MicrosoftError> {
        let response = send(
            network,
            form(
                TOKEN,
                [
                    ("grant_type", "authorization_code"),
                    ("code", code),
                    ("code_verifier", &self.verifier),
                    ("redirect_uri", REDIRECT_URI),
                    ("scope", SCOPE),
                ],
            ),
            MICROSOFT,
        )
        .await?;
        verified(network, decode(response, MICROSOFT)?).await
    }
}

fn nonce() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

/// Device authorization and its server-specified polling schedule.
pub struct DeviceLogin {
    grant: DeviceGrant,
    deadline: Instant,
}

#[derive(Deserialize)]
struct DeviceGrant {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: Option<u64>,
}

#[derive(Deserialize)]
struct OAuthFailure {
    error: String,
}

impl DeviceLogin {
    pub async fn begin(network: &impl AuthNetwork) -> Result<Self, MicrosoftError> {
        let grant: DeviceGrant = decode(
            send(network, form(DEVICE_CODE, [("scope", SCOPE)]), MICROSOFT).await?,
            MICROSOFT,
        )?;
        let verification = Url::parse(&grant.verification_uri)
            .map_err(|_| MicrosoftError::InvalidResponse(MICROSOFT))?;
        if verification.scheme() != "https"
            || !matches!(
                verification.host_str(),
                Some("microsoft.com" | "www.microsoft.com" | "login.microsoftonline.com")
            )
            || grant.device_code.is_empty()
            || grant.user_code.is_empty()
            || grant.expires_in == 0
        {
            return Err(MicrosoftError::InvalidResponse(MICROSOFT));
        }
        let deadline = Instant::now()
            .checked_add(Duration::from_secs(grant.expires_in))
            .ok_or(MicrosoftError::InvalidResponse(MICROSOFT))?;
        Ok(Self { grant, deadline })
    }

    pub fn url(&self) -> &str {
        &self.grant.verification_uri
    }

    pub fn user_code(&self) -> &str {
        &self.grant.user_code
    }

    /// Waits for authorization, applying the server's polling and expiry rules.
    pub async fn finish(
        self,
        network: &impl AuthNetwork,
    ) -> Result<VerifiedAccount, MicrosoftError> {
        let token = tokio::time::timeout_at(self.deadline.into(), self.poll(network))
            .await
            .map_err(|_| MicrosoftError::Expired)??;
        verified(network, token).await
    }

    async fn poll(&self, network: &impl AuthNetwork) -> Result<OAuthToken, MicrosoftError> {
        let mut interval = Duration::from_secs(self.grant.interval.unwrap_or(5).clamp(1, 60));
        loop {
            tokio::time::sleep(interval).await;
            let request = form(
                TOKEN,
                [
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                    ("device_code", &self.grant.device_code),
                ],
            );
            match poll_reply(send(network, request, MICROSOFT).await?)? {
                Poll::Pending => (),
                Poll::SlowDown => interval += Duration::from_secs(5),
                Poll::Authorized(token) => return Ok(token),
            }
        }
    }
}

enum Poll {
    Pending,
    SlowDown,
    Authorized(OAuthToken),
}

fn poll_reply(response: super::Response) -> Result<Poll, MicrosoftError> {
    if (200..300).contains(&response.status) {
        return decode(response, MICROSOFT).map(Poll::Authorized);
    }
    match failure(&response).as_deref() {
        Some("authorization_pending") => Ok(Poll::Pending),
        Some("slow_down") => Ok(Poll::SlowDown),
        Some("authorization_declined" | "access_denied") => Err(MicrosoftError::Denied),
        Some("expired_token" | "code_expired") => Err(MicrosoftError::Expired),
        _ => Err(MicrosoftError::Rejected {
            service: MICROSOFT,
            status: response.status,
        }),
    }
}

pub(super) async fn refresh(
    network: &impl AuthNetwork,
    token: &str,
) -> Result<OAuthToken, MicrosoftError> {
    let response = send(
        network,
        form(
            TOKEN,
            [
                ("grant_type", "refresh_token"),
                ("refresh_token", token),
                ("scope", SCOPE),
            ],
        ),
        MICROSOFT,
    )
    .await?;
    if matches!(
        failure(&response).as_deref(),
        Some("invalid_grant" | "interaction_required")
    ) {
        return Err(MicrosoftError::SignInAgain);
    }
    decode(response, MICROSOFT)
}

fn failure(response: &super::Response) -> Option<String> {
    let failure: OAuthFailure = serde_json::from_slice(&response.body).ok()?;
    Some(failure.error)
}

fn form<'a>(url: &'a str, pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Request<'a> {
    let mut body = url::form_urlencoded::Serializer::new(String::new());
    body.append_pair("client_id", CLIENT_ID).extend_pairs(pairs);
    Request::new(url, USER_AGENT).with_body(Body::Form(body.finish()))
}
