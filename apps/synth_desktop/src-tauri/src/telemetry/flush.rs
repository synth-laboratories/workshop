//! Background sync of consented, sync-eligible telemetry.
//!
//! The flusher reads the outbox past the watermark, ships one batch through
//! the configured sink, and advances the watermark only on acknowledgment.
//! Consent is re-checked on every pass — the gate governs recording, this
//! boundary governs egress, and either alone is sufficient to stop a leak.

use anyhow::Result;
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use super::consent;
use super::contract;
use super::store::{OutboxEvent, TelemetryStore};

pub const BATCH_LIMIT: usize = 200;
const FLUSH_INTERVAL: Duration = Duration::from_secs(30 * 60);
const FIRST_FLUSH_DELAY: Duration = Duration::from_secs(90);

/// Where a batch goes. Implementations: the profile-routed backend, and the
/// in-memory sink tests use. `dyn`-compatible by boxing the future.
pub trait TelemetrySink: Send + Sync {
    fn send<'a>(
        &'a self,
        batch: &'a [Value],
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlushOutcome {
    /// Consent absent, stale, or withdrawn: nothing may leave the device.
    ConsentWithheld,
    /// Nothing past the watermark.
    Empty,
    Sent { events: usize },
}

pub struct Flusher {
    store: TelemetryStore,
    sink: Arc<dyn TelemetrySink>,
}

impl Flusher {
    pub fn new(store: TelemetryStore, sink: Arc<dyn TelemetrySink>) -> Self {
        Self { store, sink }
    }

    pub async fn flush_once(&self) -> Result<FlushOutcome> {
        if !consent::sync_allowed(&consent::state(&self.store)?) {
            return Ok(FlushOutcome::ConsentWithheld);
        }
        let batch = self.store.batch_for_sync(BATCH_LIMIT)?;
        let Some(high_water) = batch.last().map(|event| event.rowid) else {
            return Ok(FlushOutcome::Empty);
        };
        let install_id = self.store.install_id()?;
        let wire: Vec<Value> = batch
            .iter()
            .map(|event| wire_event(event, &install_id))
            .collect();
        self.sink.send(&wire).await?;
        self.store.advance_watermark(high_water)?;
        Ok(FlushOutcome::Sent {
            events: batch.len(),
        })
    }

    /// Long-running loop: first pass shortly after launch, then a fixed
    /// interval. Failures are logged and retried at the next tick — the
    /// outbox is durable and idempotent, so nothing is lost by waiting.
    pub async fn run(self: Arc<Self>) {
        tokio::time::sleep(FIRST_FLUSH_DELAY).await;
        loop {
            if let Err(error) = self.flush_once().await {
                eprintln!("synth-desktop: telemetry flush failed (will retry): {error:#}");
            }
            tokio::time::sleep(FLUSH_INTERVAL).await;
        }
    }
}

/// The wire shape of `POST /api/v1/product/usage-events`. `client_event_id`
/// is the stored `pte_` id, so a retried batch dedups server-side.
fn wire_event(event: &OutboxEvent, install_id: &str) -> Value {
    let spec = contract::spec(&event.name);
    json!({
        "event_id": event.event_id,
        "name": event.name,
        "class": spec.map(|s| contract::class_name(s.class)).unwrap_or("product"),
        "owner": spec.map(|s| s.owner.as_str()).unwrap_or("workshop-unknown"),
        "observed_at": event.at,
        "install_id": install_id,
        "payload": event.properties,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;
    use std::sync::Mutex;

    struct CapturingSink {
        batches: Mutex<Vec<Vec<Value>>>,
        fail: std::sync::atomic::AtomicBool,
    }

    impl TelemetrySink for CapturingSink {
        fn send<'a>(
            &'a self,
            batch: &'a [Value],
        ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
            Box::pin(async move {
                if self.fail.load(std::sync::atomic::Ordering::Relaxed) {
                    anyhow::bail!("sink offline");
                }
                self.batches.lock().unwrap().push(batch.to_vec());
                Ok(())
            })
        }
    }

    fn fixture() -> (tempfile::TempDir, TelemetryStore, Arc<CapturingSink>, Flusher) {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let store = TelemetryStore::new(storage.database().clone());
        let sink = Arc::new(CapturingSink {
            batches: Mutex::new(Vec::new()),
            fail: std::sync::atomic::AtomicBool::new(false),
        });
        let flusher = Flusher::new(store.clone(), sink.clone());
        (dir, store, sink, flusher)
    }

    fn record(store: &TelemetryStore, name: &str) {
        let spec = super::contract::spec(name).unwrap();
        store
            .insert(name, spec.sensitivity, &json!({"install_id": "ins_t"}))
            .unwrap();
    }

    #[tokio::test]
    async fn nothing_leaves_without_current_consent() {
        let (_dir, store, sink, flusher) = fixture();
        record(&store, "signin_completed");
        assert_eq!(
            flusher.flush_once().await.unwrap(),
            FlushOutcome::ConsentWithheld
        );
        assert!(sink.batches.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn consented_flush_ships_and_advances_watermark_once() {
        let (_dir, store, sink, flusher) = fixture();
        consent::record_choice(&store, consent::ConsentChoice::Granted).unwrap();
        record(&store, "signin_completed");
        record(&store, "workflow_started");
        assert_eq!(
            flusher.flush_once().await.unwrap(),
            FlushOutcome::Sent { events: 2 }
        );
        // Idempotent: a second pass finds nothing new.
        assert_eq!(flusher.flush_once().await.unwrap(), FlushOutcome::Empty);
        let batches = sink.batches.lock().unwrap();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0][0]["name"], "signin_completed");
        assert_eq!(batches[0][0]["class"], "funnel");
        assert!(batches[0][0]["event_id"]
            .as_str()
            .unwrap()
            .starts_with("pte_"));
    }

    #[tokio::test]
    async fn essential_events_never_ship() {
        let (_dir, store, sink, flusher) = fixture();
        consent::record_choice(&store, consent::ConsentChoice::Granted).unwrap();
        record(&store, "recovery_attempted");
        assert_eq!(flusher.flush_once().await.unwrap(), FlushOutcome::Empty);
        assert!(sink.batches.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn failed_send_keeps_the_batch_for_retry() {
        let (_dir, store, sink, flusher) = fixture();
        consent::record_choice(&store, consent::ConsentChoice::Granted).unwrap();
        record(&store, "signin_completed");
        sink.fail.store(true, std::sync::atomic::Ordering::Relaxed);
        assert!(flusher.flush_once().await.is_err());
        sink.fail.store(false, std::sync::atomic::Ordering::Relaxed);
        assert_eq!(
            flusher.flush_once().await.unwrap(),
            FlushOutcome::Sent { events: 1 }
        );
    }
}
