use tokio::sync::watch;

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
