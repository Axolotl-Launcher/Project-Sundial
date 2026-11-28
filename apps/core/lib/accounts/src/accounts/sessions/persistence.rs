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
        let _ = self.cleanup_retired(None);
        result
    }

    fn persist_locked(
        &self,
        launcher: LauncherId,
        account: McAccount,
        expected: Option<AccountId>,
        secret: &str,
        commit: &SignInCommit,
    ) -> Result<(Option<CredentialRef>, bool), AccountError> {
        let mut transaction = self.file.lock()?;
        if !commit.is_pending() {
            return Err(AccountError::Cancelled);
        }
        let old = match transaction.credential(AccountKey::new(launcher, account.id)) {
            Ok(credential) => credential,
            Err(AccountError::Missing) => None,
            Err(error) => return Err(error),
        };
        if let Some(expected) = expected {
            transaction.credential(AccountKey::new(launcher, expected))?;
        }
        let key = CredentialRef::Scoped {
            key: Uuid::new_v4(),
        };
        // Reserve before touching the keyring so a failed or interrupted save is recoverable.
        transaction.update(|root| {
            root.retire([key]);
            Ok(())
        })?;
        let persisted = self
            .store
            .write(&self.file.credential_key(key), secret)
            .is_ok();
        commit.begin()?;
        let fresh = persisted.then_some(key);
        let credential = transaction.update(|root| root.remember(launcher, account, old, fresh))?;
        Ok((credential, persisted))
    }

    pub(super) fn cleanup_retired(
        &self,
        target: Option<CredentialRef>,
    ) -> Result<bool, AccountError> {
        self.file.lock()?.cleanup_retired(target, |key| {
            self.store.remove(&self.file.credential_key(key)).is_ok()
        })
    }

    pub(super) fn write_current_secret(
        &self,
        key: AccountKey,
        credential: CredentialRef,
        secret: &str,
    ) -> Result<bool, AccountError> {
        let transaction = self.file.lock()?;
        if transaction.credential(key)? != Some(credential) {
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
