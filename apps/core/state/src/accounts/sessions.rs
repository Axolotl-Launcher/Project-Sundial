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
use super::{Account, AccountError, AccountId, Document};

const REFRESH_RETRY_DELAY: Duration = Duration::from_secs(15);

/// Whether a saved Microsoft account has a usable persisted credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Persistence {
    Saved,
    SessionOnly,
    SignInRequired,
}

/// Account metadata and selection available to the desktop view.
pub struct AccountSnapshot {
    document: Document,
    persistence: HashMap<AccountId, Persistence>,
}

impl AccountSnapshot {
    pub fn list(&self) -> &[Account] {
        &self.document.accounts
    }

    pub fn selected(&self) -> Option<&Account> {
        self.document.find(self.document.selected?).ok()
    }

    pub fn persistence(&self, id: AccountId) -> Option<Persistence> {
        self.persistence.get(&id).copied()
    }
}

/// The identity resolved for the next game process.
pub enum LaunchIdentity {
    Offline(Account),
    Microsoft {
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
        let persistence = document
            .accounts
            .iter()
            .filter(|account| matches!(account.id, AccountId::Microsoft(_)))
            .map(|account| {
                let cached = sessions
                    .accounts
                    .get(&account.id)
                    .filter(|entry| entry.credential == account.credential);
                (account.id, persistence(account, cached))
            })
            .collect();
        Ok(AccountSnapshot {
            document,
            persistence,
        })
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
        Ok(self.discard(credential))
    }

    /// Saves a verified account or replaces the credentials for the expected identity.
    pub async fn remember(
        &self,
        verified: VerifiedAccount,
        expected: Option<AccountId>,
    ) -> Result<(), AccountError> {
        let mut sessions = self.sessions.lock().await;
        let id = AccountId::Microsoft(verified.profile.id);
        if expected.is_some_and(|expected| id != expected) {
            return Err(MicrosoftError::WrongAccount.into());
        }
        let key = Uuid::new_v4();
        let credential = self
            .store
            .write(&key.to_string(), &verified.session.refresh)
            .is_ok()
            .then_some(key);
        let account = Account {
            id,
            name: verified.profile.name,
            credential,
        };
        let previous =
            Document::update(&self.data, |document| document.remember(account, expected))
                .inspect_err(|_| {
                    self.discard(credential);
                })?;
        self.discard(previous);
        sessions.accounts.insert(
            id,
            Cached {
                credential,
                tokens: Some(verified.session),
                failure: None,
                session_only: credential.is_none(),
            },
        );
        Ok(())
    }

    fn discard(&self, credential: Option<Uuid>) -> bool {
        credential.is_none_or(|key| self.store.remove(&key.to_string()).is_ok())
    }

    /// Resolves the identity chosen at launch time, refreshing a Microsoft token when needed.
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
        let access_token = self.access_token(network, cached).await?;
        Ok(LaunchIdentity::Microsoft {
            account,
            access_token,
        })
    }

    async fn access_token(
        &self,
        network: &impl AuthNetwork,
        cached: &mut Cached,
    ) -> Result<String, AccountError> {
        if let Some(tokens) = cached.tokens.as_ref().filter(|tokens| tokens.is_current()) {
            return Ok(tokens.access.clone());
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
            cached.session_only = self.store.write(&key.to_string(), &tokens.refresh).is_err();
        }
        let access = tokens.access.clone();
        cached.tokens = Some(tokens);
        cached.failure = None;
        Ok(access)
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

impl Cached {
    fn recover(&self, error: MicrosoftError) -> Result<String, AccountError> {
        if !matches!(error, MicrosoftError::SignInAgain)
            && let Some(tokens) = self.tokens.as_ref().filter(|tokens| tokens.is_valid())
        {
            return Ok(tokens.access.clone());
        }
        Err(error.into())
    }
}

fn persistence(account: &Account, cached: Option<&Cached>) -> Persistence {
    if cached.is_some_and(|entry| matches!(entry.failure, Some((_, MicrosoftError::SignInAgain)))) {
        return Persistence::SignInRequired;
    }
    if cached.is_some_and(|entry| entry.session_only) {
        return Persistence::SessionOnly;
    }
    if account.credential.is_some() {
        return Persistence::Saved;
    }
    Persistence::SignInRequired
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/state/src/accounts/sessions.rs"]
mod tests;
