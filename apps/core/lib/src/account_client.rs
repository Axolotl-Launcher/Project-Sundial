//! Desktop account operations backed by the embedded account process.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use crate::{Event, Image, Service};
use axlauncher_network_spi::ServiceHost;
use axlauncher_storage::{DataDir, DataPath};

use crate::accounts::{
    AccountKey, AccountSnapshot, Action, Change, ChoiceId, Command, Identity, Reply, SignInCommit,
};

static IMAGE: OnceLock<(Image, Result<PathBuf, String>)> = OnceLock::new();
static WORKERS: Mutex<Vec<Arc<Service<Reply>>>> = Mutex::new(Vec::new());

pub(crate) fn configure(image: Image, executable: Result<PathBuf, String>) {
    let _ = IMAGE.set((image, executable));
}

pub(crate) fn take_workers() -> Vec<Arc<Service<Reply>>> {
    std::mem::take(&mut *WORKERS.lock().expect("account workers"))
}

fn new_worker(image: Image, executable: Result<PathBuf, String>) -> Arc<Service<Reply>> {
    let worker = Arc::new(Service::new(image, executable));
    let mut workers = WORKERS.lock().expect("account workers");
    workers.retain(|worker| !worker.closed_and_reaped());
    workers.push(Arc::clone(&worker));
    worker
}

pub struct DesktopAccounts {
    data: Mutex<DataDir>,
    worker: Option<Arc<Service<Reply>>>,
}

impl DesktopAccounts {
    pub fn new(data: DataDir) -> Arc<Self> {
        Arc::new(Self {
            data: Mutex::new(data),
            worker: IMAGE
                .get()
                .map(|(image, executable)| new_worker(*image, executable.clone())),
        })
    }

    pub fn data_dir(&self) -> DataDir {
        self.data.lock().expect("account paths").clone()
    }

    pub fn update_paths(&self, data: &DataDir) {
        let mut previous = self.data.lock().expect("account paths");
        assert_eq!(previous.path(DataPath::Root), data.path(DataPath::Root));
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
                data.path(DataPath::Root).to_path_buf(),
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
        key: AccountKey,
        proxy: Option<ServiceHost>,
    ) -> Result<Identity, String> {
        match self
            .exchange(Action::Resolve { key, proxy }, None, |_| {})
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
        let command = Command::new(data.path(DataPath::Root).to_path_buf(), action);
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
                Some(Event::Disconnected) | None => return disconnected(),
            }
        }
    }
}

impl Drop for DesktopAccounts {
    fn drop(&mut self) {
        self.close();
    }
}

fn disconnected<T>() -> Result<T, String> {
    Err("The account worker exited or sent an invalid reply. Try again.".into())
}
