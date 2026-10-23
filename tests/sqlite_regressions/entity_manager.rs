//! Entity-manager races and removals.

use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use tideorm::entity_manager::EntityManager;

/// A hold a test sets on the next save of a model whose `before_save` calls
/// [`Pause::wait`], to act while a flush is part way through.
pub struct Pause(Mutex<Option<(tokio::sync::oneshot::Sender<()>, mpsc::Receiver<()>)>>);

impl Pause {
    pub const fn new() -> Self {
        Self(Mutex::new(None))
    }

    /// In `before_save`: if a hold is set, report reaching it and wait to be
    /// resumed.
    pub fn wait(&self) {
        let pause = self.0.lock().unwrap().take();
        if let Some((entered, resume)) = pause {
            entered.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(10)).unwrap();
        }
    }

    /// Start `manager.flush()` on a task and return once it reaches the hold,
    /// with the task and the sender that resumes it.
    pub async fn flush(
        &self,
        manager: &Arc<EntityManager>,
    ) -> (
        tokio::task::JoinHandle<tideorm::Result<()>>,
        mpsc::Sender<()>,
    ) {
        let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
        let (resume_tx, resume_rx) = mpsc::channel();
        *self.0.lock().unwrap() = Some((entered_tx, resume_rx));
        let manager = manager.clone();
        let task = tokio::spawn(async move { manager.flush().await });
        tokio::time::timeout(Duration::from_secs(10), entered_rx)
            .await
            .unwrap()
            .unwrap();
        (task, resume_tx)
    }
}

#[path = "entity_manager/flush.rs"]
mod flush;

#[path = "entity_manager/identity.rs"]
mod identity;

#[path = "entity_manager/removal.rs"]
mod removal;

#[path = "entity_manager/relation_sync.rs"]
mod relation_sync;
