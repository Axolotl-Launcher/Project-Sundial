//! Desktop account operations backed by the embedded account process.

use std::sync::{Arc, Mutex, OnceLock, Weak};

use crate::{Event, Image, Service};
use axlauncher_spi::ServiceHost;
use axlauncher_storage::DataDir;

use crate::accounts::{
    AccountId, AccountSnapshot, Action, Change, ChoiceId, Command, Identity, Reply, SignInCommit,
};

static IMAGE: OnceLock<Image> = OnceLock::new();
static ACCOUNTS: Mutex<Weak<DesktopAccounts>> = Mutex::new(Weak::new());

pub(crate) fn configure(image: Image) {
    let _ = IMAGE.set(image);
}

pub struct DesktopAccounts {
    data: Mutex<DataDir>,
    worker: Option<Service<Reply>>,
}

impl DesktopAccounts {
    pub(crate) fn close_active() {
        let service = ACCOUNTS.lock().expect("active account service").upgrade();
        if let Some(service) = service
            && let Some(worker) = &service.worker
        {
            worker.close_and_wait();
        }
    }

    pub fn new(data: DataDir) -> Arc<Self> {
        let service = Arc::new(Self {
            data: Mutex::new(data),
            worker: IMAGE.get().copied().map(Service::new),
        });
        *ACCOUNTS.lock().expect("active account service") = Arc::downgrade(&service);
        service
    }

    pub fn data_dir(&self) -> DataDir {
        self.data.lock().expect("account paths").clone()
    }

    pub fn update_paths(&self, data: &DataDir) {
        let mut previous = self.data.lock().expect("account paths");
        assert_eq!(previous.root(), data.root());
        *previous = data.clone();
    }

    pub fn close(&self) {
        if let Some(worker) = &self.worker {
            worker.close();
        }
    }

    pub fn discard_choice(&self, choice: ChoiceId) {
        if let Some(worker) = &self.worker {
            let data = self.data_dir();
            worker.notify_if_running(Command::new(
                data.root().to_path_buf(),
                Action::DiscardChoice(choice),
            ));
        }
    }

    pub async fn change(
        &self,
        change: Change,
    ) -> Result<(AccountSnapshot, Option<String>), String> {
        match self.exchange(Action::Change(change), None, |_| {}).await? {
            Some(Reply::Snapshot { snapshot, notice }) => Ok((snapshot, notice)),
            _ => Err("The account worker sent an invalid snapshot.".into()),
        }
    }

    pub async fn sign_in(
        &self,
        action: Action,
        commit: &SignInCommit,
        stage: impl FnMut(Reply),
    ) -> Result<Option<Reply>, String> {
        let choice = match &action {
            Action::Choose { choice, .. } => Some(*choice),
            _ => None,
        };
        let result = self.exchange(action, Some(commit), stage).await;
        if !commit.is_pending()
            && let Some(choice) = choice
        {
            self.discard_choice(choice);
        }
        result
    }

    pub async fn resolve(
        &self,
        id: AccountId,
        proxy: Option<ServiceHost>,
    ) -> Result<Identity, String> {
        match self
            .exchange(Action::Resolve { id, proxy }, None, |_| {})
            .await?
        {
            Some(Reply::Identity(identity)) => Ok(identity),
            _ => Err("The account worker sent an invalid identity.".into()),
        }
    }

    async fn exchange(
        &self,
        action: Action,
        mut commit: Option<&SignInCommit>,
        mut stage: impl FnMut(Reply),
    ) -> Result<Option<Reply>, String> {
        if commit.is_some_and(|commit| !commit.is_pending()) {
            return Ok(None);
        }
        let worker = self
            .worker
            .as_ref()
            .ok_or("The embedded account worker is unavailable.")?;
        let data = self.data_dir();
        let command = Command::new(data.root().to_path_buf(), action);
        let mut request = worker.request(&data, command)?;
        let mut final_reply = None;
        loop {
            let next = match commit {
                Some(cancellation) => tokio::select! {
                    biased;
                    () = cancellation.cancelled() => {
                        request.cancel();
                        commit = None;
                        continue;
                    }
                    reply = request.next() => reply,
                },
                None => request.next().await,
            };
            match next {
                Some(Event::Reply(
                    reply @ (Reply::Saving | Reply::Browser { .. } | Reply::Device { .. }),
                )) => stage(reply),
                Some(Event::Reply(Reply::Cancelled)) => final_reply = None,
                Some(Event::Reply(reply)) => final_reply = Some(reply),
                Some(Event::Finished(result)) => return result.map(|()| final_reply),
                None => return Err("The account worker closed its reply pipe.".into()),
            }
        }
    }
}
