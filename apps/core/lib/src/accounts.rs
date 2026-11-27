//! Account view data and private account requests, without credential-store references.

use std::path::PathBuf;

use axlauncher_network_spi::ServiceHost;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use crate::Cancellation as SignInCommit;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "uuid", rename_all = "snake_case")]
pub enum AccountId {
    Offline(Uuid),
    Microsoft(Uuid),
    Yggdrasil { profile: Uuid, server: Uuid },
}

impl AccountId {
    pub fn uuid(self) -> Uuid {
        match self {
            Self::Offline(uuid) | Self::Microsoft(uuid) | Self::Yggdrasil { profile: uuid, .. } => {
                uuid
            }
        }
    }

    pub fn is_online(self) -> bool {
        !matches!(self, Self::Offline(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Server {
    url: String,
    name: String,
}

impl Server {
    pub fn new(url: String, name: String) -> Self {
        Self { url, name }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    id: AccountId,
    name: String,
    server: Option<Server>,
    persistence: Option<Persistence>,
}

impl Account {
    pub fn new(
        id: AccountId,
        name: String,
        server: Option<Server>,
        persistence: Option<Persistence>,
    ) -> Self {
        Self {
            id,
            name,
            server,
            persistence,
        }
    }

    pub fn id(&self) -> AccountId {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn server(&self) -> Option<&Server> {
        self.server.as_ref()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Persistence {
    Saved,
    SessionOnly,
    SignInRequired,
    StoreUnavailable,
}

#[derive(Serialize, Deserialize)]
pub struct AccountSnapshot {
    accounts: Vec<Account>,
    selected: Option<AccountId>,
}

impl AccountSnapshot {
    pub fn new(accounts: Vec<Account>, selected: Option<AccountId>) -> Self {
        Self { accounts, selected }
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &Account> {
        self.accounts.iter()
    }

    pub fn selected(&self) -> Option<&Account> {
        let selected = self.selected?;
        self.accounts
            .iter()
            .find(|account| account.id() == selected)
    }

    pub fn persistence(&self, id: AccountId) -> Option<Persistence> {
        self.accounts
            .iter()
            .find(|account| account.id() == id)?
            .persistence
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Profile {
    id: Uuid,
    name: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChoiceId(Uuid);

impl ChoiceId {
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }
}

impl Profile {
    pub fn new(id: Uuid, name: String) -> Self {
        Self { id, name }
    }

    pub fn id(&self) -> Uuid {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Clone, Copy, Serialize, Deserialize)]
pub enum Method {
    Browser,
    Device,
}

#[derive(Serialize, Deserialize)]
pub struct Command {
    #[serde(with = "crate::path")]
    root: PathBuf,
    action: Action,
}

impl Command {
    pub fn new(root: PathBuf, action: Action) -> Self {
        Self { root, action }
    }

    pub fn into_parts(self) -> (PathBuf, Action) {
        (self.root, self.action)
    }
}

#[derive(Serialize, Deserialize)]
pub enum Change {
    Read,
    AddOffline { name: String, uuid: String },
    Select(Option<AccountId>),
    Remove(AccountId),
}

#[derive(Serialize, Deserialize)]
pub enum Action {
    Change(Change),
    Microsoft {
        method: Method,
        expected: Option<AccountId>,
        proxy: Option<ServiceHost>,
    },
    Yggdrasil {
        url: String,
        username: String,
        password: String,
        expected: Option<AccountId>,
    },
    Choose {
        choice: ChoiceId,
        profile: Profile,
        expected: Option<AccountId>,
    },
    DiscardChoice(ChoiceId),
    Resolve {
        id: AccountId,
        proxy: Option<ServiceHost>,
    },
}

#[derive(Serialize, Deserialize)]
pub enum Identity {
    Offline {
        name: String,
    },
    Microsoft {
        name: String,
        access_token: String,
        xuid: Option<String>,
    },
    Yggdrasil {
        name: String,
        access_token: String,
        url: String,
        metadata: String,
    },
}

#[derive(Serialize, Deserialize)]
pub enum Reply {
    Snapshot {
        snapshot: AccountSnapshot,
        notice: Option<String>,
    },
    Browser {
        url: String,
    },
    Device {
        url: String,
        code: String,
    },
    Profiles {
        choice: ChoiceId,
        profiles: Vec<Profile>,
    },
    Identity(Identity),
    Saving,
    Cancelled,
}
