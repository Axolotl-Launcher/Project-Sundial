// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Coordinates the account document with the system credential store.

use std::collections::HashSet;

use axlauncher_spi::auth::CredentialStore;
use uuid::Uuid;

use super::{
    Account, AccountError, AccountId, AccountService, Cached, Document, MicrosoftError,
    Persistence, SignInCommit, yggdrasil,
};

impl<S: CredentialStore> AccountService<S> {
    pub(super) fn persist(
        &self,
        account: Account,
        expected: Option<AccountId>,
        secret: &str,
        commit: &SignInCommit,
    ) -> Result<(Option<Uuid>, bool), AccountError> {
        let result = self.persist_locked(account, expected, secret, commit);
        let _ = self.cleanup_retired();
        result
    }

    fn persist_locked(
        &self,
        mut account: Account,
        expected: Option<AccountId>,
        secret: &str,
        commit: &SignInCommit,
    ) -> Result<(Option<Uuid>, bool), AccountError> {
        let _lock = self.data.lock_accounts().map_err(AccountError::Write)?;
        if !commit.is_pending() {
            return Err(AccountError::Cancelled);
        }
        let mut document = Document::load(&self.data)?;
        let old = document
            .find(account.id)
            .ok()
            .and_then(|account| account.credential);
        if let Some(expected) = expected {
            document.find(expected)?;
        }
        let key = Uuid::new_v4();
        // Reserve before touching the keyring so a failed or interrupted save is recoverable.
        document.retired_credentials.push(key);
        document.save(&self.data)?;
        let persisted = self.store.write(&key.to_string(), secret).is_ok();
        commit.begin()?;
        let credential = if persisted { Some(key) } else { old };
        account.credential = credential;
        document.remember(account);
        if persisted {
            document
                .retired_credentials
                .retain(|candidate| *candidate != key);
            document.retired_credentials.extend(old);
        }
        document.save(&self.data)?;
        Ok((credential, persisted))
    }

    pub(super) fn cleanup_retired(&self) -> Result<Vec<Uuid>, AccountError> {
        let _lock = self.data.lock_accounts().map_err(AccountError::Write)?;
        let mut document = Document::load(&self.data)?;
        let before = document.retired_credentials.len();
        let live = document
            .accounts
            .iter()
            .filter_map(|account| account.credential)
            .collect::<HashSet<_>>();
        document
            .retired_credentials
            .retain(|key| !live.contains(key) && self.store.remove(&key.to_string()).is_err());
        if document.retired_credentials.len() != before {
            document.save(&self.data)?;
        }
        Ok(document.retired_credentials)
    }

    pub(super) fn write_current_secret(
        &self,
        id: AccountId,
        key: Uuid,
        secret: &str,
    ) -> Result<bool, AccountError> {
        let _lock = self.data.lock_accounts().map_err(AccountError::Write)?;
        let document = Document::load(&self.data)?;
        if document.find(id)?.credential != Some(key) {
            return Err(AccountError::Missing);
        }
        Ok(self.store.write(&key.to_string(), secret).is_ok())
    }
}

pub(super) fn status(
    store: &impl CredentialStore,
    account: &Account,
    cached: Option<&Cached>,
) -> Persistence {
    if cached.is_some_and(|entry| {
        entry.third_party_sign_in_required
            || matches!(entry.failure, Some((_, MicrosoftError::SignInAgain)))
    }) {
        return Persistence::SignInRequired;
    }
    if cached.is_some_and(|entry| entry.session_only) {
        return Persistence::SessionOnly;
    }
    let Some(key) = account.credential else {
        return Persistence::SignInRequired;
    };
    match store.read(&key.to_string()) {
        Ok(Some(secret)) if usable_credential(account.id, &secret) => Persistence::Saved,
        Ok(_) => Persistence::SignInRequired,
        Err(_) => Persistence::StoreUnavailable,
    }
}

fn usable_credential(id: AccountId, secret: &str) -> bool {
    match id {
        AccountId::Yggdrasil { .. } => {
            serde_json::from_str::<yggdrasil::Tokens>(secret).is_ok_and(|tokens| tokens.is_usable())
        }
        _ => !secret.is_empty(),
    }
}
