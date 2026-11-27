use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use serde::de::DeserializeOwned;
use serde_json::Value;
use tokio::sync::oneshot;

#[derive(Default)]
struct Pending {
    next: u64,
    replies: HashMap<u64, oneshot::Sender<Value>>,
}

/// Correlates host acknowledgements with callbacks from a single worker request.
#[derive(Clone, Default)]
pub struct Callbacks(Arc<Mutex<Pending>>);

impl Callbacks {
    pub async fn request<Reply: DeserializeOwned>(
        &self,
        send: impl FnOnce(u64) -> Result<(), String>,
    ) -> Result<Reply, String> {
        let (id, response) = self.reserve();
        let _ticket = Ticket(self, id);
        send(id)?;
        let value = response.await.map_err(|_| "The host dropped a callback.")?;
        serde_json::from_value(value).map_err(|_| "Invalid host callback reply.".into())
    }

    fn reserve(&self) -> (u64, oneshot::Receiver<Value>) {
        let mut pending = self.0.lock().expect("worker callbacks");
        let id = pending.next;
        pending.next = id.checked_add(1).expect("worker callback ID");
        let (sender, receiver) = oneshot::channel();
        pending.replies.insert(id, sender);
        (id, receiver)
    }

    pub(super) fn deliver(&self, id: u64, value: Value) {
        if let Some(sender) = self.remove(id) {
            let _ = sender.send(value);
        }
    }

    fn remove(&self, id: u64) -> Option<oneshot::Sender<Value>> {
        self.0.lock().expect("worker callbacks").replies.remove(&id)
    }
}

struct Ticket<'a>(&'a Callbacks, u64);

impl Drop for Ticket<'_> {
    fn drop(&mut self) {
        self.0.remove(self.1);
    }
}
