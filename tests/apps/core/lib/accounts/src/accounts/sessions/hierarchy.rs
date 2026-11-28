use std::fs;

use axlauncher_worker::accounts::Server;
use serde_json::{Value, json};

use super::*;

const OWNER: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const OTHER: &str = "2222222222222222222222222222222222222222222222222222222222222222";

#[tokio::test]
async fn anonymous_and_named_launchers_have_independent_minecraft_lists_and_selections() {
    let directory = TempRoot::new();
    let data = DataDir::isolated(directory.path()).unwrap();
    let keyring = CredentialMemory::default();
    let service = AccountService::for_principal(data.clone(), &keyring, OWNER.to_owned());
    let empty = service.snapshot().await.unwrap();
    let anonymous = empty.launcher();
    assert_eq!(empty.launchers().len(), 1);
    assert!(empty.launchers().next().unwrap().name().is_none());
    assert!(empty.selected().is_none());
    assert_eq!(empty.iter().len(), 0);

    service.add_offline(anonymous, "Player", None).unwrap();
    let first = service.snapshot().await.unwrap().selected_key().unwrap();
    service.create_launcher("Second").unwrap();
    let snapshot = service.snapshot().await.unwrap();
    let second = snapshot.launcher();
    assert_ne!(anonymous, second);
    assert_eq!(
        snapshot
            .launchers()
            .find(|launcher| launcher.id() == second)
            .unwrap()
            .name(),
        Some("Second")
    );
    assert_eq!(snapshot.iter().len(), 0);
    assert!(matches!(
        service.select(second, Some(first.id())),
        Err(AccountError::Missing)
    ));
    assert!(matches!(
        service.require_launcher(second, Some(first.id())),
        Err(AccountError::Missing)
    ));
    service.add_offline(second, "Player", None).unwrap();
    let other = service.snapshot().await.unwrap().selected_key().unwrap();
    assert_eq!(first.id(), other.id());
    assert_ne!(first, other);
    assert!(service.remove(other).await.unwrap());
    assert!(matches!(
        service.offline_name(other),
        Err(AccountError::Missing)
    ));
    assert_eq!(service.offline_name(first).unwrap(), "Player");

    service.select_launcher(anonymous).unwrap();
    let reopened = AccountService::for_principal(data, &keyring, OWNER.to_owned());
    assert_eq!(
        reopened.snapshot().await.unwrap().selected_key(),
        Some(first)
    );
}

#[tokio::test]
async fn a_pending_sign_in_cannot_follow_a_new_selection_or_reuse_another_launchers_session() {
    let directory = TempRoot::new();
    let data = DataDir::isolated(directory.path()).unwrap();
    let keyring = CredentialMemory::default();
    let service = AccountService::for_principal(data, &keyring, OWNER.to_owned());
    let first = service.snapshot().await.unwrap().launcher();
    let mut verified = verify().await;
    verified.session.access = "first-scoped-session".into();
    let id = AccountId::Microsoft(Uuid::parse_str(UUID).unwrap());
    service.create_launcher("Second").unwrap();
    let second = service.snapshot().await.unwrap().launcher();

    keyring.fail_write.store(true, Ordering::Relaxed);
    service.remember(first, verified, None).await.unwrap();
    let snapshot = service.snapshot().await.unwrap();
    assert_eq!(snapshot.launcher(), second);
    assert_eq!(snapshot.iter().len(), 0);
    assert!(matches!(
        service
            .resolve(AccountKey::new(second, id), &Exchange::new(true, true))
            .await,
        Err(AccountError::Missing)
    ));
    let mut verified = verify().await;
    verified.session.access = "second-scoped-session".into();
    service.remember(second, verified, None).await.unwrap();
    service.select_launcher(first).unwrap();
    let snapshot = service.snapshot().await.unwrap();
    assert_eq!(snapshot.persistence(id), Some(Persistence::SessionOnly));
    assert!(matches!(
        service.resolve(snapshot.selected_key().unwrap(), &Exchange::new(true, true)).await.unwrap(),
        LaunchIdentity::Microsoft { access_token, .. } if access_token == "first-scoped-session"
    ));
    assert!(matches!(
        service.resolve(AccountKey::new(second, id), &Exchange::new(true, true)).await.unwrap(),
        LaunchIdentity::Microsoft { access_token, .. } if access_token == "second-scoped-session"
    ));
}

#[tokio::test]
async fn shared_data_directory_keeps_metadata_keys_and_cleanup_scoped_to_the_current_root() {
    let directory = TempRoot::new();
    let data = DataDir::isolated(directory.path()).unwrap();
    let keyring = CredentialMemory::default();
    let owner = AccountService::for_principal(data.clone(), &keyring, OWNER.to_owned());
    let other = AccountService::for_principal(data, &keyring, OTHER.to_owned());
    let owner_launcher = owner.snapshot().await.unwrap().launcher();
    owner
        .remember(owner_launcher, verify().await, None)
        .await
        .unwrap();
    let owner_key = owner.snapshot().await.unwrap().selected_key().unwrap();
    let other_snapshot = other.snapshot().await.unwrap();
    assert_eq!(other_snapshot.launchers().len(), 1);
    assert_eq!(other_snapshot.iter().len(), 0);

    assert!(matches!(
        other.select_launcher(owner_launcher),
        Err(AccountError::MissingLauncher)
    ));
    assert!(matches!(
        other.add_offline(owner_launcher, "Player", None),
        Err(AccountError::MissingLauncher)
    ));
    assert!(matches!(
        other.remove(owner_key).await,
        Err(AccountError::MissingLauncher)
    ));
    assert!(matches!(
        other.resolve(owner_key, &Exchange::new(true, true)).await,
        Err(AccountError::MissingLauncher)
    ));
    assert!(matches!(
        other.remember(owner_launcher, verify().await, None).await,
        Err(AccountError::MissingLauncher)
    ));
    assert_eq!(
        owner.snapshot().await.unwrap().selected_key(),
        Some(owner_key)
    );

    keyring.fail_remove.store(true, Ordering::Relaxed);
    assert!(!owner.remove(owner_key).await.unwrap());
    keyring.fail_remove.store(false, Ordering::Relaxed);
    other
        .remember(other_snapshot.launcher(), verify().await, None)
        .await
        .unwrap();
    assert_eq!(keyring.values.lock().unwrap().len(), 2);
    assert_eq!(owner.snapshot().await.unwrap().iter().len(), 0);
    let keys = keyring.values.lock().unwrap();
    assert_eq!(keys.len(), 1);
    assert!(keys.keys().all(|key| key.starts_with(OTHER)));
}

#[tokio::test]
async fn opening_flat_schemas_preserves_each_provider_and_reuses_the_existing_keyring() {
    for schema in 1..=3 {
        verify_migration(schema).await;
    }
}

async fn verify_migration(schema: u32) {
    let directory = TempRoot::new();
    let data = DataDir::isolated(directory.path()).unwrap();
    let file = path(&data);
    let keyring = CredentialMemory::default();
    let (flat, selected, credential) = legacy_document(schema);
    fs::write(&file, serde_json::to_vec(&flat).unwrap()).unwrap();
    (&keyring)
        .write(&credential.to_string(), "first-refresh")
        .unwrap();
    let third_party = &flat["accounts"][2];
    (&keyring)
        .write(
            third_party["credential"].as_str().unwrap(),
            r#"{"accessToken":"third-party","clientToken":"test-client"}"#,
        )
        .unwrap();
    let service = AccountService::for_principal(data.clone(), &keyring, OWNER.to_owned());
    let snapshot = service.snapshot().await.unwrap();
    let key = snapshot.selected_key().unwrap();
    assert_eq!(key.id(), selected);
    assert_eq!(snapshot.persistence(selected), Some(Persistence::Saved));
    let third_party_id = serde_json::from_value(third_party["id"].clone()).unwrap();
    assert_eq!(
        snapshot.persistence(third_party_id),
        Some(Persistence::Saved)
    );
    assert_eq!(snapshot.iter().len(), 3);
    assert_eq!(snapshot.launchers().len(), 1);
    assert!(snapshot.launchers().next().unwrap().name().is_none());

    let migrated: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    assert_eq!(migrated["schema_version"], 4);
    assert!(migrated.get("accounts").is_none());
    assert_eq!(migrated["unrelated"], flat["unrelated"]);
    assert_eq!(
        migrated["device_roots"][OWNER]["launcher_accounts"][0]["mc_accounts"],
        flat["accounts"]
    );
    let refresh = Exchange::new(true, true);
    let reopened = AccountService::for_principal(data.clone(), &keyring, OWNER.to_owned());
    assert_eq!(reopened.snapshot().await.unwrap().selected_key(), Some(key));
    assert!(matches!(
        reopened.resolve(key, &refresh).await.unwrap(),
        LaunchIdentity::Microsoft { .. }
    ));
    refresh.all_requests_sent();
    assert_eq!(
        (&keyring).read(&credential.to_string()).unwrap().as_deref(),
        Some("rotated-refresh")
    );
    let other = AccountService::for_principal(data, &keyring, OTHER.to_owned());
    assert_eq!(other.snapshot().await.unwrap().iter().len(), 0);
}

fn legacy_document(schema: u32) -> (Value, AccountId, Uuid) {
    let selected = AccountId::Microsoft(Uuid::parse_str(UUID).unwrap());
    let credential = Uuid::new_v4();
    let server = Server::new(
        "https://example.test/api/yggdrasil".into(),
        "Example".into(),
    );
    let third_party = yggdrasil::account_id(&server, Uuid::new_v4());
    let accounts = json!([
        { "id": { "kind": "offline", "uuid": Uuid::new_v4() }, "name": "Local" },
        { "id": selected, "name": "Online", "credential": credential, "profile_note": 1 },
        { "id": third_party, "name": "ThirdParty", "server": server, "credential": Uuid::new_v4() }
    ]);
    (
        json!({
            "schema_version": schema,
            "accounts": accounts,
            "selected": selected,
            "unrelated": {"keep": true}
        }),
        selected,
        credential,
    )
}

#[tokio::test]
async fn a_failed_migration_leaves_the_flat_file_intact_for_retry() {
    let directory = TempRoot::new();
    let data = DataDir::isolated(directory.path()).unwrap();
    let file = path(&data);
    let staging = file.with_extension("json.tmp");
    let bytes = serde_json::to_vec(&legacy_document(3).0).unwrap();
    fs::write(&file, &bytes).unwrap();
    fs::create_dir(&staging).unwrap();
    let keyring = CredentialMemory::default();
    let service = AccountService::for_principal(data, &keyring, OWNER.to_owned());
    assert!(matches!(
        service.snapshot().await,
        Err(AccountError::Write(_))
    ));
    assert_eq!(fs::read(&file).unwrap(), bytes);
    fs::remove_dir(&staging).unwrap();
    assert_eq!(service.snapshot().await.unwrap().iter().len(), 3);
}

#[tokio::test]
async fn current_schema_rejects_dangling_selection_and_duplicate_ownership_without_rewriting() {
    let directory = TempRoot::new();
    let data = DataDir::isolated(directory.path()).unwrap();
    let file = path(&data);
    let keyring = CredentialMemory::default();
    let service = AccountService::for_principal(data, &keyring, OWNER.to_owned());
    let launcher = service.snapshot().await.unwrap().launcher();
    let valid: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    let mut dangling = valid.clone();
    dangling["device_roots"][OWNER]["selected_launcher"] = json!(Uuid::new_v4());
    let mut duplicate = valid;
    let launchers = duplicate["device_roots"][OWNER]["launcher_accounts"]
        .as_array_mut()
        .unwrap();
    launchers.push(launchers[0].clone());

    for invalid in [dangling, duplicate] {
        let bytes = serde_json::to_vec(&invalid).unwrap();
        fs::write(&file, &bytes).unwrap();
        assert!(matches!(
            service.select_launcher(launcher),
            Err(AccountError::InvalidHierarchy)
        ));
        assert_eq!(fs::read(&file).unwrap(), bytes);
    }
}
