//! Ownership and selection in the current device user's account hierarchy.

use std::collections::HashSet;

use axlauncher_worker::accounts::LauncherAccount as LauncherSummary;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use uuid::Uuid;

use super::{AccountError, AccountId, AccountKey, CredentialRef, LauncherId, McAccount};

const MAX_LAUNCHER_NAME_LENGTH: usize = 48;

#[derive(Default, Serialize, Deserialize)]
pub(super) struct DeviceRootUser {
    launcher_accounts: Vec<LauncherAccount>,
    selected_launcher: Option<LauncherId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    retired_credentials: Vec<CredentialRef>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

impl DeviceRootUser {
    pub(super) fn from_flat(
        accounts: Vec<McAccount>,
        selected: Option<AccountId>,
        retired_credentials: Vec<CredentialRef>,
        extra: Map<String, Value>,
    ) -> Self {
        let launcher = LauncherAccount::from_flat(accounts, selected);
        Self {
            selected_launcher: Some(launcher.id()),
            launcher_accounts: vec![launcher],
            retired_credentials,
            extra,
        }
    }

    pub(super) fn ensure_local(&mut self) -> bool {
        if !self.launcher_accounts.is_empty() {
            return false;
        }
        self.insert_launcher(None);
        true
    }

    pub(super) fn validate(&self) -> Result<(), AccountError> {
        let mut launchers = HashSet::new();
        let mut credentials = HashSet::new();
        for launcher in &self.launcher_accounts {
            launchers
                .insert(launcher.id())
                .then_some(())
                .ok_or(AccountError::InvalidHierarchy)?;
            launcher.validate(&mut credentials)?;
        }
        let anonymous_count = self
            .launcher_accounts
            .iter()
            .filter(|launcher| launcher.is_anonymous())
            .count();
        let valid = if launchers.is_empty() {
            self.selected_launcher.is_none()
        } else {
            anonymous_count == 1
                && self
                    .selected_launcher
                    .is_some_and(|id| launchers.contains(&id))
        };
        valid.then_some(()).ok_or(AccountError::InvalidHierarchy)
    }

    pub(super) fn summaries(&self) -> Vec<LauncherSummary> {
        self.launcher_accounts
            .iter()
            .map(LauncherAccount::summary)
            .collect()
    }

    pub(super) fn active(&self) -> &LauncherAccount {
        self.launcher(
            self.selected_launcher
                .expect("initialized launcher selection"),
        )
        .expect("validated launcher selection")
    }

    pub(super) fn launcher(&self, id: LauncherId) -> Result<&LauncherAccount, AccountError> {
        self.launcher_accounts
            .iter()
            .find(|launcher| launcher.id() == id)
            .ok_or(AccountError::MissingLauncher)
    }

    pub(super) fn launcher_mut(
        &mut self,
        id: LauncherId,
    ) -> Result<&mut LauncherAccount, AccountError> {
        self.launcher_accounts
            .iter_mut()
            .find(|launcher| launcher.id() == id)
            .ok_or(AccountError::MissingLauncher)
    }

    pub(super) fn create_launcher(&mut self, name: &str) -> Result<(), AccountError> {
        let name = name.trim();
        if !(1..=MAX_LAUNCHER_NAME_LENGTH).contains(&name.chars().count())
            || name.chars().any(char::is_control)
        {
            return Err(AccountError::InvalidLauncherName);
        }
        if self
            .launcher_accounts
            .iter()
            .any(|launcher| launcher.is_named(name))
        {
            return Err(AccountError::DuplicateLauncherName);
        }
        self.insert_launcher(Some(name.to_owned()));
        Ok(())
    }

    fn insert_launcher(&mut self, name: Option<String>) {
        let launcher = LauncherAccount::new(name);
        self.selected_launcher = Some(launcher.id());
        self.launcher_accounts.push(launcher);
    }

    pub(super) fn select_launcher(&mut self, id: LauncherId) -> Result<(), AccountError> {
        self.launcher(id)?;
        self.selected_launcher = Some(id);
        Ok(())
    }

    pub(super) fn find(&self, key: AccountKey) -> Result<&McAccount, AccountError> {
        self.launcher(key.launcher())?.find(key.id())
    }

    pub(super) fn remove(
        &mut self,
        key: AccountKey,
    ) -> Result<Option<CredentialRef>, AccountError> {
        let credential = self.launcher_mut(key.launcher())?.remove(key.id())?;
        self.retire(credential);
        Ok(credential)
    }

    pub(super) fn retire(&mut self, credential: impl IntoIterator<Item = CredentialRef>) {
        self.retired_credentials.extend(credential);
    }

    pub(super) fn remember(
        &mut self,
        launcher: LauncherId,
        mut account: McAccount,
        old: Option<CredentialRef>,
        fresh: Option<CredentialRef>,
    ) -> Result<Option<CredentialRef>, AccountError> {
        let credential = fresh.or(old);
        account.credential = credential;
        self.launcher_mut(launcher)?.remember(account);
        if let Some(fresh) = fresh {
            self.retired_credentials
                .retain(|candidate| *candidate != fresh);
            self.retire(old);
        }
        Ok(credential)
    }

    pub(super) fn has_retired(&self) -> bool {
        !self.retired_credentials.is_empty()
    }

    pub(super) fn is_retired(&self, credential: CredentialRef) -> bool {
        self.retired_credentials.contains(&credential)
    }

    pub(super) fn prune_retired(&mut self, mut remove: impl FnMut(CredentialRef) -> bool) -> bool {
        let before = self.retired_credentials.len();
        let live: HashSet<_> = self
            .launcher_accounts
            .iter()
            .flat_map(LauncherAccount::credentials)
            .collect();
        self.retired_credentials
            .retain(|key| !live.contains(key) && !remove(*key));
        self.retired_credentials.len() != before
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct LauncherAccount {
    id: LauncherId,
    name: Option<String>,
    mc_accounts: Vec<McAccount>,
    selected: Option<AccountId>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

impl LauncherAccount {
    fn new(name: Option<String>) -> Self {
        Self {
            id: LauncherId::new(Uuid::new_v4()),
            name,
            mc_accounts: Vec::new(),
            selected: None,
            extra: Map::new(),
        }
    }

    fn from_flat(accounts: Vec<McAccount>, selected: Option<AccountId>) -> Self {
        Self {
            selected: selected.filter(|id| accounts.iter().any(|account| account.id == *id)),
            mc_accounts: accounts,
            ..Self::new(None)
        }
    }

    pub(super) fn id(&self) -> LauncherId {
        self.id
    }

    fn is_anonymous(&self) -> bool {
        self.name.is_none()
    }

    fn is_named(&self, name: &str) -> bool {
        self.name
            .as_ref()
            .is_some_and(|existing| existing.eq_ignore_ascii_case(name))
    }

    fn summary(&self) -> LauncherSummary {
        LauncherSummary::new(self.id, self.name.clone())
    }

    pub(super) fn accounts(&self) -> impl ExactSizeIterator<Item = &McAccount> {
        self.mc_accounts.iter()
    }

    fn credentials(&self) -> impl Iterator<Item = CredentialRef> {
        self.accounts().filter_map(|account| account.credential)
    }

    pub(super) fn selection(&self) -> Option<AccountId> {
        self.selected
    }

    fn validate(&self, credentials: &mut HashSet<CredentialRef>) -> Result<(), AccountError> {
        let mut identities = HashSet::new();
        for account in self.accounts() {
            account
                .has_expected_provider()
                .then_some(())
                .ok_or(AccountError::InvalidServer)?;
            let unique = identities.insert(account.id)
                && account.credential.is_none_or(|key| credentials.insert(key));
            unique.then_some(()).ok_or(AccountError::InvalidHierarchy)?;
        }
        if self.selected.is_some_and(|id| !identities.contains(&id)) {
            return Err(AccountError::InvalidHierarchy);
        }
        Ok(())
    }

    pub(super) fn find(&self, id: AccountId) -> Result<&McAccount, AccountError> {
        self.mc_accounts
            .iter()
            .find(|account| account.id == id)
            .ok_or(AccountError::Missing)
    }

    pub(super) fn add_offline(&mut self, account: McAccount) -> Result<(), AccountError> {
        if self.find(account.id).is_ok() {
            return Err(AccountError::Duplicate);
        }
        self.selected = Some(account.id);
        self.mc_accounts.push(account);
        Ok(())
    }

    pub(super) fn select(&mut self, id: Option<AccountId>) -> Result<(), AccountError> {
        if let Some(id) = id {
            self.find(id)?;
        }
        self.selected = id;
        Ok(())
    }

    fn remove(&mut self, id: AccountId) -> Result<Option<CredentialRef>, AccountError> {
        let credential = self.find(id)?.credential;
        self.mc_accounts.retain(|account| account.id != id);
        self.selected = self.selected.filter(|selected| *selected != id);
        Ok(credential)
    }

    fn remember(&mut self, mut account: McAccount) {
        if let Some(index) = self
            .mc_accounts
            .iter()
            .position(|existing| existing.id == account.id)
        {
            account.extra = self.mc_accounts.remove(index).extra;
        }
        self.selected = Some(account.id);
        self.mc_accounts.push(account);
    }
}
