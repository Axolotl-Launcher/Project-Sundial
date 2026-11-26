use std::collections::HashMap;
use std::io;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

use serde::{Serialize, de::DeserializeOwned};
use tokio::sync::{mpsc, watch};
use tokio::task::{JoinSet, LocalSet};

use crate::protocol::{self, Input, Output};

type Active = Arc<Mutex<HashMap<u64, Cancellation>>>;
type Job<Command> = (u64, Command, Cancellation);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Pending,
    Cancelled,
    Committing,
}

/// A cancellation signal that cannot interrupt a started commit.
#[derive(Clone)]
pub struct Cancellation(watch::Sender<Phase>);

impl Default for Cancellation {
    fn default() -> Self {
        Self(watch::Sender::new(Phase::Pending))
    }
}

impl Cancellation {
    pub fn cancel(&self) {
        self.transition(Phase::Cancelled);
    }

    pub fn is_pending(&self) -> bool {
        *self.0.borrow() == Phase::Pending
    }

    pub fn is_saving(&self) -> bool {
        *self.0.borrow() == Phase::Committing
    }

    pub fn begin_commit(&self) -> bool {
        self.transition(Phase::Committing)
    }

    pub fn confirm_commit(&self) {
        self.0.send_replace(Phase::Committing);
    }

    pub async fn cancelled(&self) {
        let _ = self
            .0
            .subscribe()
            .wait_for(|state| *state == Phase::Cancelled)
            .await;
    }

    pub async fn settled(&self) {
        let _ = self
            .0
            .subscribe()
            .wait_for(|state| *state != Phase::Pending)
            .await;
    }

    fn transition(&self, next: Phase) -> bool {
        self.0.send_if_modified(|phase| transition(phase, next))
    }
}

fn transition(phase: &mut Phase, next: Phase) -> bool {
    if *phase != Phase::Pending {
        return false;
    }
    *phase = next;
    true
}

/// Writes events for one request and publishes the service's idle-retention state.
pub struct Responder<Reply> {
    id: u64,
    _type: PhantomData<fn(Reply)>,
}

impl<Reply> Clone for Responder<Reply> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Reply> Copy for Responder<Reply> {}

impl<Reply: Serialize> Responder<Reply> {
    pub fn send(self, reply: Reply) -> Result<(), String> {
        send(Output::Event { id: self.id, reply })
    }

    pub fn retain(self, retained: bool) {
        self.retention().set(retained);
    }

    pub fn retention(self) -> Retention<Reply> {
        Retention(PhantomData)
    }

    pub fn status(self, status: Reply) {
        let _ = send(Output::Status(status));
    }
}

pub struct Retention<Reply>(PhantomData<fn(Reply)>);

impl<Reply: Serialize> Retention<Reply> {
    pub fn set(&self, retained: bool) {
        let _ = send(Output::<Reply>::Retain(retained));
    }
}

fn send<Reply: Serialize>(reply: Output<Reply>) -> Result<(), String> {
    protocol::write(&mut io::stdout().lock(), &reply)
        .map_err(|_| "The worker could not send a reply.".to_owned())
}

/// Reports a bootstrap failure before accepting any account or download requests.
pub(super) fn startup_failed(error: impl std::fmt::Display) -> i32 {
    let _ = send(Output::<()>::StartupFailed(error.to_string()));
    1
}

/// Runs a versioned pipe server. Its reader, runtime and tasks end before return.
pub fn serve<Command, Reply, Handler, Work>(handle: Handler) -> i32
where
    Command: DeserializeOwned + Send + 'static,
    Reply: Serialize + 'static,
    Handler: Fn(Command, Responder<Reply>, Cancellation) -> Work + 'static,
    Work: Future<Output = Result<(), String>> + 'static,
{
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => return startup_failed(format!("could not initialize the worker: {error}")),
    };
    if send(Output::<Reply>::Ready).is_err() {
        return 1;
    }
    let active = Active::default();
    let (sender, incoming) = mpsc::unbounded_channel();
    let reader_active = Arc::clone(&active);
    let reader = std::thread::spawn(move || read_requests(reader_active, sender));
    LocalSet::new().block_on(&runtime, dispatch(incoming, active, handle));
    reader.join().unwrap_or(1)
}

fn read_requests<Command: DeserializeOwned>(
    active: Active,
    sender: mpsc::UnboundedSender<Job<Command>>,
) -> i32 {
    let mut input = io::stdin().lock();
    let status = loop {
        match protocol::read(&mut input) {
            Ok(Some(Input::Request { id, command })) => {
                match enqueue(&active, &sender, id, command) {
                    true => (),
                    false => break 1,
                }
            }
            Ok(Some(Input::Cancel { id })) => cancel_request(&active, id),
            Ok(Some(Input::Shutdown)) => break 0,
            Ok(None) => break 2,
            Err(_) => break 1,
        }
    };
    for cancellation in active.lock().expect("worker requests").values() {
        cancellation.cancel();
    }
    status
}

fn enqueue<Command>(
    active: &Active,
    sender: &mpsc::UnboundedSender<Job<Command>>,
    id: u64,
    command: Command,
) -> bool {
    use std::collections::hash_map::Entry;

    let cancellation = Cancellation::default();
    let mut requests = active.lock().expect("worker requests");
    let Entry::Vacant(entry) = requests.entry(id) else {
        return false;
    };
    entry.insert(cancellation.clone());
    sender.send((id, command, cancellation)).is_ok()
}

fn cancel_request(active: &Active, id: u64) {
    if let Some(cancellation) = active.lock().expect("worker requests").get(&id) {
        cancellation.cancel();
    }
}

async fn dispatch<Command, Reply, Handler, Work>(
    mut incoming: mpsc::UnboundedReceiver<Job<Command>>,
    active: Active,
    handle: Handler,
) where
    Reply: Serialize + 'static,
    Handler: Fn(Command, Responder<Reply>, Cancellation) -> Work,
    Work: Future<Output = Result<(), String>> + 'static,
{
    let mut tasks = JoinSet::new();
    loop {
        tokio::select! {
            job = incoming.recv() => {
                let Some((id, command, cancellation)) = job else { break };
                let reply = Responder { id, _type: PhantomData };
                let work = handle(command, reply, cancellation);
                let active = Arc::clone(&active);
                tasks.spawn_local(async move {
                    let result = work.await;
                    active.lock().expect("worker requests").remove(&id);
                    let _ = send(Output::<Reply>::Finished { id, result });
                });
            }
            outcome = tasks.join_next(), if !tasks.is_empty() => {
                if matches!(outcome, Some(Err(_))) {
                    std::process::abort();
                }
            }
        }
    }
    while let Some(result) = tasks.join_next().await {
        if result.is_err() {
            std::process::abort();
        }
    }
}
