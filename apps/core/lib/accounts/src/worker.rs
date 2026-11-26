use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use axlauncher_network::auth::AuthHttp;
use axlauncher_spi::ServiceHost;
use axlauncher_spi::auth::{AuthNetwork, CredentialStore};
use axlauncher_storage::{DataDir, SystemCredentials};
use axlauncher_worker::accounts::{AccountId, Action, Change, Command, Identity, Reply};
use axlauncher_worker::{Cancellation, Responder, serve};
use base64::{Engine as _, engine::general_purpose::STANDARD};

use crate::accounts::{AccountService, LaunchIdentity, SignInCommit, yggdrasil};

mod login;

#[allow(
    unsafe_code,
    reason = "The launcher resolves a fixed versioned C ABI export."
)]
#[unsafe(no_mangle)]
pub extern "C" fn axlauncher_accounts_v1() -> i32 {
    run()
}

fn run() -> i32 {
    let account_worker = RefCell::new(None);
    serve(move |command: Command, reply, cancel| {
        let (root, action) = command.into_parts();
        let worker = for_root(&account_worker, root);
        async move { worker?.handle(action, reply, cancel).await }
    })
}

type WorkerSlot = RefCell<Option<(PathBuf, Rc<Accounts<SystemCredentials>>)>>;

fn for_root(slot: &WorkerSlot, root: PathBuf) -> Result<Rc<Accounts<SystemCredentials>>, String> {
    let mut slot = slot.borrow_mut();
    if let Some((existing, worker)) = &*slot {
        if *existing != root {
            return Err("The account worker belongs to a different data directory.".into());
        }
        return Ok(Rc::clone(worker));
    }
    if !root.is_absolute() {
        return Err("The account data directory is not absolute.".into());
    }
    let data = DataDir::isolated(&root).map_err(|error| error.to_string())?;
    let worker = Rc::new(Accounts::new(data, SystemCredentials));
    *slot = Some((root, Rc::clone(&worker)));
    Ok(worker)
}

struct Accounts<Store> {
    service: AccountService<Store>,
    choices: RefCell<login::Choices>,
}

impl<Store: CredentialStore + 'static> Accounts<Store> {
    fn new(data: DataDir, store: Store) -> Self {
        Self {
            service: AccountService::new(data, store),
            choices: RefCell::new(login::Choices::default()),
        }
    }

    async fn handle(
        self: Rc<Self>,
        action: Action,
        reply: Responder<Reply>,
        cancel: Cancellation,
    ) -> Result<(), String> {
        let commit = SignInCommit::reporting(cancel.clone(), move || {
            let _ = reply.send(Reply::Saving);
        });
        let outcome = self.dispatch(action, reply, &cancel, &commit).await;
        self.update_retention(reply.retention()).await;
        outcome
    }

    async fn dispatch(
        self: &Rc<Self>,
        action: Action,
        reply: Responder<Reply>,
        cancel: &Cancellation,
        commit: &SignInCommit,
    ) -> Result<(), String> {
        match action {
            Action::Change(change) => self.change(change, reply, cancel).await,
            Action::Microsoft {
                method,
                expected,
                proxy,
            } => self.microsoft(method, expected, proxy, reply, commit).await,
            Action::Yggdrasil {
                url,
                username,
                password,
                expected,
            } => {
                self.third_party(
                    login::ThirdParty::SignIn {
                        url,
                        username,
                        password,
                    },
                    expected,
                    reply,
                    commit,
                )
                .await
            }
            Action::Choose {
                choice,
                profile,
                expected,
            } => {
                self.third_party(
                    login::ThirdParty::Choose {
                        choice,
                        profile: profile.id(),
                    },
                    expected,
                    reply,
                    commit,
                )
                .await
            }
            Action::DiscardChoice(choice) => {
                self.choices.borrow_mut().remove(&choice);
                Ok(())
            }
            Action::Resolve { id, proxy } => self.resolve(id, proxy, reply, cancel).await,
        }
    }

    async fn change(
        &self,
        change: Change,
        reply: Responder<Reply>,
        cancel: &Cancellation,
    ) -> Result<(), String> {
        if !cancel.is_pending() || (!matches!(change, Change::Read) && !cancel.begin_commit()) {
            return reply.send(Reply::Cancelled);
        }
        let result = match change {
            Change::Read => Ok(None),
            Change::AddOffline { name, uuid } => self.service.add_offline(&name, Some(&uuid)).map(|()| None),
            Change::Select(id) => self.service.select(id).map(|()| None),
            Change::Remove(id) => self.service.remove(id).await.map(|cleared| {
                (!cleared).then_some("The account was removed. The launcher will retry clearing its system credential.")
            }),
        };
        let notice = match result {
            Ok(notice) => notice.map(str::to_owned),
            Err(error) => Some(error.to_string()),
        };
        self.snapshot(reply, notice).await
    }

    async fn snapshot(
        &self,
        reply: Responder<Reply>,
        notice: Option<String>,
    ) -> Result<(), String> {
        let snapshot = self
            .service
            .snapshot()
            .await
            .map_err(|error| error.to_string())?;
        reply.send(Reply::Snapshot {
            snapshot: snapshot.into_reply(),
            notice,
        })
    }

    async fn resolve(
        &self,
        id: AccountId,
        proxy: Option<ServiceHost>,
        reply: Responder<Reply>,
        cancel: &Cancellation,
    ) -> Result<(), String> {
        let network = auth(proxy)?;
        reply.send(self.launch_reply(id, &network, cancel).await?)
    }

    async fn launch_reply(
        &self,
        id: AccountId,
        network: &impl AuthNetwork,
        cancel: &Cancellation,
    ) -> Result<Reply, String> {
        let identity = tokio::select! {
            biased;
            () = cancel.cancelled() => return Ok(Reply::Cancelled),
            result = self.launch_identity(id, network) => result?,
        };
        Ok(if cancel.begin_commit() {
            Reply::Identity(identity)
        } else {
            Reply::Cancelled
        })
    }

    async fn launch_identity(
        &self,
        id: AccountId,
        network: &impl AuthNetwork,
    ) -> Result<Identity, String> {
        let identity = self
            .service
            .resolve(id, network)
            .await
            .map_err(|error| error.to_string())?;
        Ok(match identity {
            LaunchIdentity::Offline(account) => Identity::Offline {
                name: account.name().to_owned(),
            },
            LaunchIdentity::Microsoft {
                account,
                access_token,
                xuid,
            } => Identity::Microsoft {
                name: account.name().to_owned(),
                access_token,
                xuid,
            },
            LaunchIdentity::Yggdrasil {
                account,
                access_token,
            } => {
                let server = account
                    .server()
                    .ok_or("The third-party account is missing its server.")?;
                let metadata = yggdrasil::launch_metadata(network, server)
                    .await
                    .map_err(|error| error.to_string())?;
                Identity::Yggdrasil {
                    name: account.name().to_owned(),
                    access_token,
                    url: server.url().to_owned(),
                    metadata: STANDARD.encode(metadata),
                }
            }
        })
    }

    async fn update_retention(&self, retention: axlauncher_worker::Retention<Reply>) {
        let volatile = self.service.has_volatile_session().await;
        retention.set(volatile || !self.choices.borrow().is_empty());
    }
}

fn auth(proxy: Option<ServiceHost>) -> Result<AuthHttp, String> {
    AuthHttp::desktop()
        .map(|network| network.with_fallen_proxy(proxy))
        .map_err(|_| "Could not initialize online sign-in.".to_owned())
}

#[cfg(test)]
#[path = "../../../../../tests/apps/core/lib/accounts/src/worker.rs"]
mod tests;
