// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Account mutations and the in-memory cache of Minecraft access tokens.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use axlauncher_network_spi::auth::AuthNetwork;
use axlauncher_spi::credentials::CredentialStore;
use axlauncher_storage::{DataDir, DeviceUserId};
use axlauncher_worker::accounts::{Account as ViewAccount, AccountSnapshot};
use tokio::sync::Mutex;

use super::document::AccountFile;
use super::microsoft::{self, MicrosoftError, Tokens, VerifiedAccount};
use super::yggdrasil::{self, YggdrasilError};
use super::{
    AccountError, AccountId, AccountKey, CredentialRef, LauncherId, McAccount, SignInCommit,
};

mod persistence;

const REFRESH_RETRY_DELAY: Duration = Duration::from_secs(15);

/// The identity resolved for the next game process.
pub enum LaunchIdentity {
    Microsoft {
        account: McAccount,
        access_token: String,
        xuid: Option<String>,
    },
    Yggdrasil {
        account: McAccount,
        access_token: String,
    },
}

#[derive(Default)]
struct SessionCache {
    accounts: HashMap<AccountKey, Cached>,
}

#[derive(Default)]
struct Cached {
    credential: Option<CredentialRef>,
    tokens: Option<Tokens>,
    failure: Option<(Instant, MicrosoftError)>,
    session_only: bool,
    third_party: Option<(Instant, yggdrasil::Tokens)>,
    third_party_sign_in_required: bool,
}

/// Manages one data root and serializes credential changes with launch refreshes.
pub struct AccountService<S> {
    file: AccountFile,
    store: S,
    sessions: Mutex<SessionCache>,
}

impl<S: CredentialStore> AccountService<S> {
    pub fn new(data: DataDir, store: S) -> Result<Self, AccountError> {
        let owner = DeviceUserId::current().map_err(|_| AccountError::DeviceUser)?;
        Ok(Self::for_principal(data, store, owner.as_str().to_owned()))
    }

    fn for_principal(data: DataDir, store: S, principal: String) -> Self {
        Self {
            file: AccountFile::new(data, principal),
            store,
            sessions: Mutex::default(),
        }
    }

    pub async fn snapshot(&self) -> Result<AccountSnapshot, AccountError> {
        let sessions = self.sessions.lock().await;
        let root = self.file.read()?;
        if !root.retired().is_empty() {
            let _ = self.cleanup_retired();
        }
        let launcher = root.active();
        let accounts = launcher
            .accounts()
            .iter()
            .map(|account| {
                let key = AccountKey::new(launcher.id(), account.id);
                let status = account.id.is_online().then(|| {
                    let cached = sessions
                        .accounts
                        .get(&key)
                        .filter(|entry| entry.credential == account.credential);
                    self.persistence(account, cached)
                });
                ViewAccount::new(
                    account.id,
                    account.name.clone(),
                    account.server.clone(),
                    status,
                )
            })
            .collect();
        Ok(AccountSnapshot::new(
            root.summaries(),
            launcher.id(),
            accounts,
            launcher.selection(),
        ))
    }

    pub async fn has_volatile_session(&self) -> bool {
        self.sessions
            .lock()
            .await
            .accounts
            .values()
            .any(|cached| cached.session_only)
    }

    pub fn require_launcher(
        &self,
        launcher: LauncherId,
        expected: Option<AccountId>,
    ) -> Result<(), AccountError> {
        let root = self.file.read()?;
        let launcher = root.launcher(launcher)?;
        if let Some(id) = expected {
            launcher.find(id)?;
        }
        Ok(())
    }

    pub fn create_launcher(&self, name: &str) -> Result<(), AccountError> {
        self.file.update(|root| root.create_launcher(name))
    }

    pub fn select_launcher(&self, launcher: LauncherId) -> Result<(), AccountError> {
        self.file.update(|root| root.select_launcher(launcher))
    }

    pub fn add_offline(
        &self,
        launcher: LauncherId,
        name: &str,
        uuid: Option<&str>,
    ) -> Result<(), AccountError> {
        let account = McAccount::offline(name, uuid)?;
        self.file
            .update(|root| root.launcher_mut(launcher)?.add_offline(account))
    }

    pub fn select(&self, launcher: LauncherId, id: Option<AccountId>) -> Result<(), AccountError> {
        self.file
            .update(|root| root.launcher_mut(launcher)?.select(id))
    }

    /// Removes an account and reports whether its stored credential was cleared.
    pub async fn remove(&self, key: AccountKey) -> Result<bool, AccountError> {
        let mut sessions = self.sessions.lock().await;
        let credential = self.file.update(|root| root.remove(key))?;
        sessions.accounts.remove(&key);
        Ok(credential.is_none_or(|key| {
            self.cleanup_retired()
                .is_ok_and(|remaining| !remaining.contains(&key))
        }))
    }

    /// Saves a verified account or replaces the credentials for the expected identity.
    #[cfg(test)]
    pub async fn remember(
        &self,
        launcher: LauncherId,
        verified: VerifiedAccount,
        expected: Option<AccountId>,
    ) -> Result<(), AccountError> {
        self.remember_with_commit(launcher, verified, expected, &SignInCommit::default())
            .await
    }

    pub async fn remember_with_commit(
        &self,
        launcher: LauncherId,
        verified: VerifiedAccount,
        expected: Option<AccountId>,
        commit: &SignInCommit,
    ) -> Result<(), AccountError> {
        let id = AccountId::Microsoft(verified.profile.id);
        if expected.is_some_and(|expected| id != expected) {
            return Err(MicrosoftError::WrongAccount.into());
        }
        let secret = verified.session.refresh.clone();
        let account = McAccount {
            id,
            name: verified.profile.name,
            credential: None,
            server: None,
            extra: Default::default(),
        };
        self.save(
            launcher,
            account,
            expected,
            &secret,
            Cached {
                tokens: Some(verified.session),
                ..Cached::default()
            },
            commit,
        )
        .await
    }

    /// Saves a profile and its server-bound access and client tokens in the system credential store.
    #[cfg(test)]
    pub async fn remember_yggdrasil(
        &self,
        launcher: LauncherId,
        verified: yggdrasil::VerifiedAccount,
        expected: Option<AccountId>,
    ) -> Result<(), AccountError> {
        self.remember_yggdrasil_with_commit(launcher, verified, expected, &SignInCommit::default())
            .await
    }

    pub async fn remember_yggdrasil_with_commit(
        &self,
        launcher: LauncherId,
        verified: yggdrasil::VerifiedAccount,
        expected: Option<AccountId>,
        commit: &SignInCommit,
    ) -> Result<(), AccountError> {
        let id = yggdrasil::account_id(&verified.server, verified.profile.id());
        if expected.is_some_and(|expected| id != expected) {
            return Err(YggdrasilError::WrongProfile.into());
        }
        let account = McAccount {
            id,
            name: verified.profile.name().to_owned(),
            credential: None,
            server: Some(verified.server),
            extra: Default::default(),
        };
        let secret = serde_json::to_string(&verified.tokens).expect("Yggdrasil tokens serialize");
        let cached = Cached {
            third_party: Some((Instant::now(), verified.tokens)),
            ..Cached::default()
        };
        self.save(launcher, account, expected, &secret, cached, commit)
            .await
    }

    async fn save(
        &self,
        launcher: LauncherId,
        account: McAccount,
        expected: Option<AccountId>,
        secret: &str,
        mut cached: Cached,
        commit: &SignInCommit,
    ) -> Result<(), AccountError> {
        let mut sessions = self.sessions.lock().await;
        let key = AccountKey::new(launcher, account.id);
        let (credential, persisted) = self.persist(launcher, account, expected, secret, commit)?;
        cached.credential = credential;
        cached.session_only = !persisted;
        sessions.accounts.insert(key, cached);
        Ok(())
    }

    /// Resolves a saved offline player without opening an authentication client.
    pub fn offline_name(&self, key: AccountKey) -> Result<String, AccountError> {
        if key.id().is_online() {
            return Err(AccountError::Missing);
        }
        Ok(self.file.read()?.find(key)?.name.clone())
    }

    /// Resolves a saved online player and refreshes credentials when needed.
    pub async fn resolve(
        &self,
        key: AccountKey,
        network: &impl AuthNetwork,
    ) -> Result<LaunchIdentity, AccountError> {
        if !key.id().is_online() {
            return Err(AccountError::Missing);
        }
        let mut sessions = self.sessions.lock().await;
        let account = self.file.read()?.find(key)?.clone();
        let cached = sessions.accounts.entry(key).or_default();
        if cached.credential != account.credential {
            *cached = Cached {
                credential: account.credential,
                ..Cached::default()
            };
        }
        if matches!(key.id(), AccountId::Yggdrasil { .. }) {
            let access_token = self.yggdrasil_token(network, key, &account, cached).await?;
            return Ok(LaunchIdentity::Yggdrasil {
                account,
                access_token,
            });
        }
        let tokens = self.access_token(network, key, cached).await?;
        Ok(LaunchIdentity::Microsoft {
            account,
            access_token: tokens.access.clone(),
            xuid: tokens.xuid.clone(),
        })
    }

    async fn yggdrasil_token(
        &self,
        network: &impl AuthNetwork,
        key: AccountKey,
        account: &McAccount,
        cached: &mut Cached,
    ) -> Result<String, AccountError> {
        let server = account
            .server()
            .filter(|server| yggdrasil::account_id(server, account.id.uuid()) == account.id)
            .ok_or(YggdrasilError::InvalidResponse)?;
        if cached.third_party_sign_in_required {
            return Err(YggdrasilError::SignInAgain.into());
        }
        if let Some((checked, tokens)) = &cached.third_party
            && checked.elapsed() < yggdrasil::SESSION_TTL
        {
            return Ok(tokens.access_token.clone());
        }
        let tokens = self
            .read_yggdrasil(cached)
            .inspect_err(|error| mark_yggdrasil_failure(cached, error))?;
        let updated = yggdrasil::validate(network, server, &tokens, account.id.uuid())
            .await
            .map_err(AccountError::from)
            .inspect_err(|error| mark_yggdrasil_failure(cached, error))?;
        if let Some(credential) = cached.credential
            && (cached.session_only || updated.access_token != tokens.access_token)
        {
            let secret = serde_json::to_string(&updated).expect("Yggdrasil tokens serialize");
            cached.session_only = !self.write_current_secret(key, credential, &secret)?;
        }
        let access_token = updated.access_token.clone();
        cached.third_party = Some((Instant::now(), updated));
        Ok(access_token)
    }

    fn read_yggdrasil(&self, cached: &Cached) -> Result<yggdrasil::Tokens, AccountError> {
        if let Some((_, tokens)) = &cached.third_party {
            return Ok(tokens.clone());
        }
        let key = cached.credential.ok_or(YggdrasilError::SignInAgain)?;
        let secret = self
            .store
            .read(&self.file.credential_key(key))
            .map_err(|_| AccountError::CredentialStore)?;
        secret
            .and_then(|value| serde_json::from_str(&value).ok())
            .ok_or_else(|| YggdrasilError::SignInAgain.into())
    }

    async fn access_token<'a>(
        &self,
        network: &impl AuthNetwork,
        key: AccountKey,
        cached: &'a mut Cached,
    ) -> Result<&'a Tokens, AccountError> {
        if cached.tokens.as_ref().is_some_and(Tokens::is_current) {
            return Ok(cached.tokens.as_ref().expect("current Microsoft tokens"));
        }
        if let Some((time, error)) = &cached.failure
            && (matches!(error, MicrosoftError::SignInAgain)
                || time.elapsed() < REFRESH_RETRY_DELAY)
        {
            return cached.recover(error.clone());
        }
        let refresh = self.refresh_token(cached)?;
        let tokens = match microsoft::refresh(network, &refresh).await {
            Ok(tokens) => tokens,
            Err(error) => {
                cached.failure = Some((Instant::now(), error.clone()));
                return cached.recover(error);
            }
        };
        if let Some(credential) = cached.credential
            && (cached.session_only || tokens.refresh != refresh)
        {
            cached.session_only = !self.write_current_secret(key, credential, &tokens.refresh)?;
        }
        cached.tokens = Some(tokens);
        cached.failure = None;
        Ok(cached.tokens.as_ref().expect("refreshed Microsoft tokens"))
    }

    fn refresh_token(&self, cached: &mut Cached) -> Result<String, AccountError> {
        if let Some(tokens) = &cached.tokens {
            return Ok(tokens.refresh.clone());
        }
        let token = match cached.credential {
            Some(key) => self
                .store
                .read(&self.file.credential_key(key))
                .map_err(|_| AccountError::CredentialStore)?,
            None => None,
        };
        match token.filter(|token| !token.is_empty()) {
            Some(token) => Ok(token),
            None => {
                cached.failure = Some((Instant::now(), MicrosoftError::SignInAgain));
                Err(MicrosoftError::SignInAgain.into())
            }
        }
    }
}

fn mark_yggdrasil_failure(cached: &mut Cached, error: &AccountError) {
    cached.third_party_sign_in_required = matches!(
        error,
        AccountError::Yggdrasil(YggdrasilError::SignInAgain | YggdrasilError::WrongProfile)
    );
}

impl Cached {
    fn recover(&self, error: MicrosoftError) -> Result<&Tokens, AccountError> {
        if !matches!(error, MicrosoftError::SignInAgain)
            && let Some(tokens) = self.tokens.as_ref().filter(|tokens| tokens.is_valid())
        {
            return Ok(tokens);
        }
        Err(error.into())
    }
}

#[cfg(test)]
#[path = "../../../../../../tests/apps/core/lib/accounts/src/accounts/sessions.rs"]
mod tests;
