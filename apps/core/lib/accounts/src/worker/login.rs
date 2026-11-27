use std::collections::HashMap;
use std::rc::Rc;

use axlauncher_network::auth::{AuthHttp, BrowserCallback, CallbackError};
use axlauncher_network_spi::ServiceHost;
use axlauncher_spi::credentials::CredentialStore;
use axlauncher_worker::Responder;
use axlauncher_worker::accounts::{AccountId, ChoiceId, Method, Profile, Reply};
use uuid::Uuid;

use crate::accounts::microsoft::{
    BrowserLogin, CALLBACK_PATH, CALLBACK_PORT, DeviceLogin, VerifiedAccount,
};
use crate::accounts::{AccountError, SignInCommit, yggdrasil};

use super::{Accounts, auth};

pub(super) type Choices = HashMap<ChoiceId, yggdrasil::ProfileChoice>;

pub(super) enum ThirdParty {
    SignIn {
        url: String,
        username: String,
        password: String,
    },
    Choose {
        choice: ChoiceId,
        profile: Uuid,
    },
}

impl<Store: CredentialStore + 'static> Accounts<Store> {
    pub(super) async fn microsoft(
        &self,
        method: Method,
        expected: Option<AccountId>,
        proxy: Option<ServiceHost>,
        reply: Responder<Reply>,
        commit: &SignInCommit,
    ) -> Result<(), String> {
        let network = auth(proxy)?;
        let verified = tokio::select! {
            biased;
            () = commit.cancelled() => return reply.send(Reply::Cancelled),
            verified = authorize(method, &network, reply) => verified?,
        };
        self.finish(
            self.service
                .remember_with_commit(verified, expected, commit)
                .await,
            reply,
        )
        .await
    }

    pub(super) async fn third_party(
        self: &Rc<Self>,
        action: ThirdParty,
        expected: Option<AccountId>,
        reply: Responder<Reply>,
        commit: &SignInCommit,
    ) -> Result<(), String> {
        let network = auth(None)?;
        let outcome = tokio::select! {
            biased;
            () = commit.cancelled() => return reply.send(Reply::Cancelled),
            result = self.authenticate(action, &network) => result?,
        };
        match outcome {
            yggdrasil::SignIn::Choose(choice) if commit.is_pending() => self.offer(choice, reply),
            yggdrasil::SignIn::Choose(_) => reply.send(Reply::Cancelled),
            yggdrasil::SignIn::Complete(verified) => {
                self.finish(
                    self.service
                        .remember_yggdrasil_with_commit(verified, expected, commit)
                        .await,
                    reply,
                )
                .await
            }
        }
    }

    async fn finish(
        &self,
        outcome: Result<(), AccountError>,
        reply: Responder<Reply>,
    ) -> Result<(), String> {
        match outcome {
            Ok(()) => self.snapshot(reply, None).await,
            Err(AccountError::Cancelled) => reply.send(Reply::Cancelled),
            Err(error) => Err(error.to_string()),
        }
    }

    async fn authenticate(
        &self,
        action: ThirdParty,
        network: &AuthHttp,
    ) -> Result<yggdrasil::SignIn, String> {
        let outcome = match action {
            ThirdParty::SignIn {
                url,
                username,
                password,
            } => yggdrasil::sign_in(network, &url, &username, &password).await,
            ThirdParty::Choose { choice, profile } => {
                let choice = self
                    .choices
                    .borrow_mut()
                    .remove(&choice)
                    .ok_or(yggdrasil::YggdrasilError::Expired)
                    .map_err(|error| error.to_string())?;
                choice
                    .finish(network, profile)
                    .await
                    .map(yggdrasil::SignIn::Complete)
            }
        };
        outcome.map_err(|error| error.to_string())
    }

    fn offer(
        self: &Rc<Self>,
        choice: yggdrasil::ProfileChoice,
        reply: Responder<Reply>,
    ) -> Result<(), String> {
        let profiles = choice
            .profiles()
            .iter()
            .map(|profile| Profile::new(profile.id(), profile.name().to_owned()))
            .collect();
        let id = ChoiceId::new(Uuid::new_v4());
        reply.send(Reply::Profiles {
            choice: id,
            profiles,
        })?;
        self.choices.borrow_mut().insert(id, choice);
        let accounts = Rc::clone(self);
        let retention = reply.retention();
        tokio::task::spawn_local(accounts.expire_choice(id, retention));
        Ok(())
    }

    async fn expire_choice(
        self: Rc<Self>,
        id: ChoiceId,
        retention: axlauncher_worker::Retention<Reply>,
    ) {
        tokio::time::sleep(yggdrasil::CHOICE_TTL).await;
        if self.choices.borrow_mut().remove(&id).is_some() {
            self.update_retention(retention).await;
        }
    }
}

async fn authorize(
    method: Method,
    network: &AuthHttp,
    reply: Responder<Reply>,
) -> Result<VerifiedAccount, String> {
    match method {
        Method::Browser => {
            let flow = BrowserLogin::begin();
            let callback = BrowserCallback::bind(CALLBACK_PORT, CALLBACK_PATH)
                .await
                .map_err(callback_message)?;
            reply.send(Reply::Browser {
                url: flow.url().to_owned(),
            })?;
            let code = callback
                .receive(flow.state())
                .await
                .map_err(callback_message)?;
            flow.finish(network, &code)
                .await
                .map_err(|error| error.to_string())
        }
        Method::Device => {
            let flow = DeviceLogin::begin(network)
                .await
                .map_err(|error| error.to_string())?;
            reply.send(Reply::Device {
                url: flow.url().to_owned(),
                code: flow.user_code().to_owned(),
            })?;
            flow.finish(network)
                .await
                .map_err(|error| error.to_string())
        }
    }
}

fn callback_message(error: CallbackError) -> String {
    match error {
        CallbackError::Unavailable => {
            "The browser callback port is unavailable. Try signing in with a device code."
        }
        CallbackError::TimedOut => "Browser sign-in timed out. Start sign-in again.",
        CallbackError::Denied => "Microsoft sign-in was declined.",
    }
    .into()
}
