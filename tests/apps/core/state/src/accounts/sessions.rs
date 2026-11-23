// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

use std::collections::VecDeque;
use std::sync::Mutex as SyncMutex;

use axlauncher_spi::auth::{Body, CredentialError, Request, Response, TransportError};

use super::*;
use crate::accounts::microsoft::BrowserLogin;
use crate::test_support::TempRoot;

#[derive(Default)]
struct CredentialMemory {
    values: SyncMutex<HashMap<String, String>>,
    unavailable: bool,
}

impl CredentialStore for &CredentialMemory {
    fn read(&self, key: &str) -> Result<Option<String>, CredentialError> {
        Ok(self.values.lock().unwrap().get(key).cloned())
    }

    fn write(&self, key: &str, secret: &str) -> Result<(), CredentialError> {
        if self.unavailable {
            return Err(CredentialError);
        }
        self.values
            .lock()
            .unwrap()
            .insert(key.to_owned(), secret.to_owned());
        Ok(())
    }

    fn remove(&self, key: &str) -> Result<(), CredentialError> {
        self.values.lock().unwrap().remove(key);
        Ok(())
    }
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
        let mut expected = vec![
            ("/token", SIGN_IN[usize::from(refresh)]),
            ("/user/authenticate", r#"{"Token":"xbox"}"#),
            (
                "/xsts/authorize",
                r#"{"Token":"xsts","DisplayClaims":{"xui":[{"uhs":"hash"}]}}"#,
            ),
            (
                "/launcher/login",
                r#"{"access_token":"minecraft-access","expires_in":3600}"#,
            ),
        ];
        match (refresh, licensed) {
            (true, _) => (),
            (false, false) => expected.push(("/entitlements/license", r#"{"items":[]}"#)),
            (false, true) => expected.extend([
                (
                    "/entitlements/license",
                    r#"{"items":[{"name":"game_minecraft"}]}"#,
                ),
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
        let data = DataDir::new(directory.path());
        let keyring = CredentialMemory::default();
        let service = AccountService::new(data.clone(), &keyring);
        service.add_offline(" Notch ", None).unwrap();
        let reopened = AccountService::new(data.clone(), &keyring);
        let snapshot = reopened.snapshot().await.unwrap();
        let account = snapshot.selected().unwrap();
        assert_eq!(account.name(), "Notch");
        assert_eq!(
            account.id().uuid().to_string(),
            "b50ad385-829d-3141-a216-7e7d7539ba7f"
        );
        assert!(matches!(
            reopened.add_offline("Another", Some(&account.id().uuid().to_string())),
            Err(AccountError::Duplicate)
        ));
        assert!(matches!(
            reopened.add_offline("invalid name", None),
            Err(AccountError::InvalidName)
        ));
        assert!(keyring.values.lock().unwrap().is_empty());
        let newer = br#"{"schema_version":42,"new_fields":["keep"]}"#;
        std::fs::write(data.accounts(), newer).unwrap();
        assert!(matches!(
            reopened.select(None),
            Err(AccountError::UnsupportedSchema(42))
        ));
        assert_eq!(std::fs::read(data.accounts()).unwrap(), newer);
    });
}

#[test]
fn saves_only_identity_in_json_and_coalesces_refresh_after_restart() {
    runtime().block_on(async {
        let directory = TempRoot::new();
        let data = DataDir::new(directory.path());
        let keyring = CredentialMemory::default();
        let service = AccountService::new(data.clone(), &keyring);
        let sign_in = Exchange::new(false, true);
        let verified = BrowserLogin::begin()
            .finish(&sign_in, "code")
            .await
            .unwrap();
        sign_in.all_requests_sent();
        assert!(sign_in.forms.lock().unwrap()[0].contains("code_verifier="));
        service.remember(verified, None).await.unwrap();
        let document = std::fs::read_to_string(data.accounts()).unwrap();
        assert!(!document.contains("first-refresh"));
        assert!(!document.contains("minecraft-access"));
        let reopened = AccountService::new(data, &keyring);
        let id = AccountId::Microsoft(Uuid::parse_str(UUID).unwrap());
        let refresh = Exchange::new(true, true);
        let (first, second) = tokio::join!(
            reopened.resolve(id, &refresh),
            reopened.resolve(id, &refresh)
        );
        for result in [first, second] {
            assert!(matches!(result.unwrap(), LaunchIdentity::Microsoft { access_token, .. } if access_token == "minecraft-access"));
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
        assert!(reopened.remove(id).await.unwrap());
        assert!(reopened.snapshot().await.unwrap().selected().is_none());
        assert!(keyring.values.lock().unwrap().is_empty());
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
        let data = DataDir::new(directory.path());
        let keyring = CredentialMemory {
            unavailable: true,
            ..CredentialMemory::default()
        };
        let service = AccountService::new(data.clone(), &keyring);
        let sign_in = Exchange::new(false, true);
        let verified = BrowserLogin::begin()
            .finish(&sign_in, "code")
            .await
            .unwrap();
        service.remember(verified, None).await.unwrap();
        let id = AccountId::Microsoft(Uuid::parse_str(UUID).unwrap());
        assert_eq!(
            service.snapshot().await.unwrap().persistence(id),
            Some(Persistence::SessionOnly)
        );
        assert!(service.resolve(id, &sign_in).await.is_ok());
        let reopened = AccountService::new(data, &keyring);
        assert_eq!(
            reopened.snapshot().await.unwrap().persistence(id),
            Some(Persistence::SignInRequired)
        );
        assert!(matches!(
            reopened.resolve(id, &sign_in).await,
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
