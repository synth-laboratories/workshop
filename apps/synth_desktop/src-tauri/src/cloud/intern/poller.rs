use super::{
    normalize_event, InternClient, InternClientError, InternEvent, NormalizedInternEvent,
    RuntimeProjection,
};
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::Arc,
    time::Duration,
};
use tokio::{
    sync::{mpsc, watch, Mutex},
    task::JoinHandle,
};

const DEDUPE_WINDOW: usize = 4_096;

#[derive(Clone, Debug)]
pub struct PollerConfig {
    pub page_size: u16,
    pub idle_interval: Duration,
    pub projection_interval: Duration,
    pub initial_backoff: Duration,
    pub maximum_backoff: Duration,
    pub maximum_pages_per_tick: usize,
}

impl Default for PollerConfig {
    fn default() -> Self {
        Self {
            page_size: 500,
            idle_interval: Duration::from_millis(900),
            projection_interval: Duration::from_secs(4),
            initial_backoff: Duration::from_secs(1),
            maximum_backoff: Duration::from_secs(15),
            maximum_pages_per_tick: 20,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PollUpdate {
    Events {
        events: Vec<NormalizedInternEvent>,
        next_sequence: u64,
    },
    Projection {
        projection: RuntimeProjection,
    },
    Retry {
        attempt: u32,
        delay_ms: u64,
        message: String,
    },
    Stopped {
        reason: String,
    },
}

pub struct PollerHandle {
    cancel: watch::Sender<bool>,
    task: JoinHandle<()>,
}

impl PollerHandle {
    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }

    pub async fn stop(self) {
        let _ = self.cancel.send(true);
        let _ = self.task.await;
    }
}

/// Owns at most one live poller for each local session key. The key is local,
/// not a bearer token or remote URL, so diagnostics never expose credentials.
#[derive(Default)]
pub struct InternPoller {
    handles: Mutex<HashMap<String, PollerHandle>>,
}

impl InternPoller {
    pub async fn ensure_sync(
        &self,
        key: String,
        client: Arc<InternClient>,
        runtime_id: String,
        after_sequence: u64,
        updates: mpsc::Sender<PollUpdate>,
        config: PollerConfig,
    ) -> bool {
        self.ensure(
            key,
            client,
            PollTarget::Sync(runtime_id),
            after_sequence,
            updates,
            config,
        )
        .await
    }

    pub async fn ensure_async(
        &self,
        key: String,
        client: Arc<InternClient>,
        runtime_id: String,
        after_sequence: u64,
        updates: mpsc::Sender<PollUpdate>,
        config: PollerConfig,
    ) -> bool {
        self.ensure(
            key,
            client,
            PollTarget::Async(runtime_id),
            after_sequence,
            updates,
            config,
        )
        .await
    }

    async fn ensure(
        &self,
        key: String,
        client: Arc<InternClient>,
        target: PollTarget,
        after_sequence: u64,
        updates: mpsc::Sender<PollUpdate>,
        config: PollerConfig,
    ) -> bool {
        let mut handles = self.handles.lock().await;
        if handles
            .get(&key)
            .is_some_and(|handle| !handle.is_finished())
        {
            return false;
        }
        if let Some(finished) = handles.remove(&key) {
            let _ = finished.task.await;
        }
        handles.insert(
            key,
            spawn_poller(client, target, after_sequence, updates, config),
        );
        true
    }

    pub async fn stop(&self, key: &str) -> bool {
        let handle = self.handles.lock().await.remove(key);
        if let Some(handle) = handle {
            handle.stop().await;
            true
        } else {
            false
        }
    }

    pub async fn shutdown(&self) {
        let handles = std::mem::take(&mut *self.handles.lock().await);
        for (_, handle) in handles {
            handle.stop().await;
        }
    }
}

#[derive(Clone)]
enum PollTarget {
    Sync(String),
    Async(String),
}

fn spawn_poller(
    client: Arc<InternClient>,
    target: PollTarget,
    after_sequence: u64,
    updates: mpsc::Sender<PollUpdate>,
    config: PollerConfig,
) -> PollerHandle {
    let (cancel, cancel_rx) = watch::channel(false);
    let task = tokio::spawn(poll_loop(
        client,
        target,
        after_sequence,
        updates,
        config,
        cancel_rx,
    ));
    PollerHandle { cancel, task }
}

async fn poll_loop(
    client: Arc<InternClient>,
    target: PollTarget,
    after_sequence: u64,
    updates: mpsc::Sender<PollUpdate>,
    config: PollerConfig,
    mut cancel: watch::Receiver<bool>,
) {
    let mut cursor = CursorState::new(after_sequence, target.runtime_id().to_owned());
    let mut backoff = Backoff::new(config.initial_backoff, config.maximum_backoff);
    let mut next_projection = tokio::time::Instant::now();
    loop {
        if *cancel.borrow() {
            break;
        }
        let result = poll_tick(
            &client,
            &target,
            &mut cursor,
            &updates,
            &config,
            &mut next_projection,
        )
        .await;
        let sleep = match result {
            Ok(had_events) => {
                backoff.reset();
                if had_events {
                    Duration::ZERO
                } else {
                    config.idle_interval
                }
            }
            Err(error) if error.is_auth_failure() => {
                let _ = updates
                    .send(PollUpdate::Stopped {
                        reason: "authentication_failed".into(),
                    })
                    .await;
                break;
            }
            Err(error) if !error.is_retryable() => {
                let _ = updates
                    .send(PollUpdate::Stopped {
                        reason: error.to_string(),
                    })
                    .await;
                break;
            }
            Err(error) => {
                let (attempt, delay) = backoff.next();
                let _ = updates
                    .send(PollUpdate::Retry {
                        attempt,
                        delay_ms: millis(delay),
                        message: error.to_string(),
                    })
                    .await;
                delay
            }
        };
        tokio::select! {
            _ = tokio::time::sleep(sleep) => {}
            changed = cancel.changed() => { if changed.is_err() || *cancel.borrow() { break; } }
        }
    }
}

async fn poll_tick(
    client: &InternClient,
    target: &PollTarget,
    cursor: &mut CursorState,
    updates: &mpsc::Sender<PollUpdate>,
    config: &PollerConfig,
    next_projection: &mut tokio::time::Instant,
) -> Result<bool, InternClientError> {
    let mut had_events = false;
    for _ in 0..config.maximum_pages_per_tick.max(1) {
        let events = match target {
            PollTarget::Sync(runtime_id) => {
                client
                    .sync_events(runtime_id, cursor.sequence, config.page_size)
                    .await?
            }
            PollTarget::Async(_) => {
                client
                    .async_events(cursor.sequence, config.page_size)
                    .await?
            }
        };
        let page_len = events.len();
        let normalized = cursor
            .ingest(events)
            .map_err(|error| InternClientError::Protocol(error.to_string()))?;
        if !normalized.is_empty() {
            had_events = true;
            if updates
                .send(PollUpdate::Events {
                    events: normalized,
                    next_sequence: cursor.sequence,
                })
                .await
                .is_err()
            {
                return Err(InternClientError::Protocol(
                    "poll update receiver closed".into(),
                ));
            }
        }
        if page_len < usize::from(config.page_size.max(1)) {
            break;
        }
    }
    if tokio::time::Instant::now() >= *next_projection {
        let projection = match target {
            PollTarget::Sync(runtime_id) => client.get_sync(runtime_id).await?,
            PollTarget::Async(_) => client.get_async().await?,
        };
        if updates
            .send(PollUpdate::Projection { projection })
            .await
            .is_err()
        {
            return Err(InternClientError::Protocol(
                "poll update receiver closed".into(),
            ));
        }
        *next_projection = tokio::time::Instant::now() + config.projection_interval;
    }
    Ok(had_events)
}

impl PollTarget {
    fn runtime_id(&self) -> &str {
        match self {
            Self::Sync(id) | Self::Async(id) => id,
        }
    }
}

struct CursorState {
    sequence: u64,
    runtime_id: String,
    ids: HashSet<String>,
    order: VecDeque<String>,
}

impl CursorState {
    fn new(sequence: u64, runtime_id: String) -> Self {
        Self {
            sequence,
            runtime_id,
            ids: HashSet::new(),
            order: VecDeque::new(),
        }
    }

    fn ingest(&mut self, events: Vec<InternEvent>) -> Result<Vec<NormalizedInternEvent>> {
        let mut accepted = Vec::new();
        for event in events {
            event.validate()?;
            if event.runtime_id != self.runtime_id {
                bail!("Intern event runtime identity drifted");
            }
            if event.sequence <= self.sequence {
                continue;
            }
            if event.sequence != self.sequence + 1 {
                bail!("Intern event sequence gap after {}", self.sequence);
            }
            if !self.ids.insert(event.event_id.clone()) {
                bail!("Intern event id was reused");
            }
            self.order.push_back(event.event_id.clone());
            if self.order.len() > DEDUPE_WINDOW {
                if let Some(old) = self.order.pop_front() {
                    self.ids.remove(&old);
                }
            }
            self.sequence = event.sequence;
            accepted.push(normalize_event(event));
        }
        Ok(accepted)
    }
}

struct Backoff {
    initial: Duration,
    maximum: Duration,
    current: Duration,
    attempt: u32,
}
impl Backoff {
    fn new(initial: Duration, maximum: Duration) -> Self {
        Self {
            initial,
            maximum,
            current: initial,
            attempt: 0,
        }
    }
    fn reset(&mut self) {
        self.current = self.initial;
        self.attempt = 0;
    }
    fn next(&mut self) -> (u32, Duration) {
        self.attempt = self.attempt.saturating_add(1);
        let base = self.current.min(self.maximum);
        self.current = self.current.saturating_mul(2).min(self.maximum);
        let jitter_max = (base.as_millis() / 5) as u64;
        let jitter = if jitter_max == 0 {
            0
        } else {
            rand::random_range(0..=jitter_max)
        };
        (self.attempt, base + Duration::from_millis(jitter))
    }
}

fn millis(value: Duration) -> u64 {
    value.as_millis().min(u128::from(u64::MAX)) as u64
}

