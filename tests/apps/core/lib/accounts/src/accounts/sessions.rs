// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::VecDeque;
use std::sync::Mutex as SyncMutex;
use std::sync::atomic::{AtomicBool, Ordering};

use axlauncher_network_spi::auth::{Body, Request, Response, TransportError};
use axlauncher_spi::credentials::CredentialError;
use axlauncher_storage::{DataPath, Document};
use axlauncher_worker::accounts::Persistence;
use uuid::Uuid;

use super::*;
use crate::accounts::microsoft::BrowserLogin;
use crate::test_support::TempRoot;

#[path = "sessions/hierarchy.rs"]
mod hierarchy;

fn path(data: &DataDir) -> std::path::PathBuf {
    Document::Accounts.path(data.path(DataPath::Root))
}

#[derive(Default)]
struct CredentialMemory {
    values: SyncMutex<HashMap<String, String>>,
    unavailable: bool,
    fail_write: AtomicBool,
    fail_read: AtomicBool,
    fail_remove: AtomicBool,
    cancel_during_write: SyncMutex<Option<SignInCommit>>,
}

impl CredentialStore for &CredentialMemory {
    fn read(&self, key: &str) -> Result<Option<String>, CredentialError> {
        if self.fail_read.load(Ordering::Relaxed) {
            return Err(CredentialError);
        }
        Ok(self.values.lock().unwrap().get(key).cloned())
    }

    fn write(&self, key: &str, secret: &str) -> Result<(), CredentialError> {
        if self.unavailable || self.fail_write.load(Ordering::Relaxed) {
            return Err(CredentialError);
        }
        self.values
            .lock()
            .unwrap()
            .insert(key.to_owned(), secret.to_owned());
        if let Some(commit) = self.cancel_during_write.lock().unwrap().take() {
            commit.cancel();
        }
        Ok(())
    }

    fn remove(&self, key: &str) -> Result<(), CredentialError> {
        if self.fail_remove.load(Ordering::Relaxed) {
            return Err(CredentialError);
        }
        self.values.lock().unwrap().remove(key);
        Ok(())
    }
}

#[test]
fn cancelling_during_a_keyring_write_never_saves_or_selects_the_account() {
    runtime().block_on(async {
        let directory = TempRoot::new();
        let data = DataDir::isolated(directory.path()).unwrap();
        let keyring = CredentialMemory::default();
        let service = AccountService::new(data, &keyring).unwrap();
        let launcher = service.snapshot().await.unwrap().launcher();
        service.add_offline(launcher, "Previous", None).unwrap();
        let commit = SignInCommit::default();
        *keyring.cancel_during_write.lock().unwrap() = Some(commit.clone());
        assert!(matches!(
            service
                .remember_with_commit(launcher, verify().await, None, &commit)
                .await,
            Err(AccountError::Cancelled)
        ));
        assert_eq!(
            service.snapshot().await.unwrap().selected().unwrap().name(),
            "Previous"
        );
        assert!(keyring.values.lock().unwrap().is_empty());
        assert!(!service.file.read().unwrap().has_retired());
    });
}

#[test]
fn failed_reauthentication_keeps_the_saved_key_and_removal_retries_after_restart() {
    runtime().block_on(async {
        let directory = TempRoot::new();
        let data = DataDir::isolated(directory.path()).unwrap();
        let keyring = CredentialMemory::default();
        let service = AccountService::new(data.clone(), &keyring).unwrap();
        let launcher = service.snapshot().await.unwrap().launcher();
        service
            .remember(launcher, verify().await, None)
            .await
            .unwrap();
        assert!(!service.has_volatile_session().await);
        let id = service.snapshot().await.unwrap().selected().unwrap().id();
        let key = AccountKey::new(launcher, id);
        let old = service
            .file
            .read()
            .unwrap()
            .find(key)
            .unwrap()
            .credential
            .unwrap();
        keyring.fail_write.store(true, Ordering::Relaxed);
        service
            .remember(launcher, verify().await, Some(id))
            .await
            .unwrap();
        assert!(service.has_volatile_session().await);
        assert_eq!(
            service.snapshot().await.unwrap().persistence(id),
            Some(Persistence::SessionOnly)
        );
        let restarted = AccountService::new(data.clone(), &keyring).unwrap();
        assert_eq!(
            restarted.snapshot().await.unwrap().persistence(id),
            Some(Persistence::Saved)
        );
        assert_eq!(
            restarted.file.read().unwrap().find(key).unwrap().credential,
            Some(old)
        );
        keyring.fail_remove.store(true, Ordering::Relaxed);
        assert!(!restarted.remove(key).await.unwrap());
        assert!(restarted.file.read().unwrap().is_retired(old));
        keyring.fail_remove.store(false, Ordering::Relaxed);
        let restarted = AccountService::new(data, &keyring).unwrap();
        assert!(restarted.snapshot().await.unwrap().selected().is_none());
        assert!(keyring.values.lock().unwrap().is_empty());
        assert!(!restarted.file.read().unwrap().has_retired());
    });
}

#[test]
fn missing_credentials_and_keyring_read_failures_have_distinct_statuses() {
    runtime().block_on(async {
        let root = TempRoot::new();
        let keyring = CredentialMemory::default();
        let service =
            AccountService::new(DataDir::isolated(root.path()).unwrap(), &keyring).unwrap();
        let launcher = service.snapshot().await.unwrap().launcher();
        service
            .remember(launcher, verify().await, None)
            .await
            .unwrap();
        let id = service.snapshot().await.unwrap().selected().unwrap().id();
        keyring.fail_read.store(true, Ordering::Relaxed);
        assert_eq!(
            service.snapshot().await.unwrap().persistence(id),
            Some(Persistence::StoreUnavailable)
        );
        keyring.fail_read.store(false, Ordering::Relaxed);
        keyring.values.lock().unwrap().clear();
        assert_eq!(
            service.snapshot().await.unwrap().persistence(id),
            Some(Persistence::SignInRequired)
        );
    });
}

async fn verify() -> VerifiedAccount {
    BrowserLogin::begin()
        .finish(&Exchange::new(false, true), "code")
        .await
        .unwrap()
}

#[test]
fn cancellation_while_waiting_for_the_session_lock_prevents_persistence() {
    runtime().block_on(async {
        let directory = TempRoot::new();
        let data = DataDir::isolated(directory.path()).unwrap();
        let keyring = CredentialMemory::default();
        let service = AccountService::new(data.clone(), &keyring).unwrap();
        let launcher = service.snapshot().await.unwrap().launcher();
        let before = std::fs::read(path(&data)).unwrap();
        let commit = SignInCommit::default();
        let lock = service.sessions.lock().await;
        let mut saving =
            std::pin::pin!(service.remember_with_commit(launcher, verify().await, None, &commit));
        tokio::select! {
            biased;
            _ = &mut saving => panic!("save must wait for the mutex"),
            () = std::future::ready(()) => (),
        }
        commit.cancel();
        drop(lock);
        assert!(matches!(saving.await, Err(AccountError::Cancelled)));
        assert_eq!(std::fs::read(path(&data)).unwrap(), before);
        assert!(keyring.values.lock().unwrap().is_empty());
    });
}

struct Exchange {
    expected: Vec<(&'static str, &'static str)>,
    replies: SyncMutex<VecDeque<(&'static str, &'static str)>>,
    forms: SyncMutex<Vec<String>>,
}

const UUID: &str = "aeeee337fa3b48d1b2014866a1260456";
const SIGN_IN: [&str; 2] = [
    r#"{"access_token":"oauth-access","refresh_token":"first-refresh","expires_in":3600}"#,
    r#"{"access_token":"oauth-access","refresh_token":"rotated-refresh","expires_in":3600}"#,
];

impl Exchange {
    fn new(refresh: bool, licensed: bool) -> Self {
        Self::with_entitlements(
            refresh,
            licensed.then_some(r#"{"items":[{"name":"game_minecraft"}]}"#),
        )
    }

    fn with_entitlements(refresh: bool, entitlements: Option<&'static str>) -> Self {
        let mut expected = vec![
            ("/token", SIGN_IN[usize::from(refresh)]),
            ("/user/authenticate", r#"{"Token":"xbox"}"#),
            (
                "/xsts/authorize",
                r#"{"Token":"xsts","DisplayClaims":{"xui":[{"uhs":"hash","xid":"2533274790000000"}]}}"#,
            ),
            (
                "/launcher/login",
                r#"{"access_token":"minecraft-access","expires_in":3600}"#,
            ),
        ];
        match (refresh, entitlements) {
            (true, _) => (),
            (false, None) => expected.push(("/entitlements/license", r#"{"items":[]}"#)),
            (false, Some(body)) => expected.extend([
                ("/entitlements/license", body),
                (
                    "/minecraft/profile",
                    r#"{"id":"aeeee337fa3b48d1b2014866a1260456","name":"Example"}"#,
                ),
            ]),
        }
        Self {
            replies: SyncMutex::new(expected.clone().into()),
            expected,
            forms: SyncMutex::default(),
        }
    }

    fn all_requests_sent(&self) {
        assert!(
            self.replies.lock().unwrap().is_empty(),
            "{} expected requests",
            self.expected.len()
        );
    }
}

impl AuthNetwork for Exchange {
    async fn send(&self, request: Request<'_>) -> Result<Response, TransportError> {
        let (expected_path, body) = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected request");
        assert!(
            url::Url::parse(request.url)
                .unwrap()
                .path()
                .ends_with(expected_path)
        );
        if let Some(Body::Form(form)) = request.body {
            self.forms.lock().unwrap().push(form);
        }
        if matches!(
            expected_path,
            "/entitlements/license" | "/minecraft/profile"
        ) {
            assert_eq!(request.bearer, Some("minecraft-access"));
        }
        Ok(Response {
            status: 200,
            body: body.as_bytes().to_vec(),
        })
    }
}

#[test]
fn saves_offline_identity_and_protects_an_unknown_schema() {
    runtime().block_on(async {
        let directory = TempRoot::new();
        let data = DataDir::isolated(directory.path()).unwrap();
        let keyring = CredentialMemory::default();
        let service = AccountService::new(data.clone(), &keyring).unwrap();
        let launcher = service.snapshot().await.unwrap().launcher();
        service.add_offline(launcher, " Notch ", None).unwrap();
        let reopened = AccountService::new(data.clone(), &keyring).unwrap();
        let snapshot = reopened.snapshot().await.unwrap();
        let account = snapshot.selected().unwrap();
        assert_eq!(account.name(), "Notch");
        assert_eq!(
            account.id().uuid().to_string(),
            "b50ad385-829d-3141-a216-7e7d7539ba7f"
        );
        assert!(matches!(
            reopened.add_offline(launcher, "Another", Some(&account.id().uuid().to_string())),
            Err(AccountError::Duplicate)
        ));
        assert!(matches!(
            reopened.add_offline(launcher, "invalid name", None),
            Err(AccountError::InvalidName)
        ));
        assert!(keyring.values.lock().unwrap().is_empty());
        let newer = br#"{"schema_version":42,"new_fields":["keep"]}"#;
        std::fs::write(path(&data), newer).unwrap();
        assert!(matches!(
            reopened.select(launcher, None),
            Err(AccountError::UnsupportedSchema(42))
        ));
        assert_eq!(std::fs::read(path(&data)).unwrap(), newer);
    });
}

#[test]
fn saves_only_identity_in_json_and_coalesces_refresh_after_restart() {
    runtime().block_on(async {
        let directory = TempRoot::new();
        let data = DataDir::isolated(directory.path()).unwrap();
        let keyring = CredentialMemory::default();
        let service = AccountService::new(data.clone(), &keyring).unwrap();
        let launcher = service.snapshot().await.unwrap().launcher();
        let sign_in = Exchange::new(false, true);
        let verified = BrowserLogin::begin()
            .finish(&sign_in, "code")
            .await
            .unwrap();
        sign_in.all_requests_sent();
        assert!(sign_in.forms.lock().unwrap()[0].contains("code_verifier="));
        service.remember(launcher, verified, None).await.unwrap();
        let document = std::fs::read_to_string(path(&data)).unwrap();
        assert!(!document.contains("first-refresh"));
        assert!(!document.contains("minecraft-access"));
        let reopened = AccountService::new(data, &keyring).unwrap();
        let id = AccountId::Microsoft(Uuid::parse_str(UUID).unwrap());
        let key = AccountKey::new(launcher, id);
        let refresh = Exchange::new(true, true);
        let (first, second) = tokio::join!(
            reopened.resolve(key, &refresh),
            reopened.resolve(key, &refresh)
        );
        for result in [first, second] {
            assert!(matches!(result.unwrap(), LaunchIdentity::Microsoft { access_token, xuid, .. }
                if access_token == "minecraft-access" && xuid.as_deref() == Some("2533274790000000")));
        }
        refresh.all_requests_sent();
        assert!(refresh.forms.lock().unwrap()[0].contains("refresh_token=first-refresh"));
        assert_eq!(
            keyring
                .values
                .lock()
                .unwrap()
                .values()
                .next()
                .map(String::as_str),
            Some("rotated-refresh")
        );
        assert!(reopened.remove(key).await.unwrap());
        assert!(reopened.snapshot().await.unwrap().selected().is_none());
        assert!(keyring.values.lock().unwrap().is_empty());
    });
}

#[test]
fn product_minecraft_entitlement_allows_fetching_the_java_profile() {
    runtime().block_on(async {
        let sign_in =
            Exchange::with_entitlements(false, Some(r#"{"items":[{"name":"product_minecraft"}]}"#));
        BrowserLogin::begin()
            .finish(&sign_in, "code")
            .await
            .expect("product_minecraft grants Java Edition ownership");
        sign_in.all_requests_sent();
    });
}

#[test]
fn unlicensed_sign_in_stops_and_failed_keyring_keeps_verified_session_only() {
    runtime().block_on(async {
        let no_license = Exchange::new(false, false);
        assert!(matches!(
            BrowserLogin::begin().finish(&no_license, "code").await,
            Err(MicrosoftError::NoLicense)
        ));
        no_license.all_requests_sent();
        let directory = TempRoot::new();
        let data = DataDir::isolated(directory.path()).unwrap();
        let keyring = CredentialMemory {
            unavailable: true,
            ..CredentialMemory::default()
        };
        let service = AccountService::new(data.clone(), &keyring).unwrap();
        let launcher = service.snapshot().await.unwrap().launcher();
        let sign_in = Exchange::new(false, true);
        let verified = BrowserLogin::begin()
            .finish(&sign_in, "code")
            .await
            .unwrap();
        service.remember(launcher, verified, None).await.unwrap();
        let id = AccountId::Microsoft(Uuid::parse_str(UUID).unwrap());
        let key = AccountKey::new(launcher, id);
        assert_eq!(
            service.snapshot().await.unwrap().persistence(id),
            Some(Persistence::SessionOnly)
        );
        assert!(service.resolve(key, &sign_in).await.is_ok());
        let reopened = AccountService::new(data, &keyring).unwrap();
        assert_eq!(
            reopened.snapshot().await.unwrap().persistence(id),
            Some(Persistence::SignInRequired)
        );
        assert!(matches!(
            reopened.resolve(key, &sign_in).await,
            Err(AccountError::Microsoft(MicrosoftError::SignInAgain))
        ));
    });
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
}
