// SPDX-FileCopyrightText: 2026 Axolotl Team
// SPDX-License-Identifier: GPL-3.0-only

//! Account mutations and the in-memory cache of Minecraft access tokens.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use axlauncher_spi::auth::{AuthNetwork, CredentialStore};
use axlauncher_storage::DataDir;
use tokio::sync::Mutex;
use uuid::Uuid;

use super::microsoft::{self, MicrosoftError, Tokens, VerifiedAccount};
use super::yggdrasil::{self, YggdrasilError};
use super::{Account, AccountError, AccountId, Document, SignInCommit};
pub use axlauncher_worker::accounts::Persistence;

mod persistence;

const REFRESH_RETRY_DELAY: Duration = Duration::from_secs(15);

/// Account metadata and selection available to the desktop view.
pub struct AccountSnapshot {
    document: Document,
    persistence: HashMap<AccountId, Persistence>,
}

impl AccountSnapshot {
    #[cfg(test)]
    pub fn list(&self) -> &[Account] {
        &self.document.accounts
    }

    #[cfg(test)]
    pub fn selected(&self) -> Option<&Account> {
        self.document.find(self.document.selected?).ok()
    }

    #[cfg(test)]
    pub fn persistence(&self, id: AccountId) -> Option<Persistence> {
        self.persistence.get(&id).copied()
    }

    pub fn into_reply(self) -> axlauncher_worker::accounts::AccountSnapshot {
        use axlauncher_worker::accounts::{Account as ViewAccount, AccountSnapshot};

        let accounts = self
            .document
            .accounts
            .into_iter()
            .map(|account| {
                ViewAccount::new(
                    account.id,
                    account.name,
                    account.server,
                    self.persistence.get(&account.id).copied(),
                )
            })
            .collect();
        AccountSnapshot::new(accounts, self.document.selected)
    }
}

/// The identity resolved for the next game process.
pub enum LaunchIdentity {
    Offline(Account),
    Microsoft {
        account: Account,
        access_token: String,
        xuid: Option<String>,
    },
    Yggdrasil {
        account: Account,
        access_token: String,
    },
}

#[derive(Default)]
struct SessionCache {
    accounts: HashMap<AccountId, Cached>,
}

#[derive(Default)]
struct Cached {
    credential: Option<Uuid>,
    tokens: Option<Tokens>,
    failure: Option<(Instant, MicrosoftError)>,
    session_only: bool,
    third_party: Option<(Instant, yggdrasil::Tokens)>,
    third_party_sign_in_required: bool,
}

/// Manages one data root and serializes credential changes with launch refreshes.
pub struct AccountService<S> {
    data: DataDir,
    store: S,
    sessions: Mutex<SessionCache>,
}

impl<S: CredentialStore> AccountService<S> {
    pub fn new(data: DataDir, store: S) -> Self {
        Self {
            data,
            store,
            sessions: Mutex::default(),
        }
    }

    pub async fn snapshot(&self) -> Result<AccountSnapshot, AccountError> {
        let sessions = self.sessions.lock().await;
        let document = Document::load(&self.data)?;
        if !document.retired_credentials.is_empty() {
            let _ = self.cleanup_retired();
        }
        let persistence = document
            .accounts
            .iter()
            .filter(|account| account.id.is_online())
            .map(|account| {
                let cached = sessions
                    .accounts
                    .get(&account.id)
                    .filter(|entry| entry.credential == account.credential);
                (
                    account.id,
                    persistence::status(&self.store, account, cached),
                )
            })
            .collect();
        Ok(AccountSnapshot {
            document,
            persistence,
        })
    }

    pub async fn has_volatile_session(&self) -> bool {
        self.sessions
            .lock()
            .await
            .accounts
            .values()
            .any(|cached| cached.session_only)
    }

    pub fn add_offline(&self, name: &str, uuid: Option<&str>) -> Result<(), AccountError> {
        let account = Account::offline(name, uuid)?;
        Document::update(&self.data, |document| document.add_offline(account))
    }

    pub fn select(&self, id: Option<AccountId>) -> Result<(), AccountError> {
        Document::update(&self.data, |document| document.select(id))
    }

    /// Removes an account and reports whether its stored credential was cleared.
    pub async fn remove(&self, id: AccountId) -> Result<bool, AccountError> {
        let mut sessions = self.sessions.lock().await;
        let credential = Document::update(&self.data, |document| document.remove(id))?;
        sessions.accounts.remove(&id);
        Ok(credential.is_none_or(|key| {
            self.cleanup_retired()
                .is_ok_and(|remaining| !remaining.contains(&key))
        }))
    }

    /// Saves a verified account or replaces the credentials for the expected identity.
    #[cfg(test)]
    pub async fn remember(
        &self,
        verified: VerifiedAccount,
        expected: Option<AccountId>,
    ) -> Result<(), AccountError> {
        self.remember_with_commit(verified, expected, &SignInCommit::default())
            .await
    }

    pub async fn remember_with_commit(
        &self,
        verified: VerifiedAccount,
        expected: Option<AccountId>,
        commit: &SignInCommit,
    ) -> Result<(), AccountError> {
        let id = AccountId::Microsoft(verified.profile.id);
        if expected.is_some_and(|expected| id != expected) {
            return Err(MicrosoftError::WrongAccount.into());
        }
        let secret = verified.session.refresh.clone();
        let account = Account {
            id,
            name: verified.profile.name,
            credential: None,
            server: None,
        };
        self.save(
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
        verified: yggdrasil::VerifiedAccount,
        expected: Option<AccountId>,
    ) -> Result<(), AccountError> {
        self.remember_yggdrasil_with_commit(verified, expected, &SignInCommit::default())
            .await
    }

    pub async fn remember_yggdrasil_with_commit(
        &self,
        verified: yggdrasil::VerifiedAccount,
        expected: Option<AccountId>,
        commit: &SignInCommit,
    ) -> Result<(), AccountError> {
        let id = yggdrasil::account_id(&verified.server, verified.profile.id());
        if expected.is_some_and(|expected| id != expected) {
            return Err(YggdrasilError::WrongProfile.into());
        }
        let account = Account {
            id,
            name: verified.profile.name().to_owned(),
            credential: None,
            server: Some(verified.server),
        };
        let secret = serde_json::to_string(&verified.tokens).expect("Yggdrasil tokens serialize");
        let cached = Cached {
            third_party: Some((Instant::now(), verified.tokens)),
            ..Cached::default()
        };
        self.save(account, expected, &secret, cached, commit).await
    }

    async fn save(
        &self,
        account: Account,
        expected: Option<AccountId>,
        secret: &str,
        mut cached: Cached,
        commit: &SignInCommit,
    ) -> Result<(), AccountError> {
        let mut sessions = self.sessions.lock().await;
        let id = account.id;
        let (credential, persisted) = self.persist(account, expected, secret, commit)?;
        cached.credential = credential;
        cached.session_only = !persisted;
        sessions.accounts.insert(id, cached);
        Ok(())
    }

    /// Resolves the identity chosen at launch time, refreshing online credentials when needed.
    pub async fn resolve(
        &self,
        id: AccountId,
        network: &impl AuthNetwork,
    ) -> Result<LaunchIdentity, AccountError> {
        if matches!(id, AccountId::Offline(_)) {
            return Ok(LaunchIdentity::Offline(
                Document::load(&self.data)?.find(id)?.clone(),
            ));
        }
        let mut sessions = self.sessions.lock().await;
        let account = Document::load(&self.data)?.find(id)?.clone();
        let cached = sessions.accounts.entry(id).or_default();
        if cached.credential != account.credential {
            *cached = Cached {
                credential: account.credential,
                ..Cached::default()
            };
        }
        if matches!(id, AccountId::Yggdrasil { .. }) {
            let access_token = self.yggdrasil_token(network, &account, cached).await?;
            return Ok(LaunchIdentity::Yggdrasil {
                account,
                access_token,
            });
        }
        let tokens = self.access_token(network, id, cached).await?;
        Ok(LaunchIdentity::Microsoft {
            account,
            access_token: tokens.access.clone(),
            xuid: tokens.xuid.clone(),
        })
    }

    async fn yggdrasil_token(
        &self,
        network: &impl AuthNetwork,
        account: &Account,
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
        if let Some(key) = cached.credential
            && (cached.session_only || updated.access_token != tokens.access_token)
        {
            let secret = serde_json::to_string(&updated).expect("Yggdrasil tokens serialize");
            cached.session_only = !self.write_current_secret(account.id, key, &secret)?;
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
            .read(&key.to_string())
            .map_err(|_| AccountError::CredentialStore)?;
        secret
            .and_then(|value| serde_json::from_str(&value).ok())
            .ok_or_else(|| YggdrasilError::SignInAgain.into())
    }

    async fn access_token<'a>(
        &self,
        network: &impl AuthNetwork,
        id: AccountId,
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
        if let Some(key) = cached.credential
            && (cached.session_only || tokens.refresh != refresh)
        {
            cached.session_only = !self.write_current_secret(id, key, &tokens.refresh)?;
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
                .read(&key.to_string())
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
