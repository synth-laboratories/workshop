use async_trait::async_trait;
use uuid::Uuid;

use crate::types::ThreadId;

/// Soft fan-out: never source of truth. Failure must not fail publish.
#[async_trait]
pub trait Wake: Send + Sync {
    async fn notify_thread(&self, thread_id: ThreadId);
    async fn notify_worker(&self);
}

#[derive(Default)]
pub struct NoopWake;

#[async_trait]
impl Wake for NoopWake {
    async fn notify_thread(&self, _thread_id: ThreadId) {}
    async fn notify_worker(&self) {}
}

/// In-process broadcast for SSE / same-process workers.
#[derive(Clone)]
pub struct LocalWake {
    tx: tokio::sync::broadcast::Sender<WakeEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeEvent {
    Thread(Uuid),
    Worker,
}

impl LocalWake {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(capacity);
        Self { tx }
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<WakeEvent> {
        self.tx.subscribe()
    }
}

#[async_trait]
impl Wake for LocalWake {
    async fn notify_thread(&self, thread_id: ThreadId) {
        let _ = self.tx.send(WakeEvent::Thread(thread_id.0));
    }

    async fn notify_worker(&self) {
        let _ = self.tx.send(WakeEvent::Worker);
    }
}
