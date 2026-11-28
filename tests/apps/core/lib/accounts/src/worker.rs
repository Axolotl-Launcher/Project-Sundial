use std::future::pending;
use std::time::Duration;

use axlauncher_network_spi::auth::{Body, Request, Response, TransportError};
use axlauncher_spi::credentials::CredentialError;
use serde_json::{Value, json};
use tokio::sync::Notify;

use super::*;

const ROOT: &str = "https://accounts.example.test/yggdrasil";

struct SignInProvider;

impl AuthNetwork for SignInProvider {
    async fn send(&self, request: Request<'_>) -> Result<Response, TransportError> {
        let body = match request.body {
            None => json!({"meta": {}}),
            Some(Body::Json(payload)) => {
                let payload: Value = serde_json::from_str(&payload).unwrap();
                json!({
                    "clientToken": payload["clientToken"],
                    "accessToken": "test-access-token",
                    "selectedProfile": {
                        "id": "d35cea1bfd1244bdab192183f93a50b6",
                        "name": "ExamplePlayer"
                    }
                })
            }
            Some(Body::Form(_)) => panic!("unexpected form request"),
        };
        Ok(Response::new(200, serde_json::to_vec(&body).unwrap()))
    }
}

struct PendingMetadata(Notify);

impl AuthNetwork for PendingMetadata {
    async fn send(&self, request: Request<'_>) -> Result<Response, TransportError> {
        assert_eq!(request.url, ROOT);
        assert!(request.body.is_none());
        self.0.notify_one();
        pending().await
    }
}

struct UnavailableCredentials;

impl CredentialStore for UnavailableCredentials {
    fn read(&self, _key: &str) -> Result<Option<String>, CredentialError> {
        Err(CredentialError)
    }

    fn write(&self, _key: &str, _secret: &str) -> Result<(), CredentialError> {
        Err(CredentialError)
    }

    fn remove(&self, _key: &str) -> Result<(), CredentialError> {
        Err(CredentialError)
    }
}

#[tokio::test]
async fn cancelling_launch_metadata_returns_no_identity() {
    let root = tempfile::tempdir().unwrap();
    let accounts = Accounts::new(
        DataDir::isolated(root.path()).unwrap(),
        UnavailableCredentials,
    )
    .unwrap();
    let launcher = accounts.service.snapshot().await.unwrap().launcher();
    let sign_in = yggdrasil::sign_in(&SignInProvider, ROOT, "example", "test-password")
        .await
        .unwrap();
    let yggdrasil::SignIn::Complete(account) = sign_in else {
        panic!("expected a verified profile");
    };
    accounts
        .service
        .remember_yggdrasil(launcher, account, None)
        .await
        .unwrap();
    let snapshot = accounts.service.snapshot().await.unwrap();
    let key = snapshot.selected_key().unwrap();
    let network = PendingMetadata(Notify::new());
    let cancel = Cancellation::default();
    let cancel_request = async {
        network.0.notified().await;
        cancel.cancel();
    };
    let (reply, ()) = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::join!(
            accounts.launch_reply(key, &network, &cancel),
            cancel_request
        )
    })
    .await
    .expect("cancellation did not interrupt the metadata request");
    assert!(matches!(reply, Ok(Reply::Cancelled)));
}
