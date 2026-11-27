// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use axlauncher_network_spi::auth::{Body, Request, Response, TransportError};
use axlauncher_spi::credentials::CredentialError;
use serde_json::{Value, json};

use super::*;
use crate::accounts::{AccountError, AccountService, LaunchIdentity, Persistence, yggdrasil};
use crate::test_support::TempRoot;
use axlauncher_spi::credentials::CredentialStore;
use axlauncher_storage::DataDir;

const ROOT: &str = "https://accounts.example.test/api/yggdrasil";
const PROFILE: &str = "d35cea1bfd1244bdab192183f93a50b6";
const OTHER: &str = "adfabd9181d4420c973706fceaa8fbb0";
const ACCESS: &str = "private-yggdrasil-access";

#[derive(Default)]
struct CredentialMemory(Mutex<HashMap<String, String>>);

impl CredentialStore for &CredentialMemory {
    fn read(&self, key: &str) -> Result<Option<String>, CredentialError> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }

    fn write(&self, key: &str, secret: &str) -> Result<(), CredentialError> {
        self.0
            .lock()
            .unwrap()
            .insert(key.to_owned(), secret.to_owned());
        Ok(())
    }

    fn remove(&self, key: &str) -> Result<(), CredentialError> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Step {
    Metadata,
    Authenticate,
    Bind,
    Validate,
    Refresh,
    Denied,
}

struct Provider {
    root: &'static str,
    steps: Mutex<VecDeque<Step>>,
    binding: Mutex<String>,
}

impl Provider {
    fn new(root: &'static str, steps: impl IntoIterator<Item = Step>) -> Self {
        Self {
            root,
            steps: Mutex::new(steps.into_iter().collect()),
            binding: Mutex::default(),
        }
    }

    fn complete(&self) {
        assert!(self.steps.lock().unwrap().is_empty());
    }
}

impl AuthNetwork for Provider {
    async fn send(&self, request: Request<'_>) -> Result<Response, TransportError> {
        let step = self
            .steps
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected request");
        let endpoint = match step {
            Step::Metadata => "",
            Step::Authenticate => "/authserver/authenticate",
            Step::Bind | Step::Refresh | Step::Denied => "/authserver/refresh",
            Step::Validate => "/authserver/validate",
        };
        let expected = Url::parse(&format!("{}{endpoint}", self.root)).unwrap();
        assert_eq!(Url::parse(request.url).unwrap(), expected);
        let payload: Value = match request.body {
            Some(Body::Json(body)) => serde_json::from_str(&body).unwrap(),
            None => Value::Null,
            Some(Body::Form(_)) => panic!("unexpected form body"),
        };
        assert!(request.bearer.is_none());
        Ok(self.reply(step, payload))
    }
}

impl Provider {
    fn reply(&self, step: Step, payload: Value) -> Response {
        let (status, body) = match step {
            Step::Metadata => (
                200,
                json!({"meta":{"serverName":"Example"},"skinDomains":[]}),
            ),
            Step::Authenticate => {
                assert_eq!(payload["password"], "test-password");
                let client = payload["clientToken"].as_str().unwrap();
                *self.binding.lock().unwrap() = client.to_owned();
                (
                    200,
                    json!({"accessToken": ACCESS, "clientToken":client, "availableProfiles":[profile(PROFILE), profile(OTHER)]}),
                )
            }
            Step::Bind => {
                assert_eq!(payload["selectedProfile"]["id"], PROFILE);
                assert_eq!(
                    payload["clientToken"],
                    self.binding.lock().unwrap().as_str()
                );
                (200, session(&payload, ACCESS))
            }
            Step::Validate => {
                assert_eq!(payload["accessToken"], ACCESS);
                assert!(payload.get("password").is_none());
                (403, Value::Null)
            }
            Step::Refresh => {
                assert!(payload.get("selectedProfile").is_none());
                (200, session(&payload, "refreshed-yggdrasil-access"))
            }
            Step::Denied => (403, json!({"error":"ForbiddenOperationException"})),
        };
        Response {
            status,
            body: serde_json::to_vec(&body).unwrap(),
        }
    }
}

fn profile(id: &str) -> Value {
    json!({"id":id,"name":"ExamplePlayer"})
}

fn session(payload: &Value, access: &str) -> Value {
    json!({"clientToken":payload["clientToken"],"accessToken":access,"selectedProfile":profile(PROFILE)})
}

async fn verified(provider: &Provider) -> yggdrasil::VerifiedAccount {
    let root = format!(" {}/ ", provider.root);
    let yggdrasil::SignIn::Choose(choice) =
        yggdrasil::sign_in(provider, &root, "test-user", "test-password")
            .await
            .unwrap()
    else {
        panic!("expected to choose a profile");
    };
    assert_eq!(choice.profiles().len(), 2);
    assert!(matches!(
        choice.finish(provider, Uuid::nil()).await,
        Err(YggdrasilError::WrongProfile)
    ));
    choice
        .finish(provider, Uuid::parse_str(PROFILE).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn binds_a_profile_persists_only_tokens_and_refreshes_after_restart() {
    let temp = TempRoot::new();
    let data = DataDir::isolated(temp.path()).unwrap();
    std::fs::write(
        crate::accounts::Document::path(&data),
        br#"{"schema_version":1,"accounts":[],"selected":null}"#,
    )
    .unwrap();
    let keyring = CredentialMemory::default();
    let service = AccountService::new(data.clone(), &keyring);
    let sign_in = Provider::new(ROOT, [Step::Metadata, Step::Authenticate, Step::Bind]);
    service
        .remember_yggdrasil(verified(&sign_in).await, None)
        .await
        .unwrap();
    sign_in.complete();
    let snapshot = service.snapshot().await.unwrap();
    let account = snapshot.selected().unwrap();
    assert_eq!(account.server().unwrap().url(), ROOT);
    assert_eq!(account.id().uuid().simple().to_string(), PROFILE);
    let document = std::fs::read_to_string(crate::accounts::Document::path(&data)).unwrap();
    assert!(!document.contains(ACCESS) && !document.contains("test-password"));
    let reopened = AccountService::new(data, &keyring);
    let refresh = Provider::new(ROOT, [Step::Validate, Step::Refresh]);
    let result = reopened.resolve(account.id(), &refresh).await.unwrap();
    assert!(
        matches!(result, LaunchIdentity::Yggdrasil { access_token, .. } if access_token == "refreshed-yggdrasil-access")
    );
    refresh.complete();
    assert!(
        keyring
            .0
            .lock()
            .unwrap()
            .values()
            .any(|value| value.contains("refreshed-yggdrasil-access"))
    );
}

#[tokio::test]
async fn separates_servers_and_reports_revoked_sessions() {
    let temp = TempRoot::new();
    let data = DataDir::isolated(temp.path()).unwrap();
    let keyring = CredentialMemory::default();
    let service = AccountService::new(data.clone(), &keyring);
    let first = Provider::new(ROOT, [Step::Metadata, Step::Authenticate, Step::Bind]);
    let other = Provider::new(
        "https://other.example.test",
        [Step::Metadata, Step::Authenticate, Step::Bind],
    );
    service
        .remember_yggdrasil(verified(&first).await, None)
        .await
        .unwrap();
    let id = service.snapshot().await.unwrap().selected().unwrap().id();
    service
        .remember_yggdrasil(verified(&other).await, None)
        .await
        .unwrap();
    assert_eq!(service.snapshot().await.unwrap().list().len(), 2);
    let reopened = AccountService::new(data, &keyring);
    let expired = Provider::new(ROOT, [Step::Validate, Step::Denied]);
    assert!(matches!(
        reopened.resolve(id, &expired).await,
        Err(AccountError::Yggdrasil(YggdrasilError::SignInAgain))
    ));
    expired.complete();
    assert_eq!(
        reopened.snapshot().await.unwrap().persistence(id),
        Some(Persistence::SignInRequired)
    );
}

#[tokio::test]
async fn rejects_insecure_or_credential_bearing_urls_before_sending_passwords() {
    let provider = Provider::new(ROOT, []);
    for url in [
        "http://accounts.example.test",
        "https://user@accounts.example.test",
        "https://accounts.example.test/#unsafe",
    ] {
        assert!(matches!(
            yggdrasil::sign_in(&provider, url, "test-user", "test-password").await,
            Err(YggdrasilError::InvalidUrl)
        ));
    }
    provider.complete();
}
