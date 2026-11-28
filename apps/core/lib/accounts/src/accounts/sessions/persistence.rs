// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Coordinates the account document with the system credential store.

use axlauncher_spi::credentials::CredentialStore;
use axlauncher_worker::accounts::Persistence;
use uuid::Uuid;

use super::{
    AccountError, AccountId, AccountKey, AccountService, Cached, CredentialRef, LauncherId,
    McAccount, MicrosoftError, SignInCommit, yggdrasil,
};

impl<S: CredentialStore> AccountService<S> {
    pub(super) fn persist(
        &self,
        launcher: LauncherId,
        account: McAccount,
        expected: Option<AccountId>,
        secret: &str,
        commit: &SignInCommit,
    ) -> Result<(Option<CredentialRef>, bool), AccountError> {
        if !commit.is_pending() {
            return Err(AccountError::Cancelled);
        }
        let result = self.persist_locked(launcher, account, expected, secret, commit);
        let _ = self.cleanup_retired();
        result
    }

    fn persist_locked(
        &self,
        launcher: LauncherId,
        mut account: McAccount,
        expected: Option<AccountId>,
        secret: &str,
        commit: &SignInCommit,
    ) -> Result<(Option<CredentialRef>, bool), AccountError> {
        let mut transaction = self.file.lock()?;
        if !commit.is_pending() {
            return Err(AccountError::Cancelled);
        }
        let target = transaction.root().launcher(launcher)?;
        let old = target
            .find(account.id)
            .ok()
            .and_then(|account| account.credential);
        if let Some(expected) = expected {
            target.find(expected)?;
        }
        let key = CredentialRef::Scoped {
            key: Uuid::new_v4(),
        };
        // Reserve before touching the keyring so a failed or interrupted save is recoverable.
        transaction.root_mut().retire([key]);
        transaction.save()?;
        let persisted = self
            .store
            .write(&self.file.credential_key(key), secret)
            .is_ok();
        commit.begin()?;
        let credential = if persisted { Some(key) } else { old };
        account.credential = credential;
        let root = transaction.root_mut();
        root.launcher_mut(launcher)?.remember(account);
        if persisted {
            root.keep_retired(|candidate| *candidate != key);
            root.retire(old);
        }
        transaction.save()?;
        Ok((credential, persisted))
    }

    pub(super) fn cleanup_retired(&self) -> Result<Vec<CredentialRef>, AccountError> {
        let mut transaction = self.file.lock()?;
        let root = transaction.root_mut();
        let before = root.retired().len();
        let live = root.live_credentials();
        root.keep_retired(|key| {
            !live.contains(key) && self.store.remove(&self.file.credential_key(*key)).is_err()
        });
        let remaining = root.retired().to_vec();
        if remaining.len() != before {
            transaction.save()?;
        }
        Ok(remaining)
    }

    pub(super) fn write_current_secret(
        &self,
        key: AccountKey,
        credential: CredentialRef,
        secret: &str,
    ) -> Result<bool, AccountError> {
        let transaction = self.file.lock()?;
        if transaction.root().find(key)?.credential != Some(credential) {
            return Err(AccountError::Missing);
        }
        Ok(self
            .store
            .write(&self.file.credential_key(credential), secret)
            .is_ok())
    }

    pub(super) fn persistence(&self, account: &McAccount, cached: Option<&Cached>) -> Persistence {
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
        match self.store.read(&self.file.credential_key(key)) {
            Ok(Some(secret)) if usable_credential(account.id, &secret) => Persistence::Saved,
            Ok(_) => Persistence::SignInRequired,
            Err(_) => Persistence::StoreUnavailable,
        }
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
