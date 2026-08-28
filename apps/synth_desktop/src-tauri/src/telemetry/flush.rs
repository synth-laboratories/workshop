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
    Sent {
        events: usize,
    },
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

