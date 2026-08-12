use super::{
    InternRuntime, NormalizedInternEvent, PollUpdate, PollerConfig, RuntimeKind, RuntimeProjection,
};
use crate::storage::{append_event, AppEvent, Database, EventAppend, EventSource};
use anyhow::{bail, Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, sync::Arc};
use tokio::{
    sync::{mpsc, Mutex},
    task::JoinHandle,
};

const SOURCE: &str = "intern";

/// Stable boundary between the Intern mailbox transport and shared desktop
/// session/run services. The transport may be replaced without changing the
/// durable journal contract consumed by the rest of the application.
#[derive(Clone)]
pub struct InternIngestion {
    db: Arc<Database>,
}

/// Owns transport pollers and their durable consumers as a unit. The manager
/// deliberately emits committed `AppEvent`s rather than depending on Tauri or
/// a renderer; CoreRuntime/shared SessionService can bridge that stream to the
/// single application broadcaster.
pub struct InternProviderManager {
    runtime: Arc<InternRuntime>,
    ingestion: InternIngestion,
    consumers: Mutex<HashMap<String, JoinHandle<Result<()>>>>,
}

impl InternProviderManager {
    pub fn new(runtime: Arc<InternRuntime>, db: Arc<Database>) -> Self {
        Self {
            runtime,
            ingestion: InternIngestion::new(db),
            consumers: Mutex::new(HashMap::new()),
        }
    }

    pub fn ingestion(&self) -> &InternIngestion {
        &self.ingestion
    }

    pub async fn start_sync(
        &self,
        binding: InternSessionBinding,
        committed: mpsc::Sender<AppEvent>,
        config: PollerConfig,
    ) -> Result<bool> {
        self.start(binding, committed, config, true).await
    }

    pub async fn start_async(
        &self,
        binding: InternSessionBinding,
        committed: mpsc::Sender<AppEvent>,
        config: PollerConfig,
    ) -> Result<bool> {
        self.start(binding, committed, config, false).await
    }

    async fn start(
        &self,
        binding: InternSessionBinding,
        committed: mpsc::Sender<AppEvent>,
        config: PollerConfig,
        sync: bool,
    ) -> Result<bool> {
        if sync != (binding.runtime_kind == RuntimeKind::Sync) {
            bail!("Intern binding kind does not match the requested poller");
        }
        let state = self.ingestion.attach(binding.clone()).await?;
        let client = self.runtime.client().await?;
        let (updates_tx, updates_rx) = mpsc::channel(64);
        let started = if sync {
            self.runtime
                .poller()
                .ensure_sync(
                    binding.session_id.clone(),
                    client,
                    binding.runtime_id,
                    state.remote_cursor,
                    updates_tx,
                    config,
                )
                .await
        } else {
            self.runtime
                .poller()
                .ensure_async(
                    binding.session_id.clone(),
                    client,
                    binding.runtime_id,
                    state.remote_cursor,
                    updates_tx,
                    config,
                )
                .await
        };
        if !started {
            return Ok(false);
        }
        let ingestion = self.ingestion.clone();
        let session_id = binding.session_id.clone();
        let consumer =
            tokio::spawn(async move { ingestion.consume(session_id, updates_rx, committed).await });
        let mut consumers = self.consumers.lock().await;
        if let Some(previous) = consumers.insert(binding.session_id, consumer) {
            previous.abort();
        }
        Ok(true)
    }

    pub async fn stop(&self, session_id: &str) -> Result<bool> {
        let stopped = self.runtime.poller().stop(session_id).await;
        if let Some(consumer) = self.consumers.lock().await.remove(session_id) {
            consumer.await.context("join Intern ingestion consumer")??;
        }
        Ok(stopped)
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.runtime.poller().shutdown().await;
        let consumers = std::mem::take(&mut *self.consumers.lock().await);
        for (_, consumer) in consumers {
            consumer.await.context("join Intern ingestion consumer")??;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InternSessionBinding {
    pub session_id: String,
    pub runtime_id: String,
    pub runtime_kind: RuntimeKind,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InternIngestionState {
    pub session_id: String,
    pub runtime_id: Option<String>,
    pub status: String,
    pub state_generation: Option<i64>,
    pub remote_cursor: u64,
    pub last_diagnostic: Option<String>,
}

impl InternIngestion {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Register the local/remote identity before starting a poller. Reattaching
    /// the same runtime is idempotent; changing a bound runtime fails closed.
    pub async fn attach(&self, binding: InternSessionBinding) -> Result<InternIngestionState> {
        validate_binding(&binding)?;
        let db = self.db.clone();
        db.run_transaction(move |conn| attach_session(conn, &binding))
            .await
    }

    pub async fn state(&self, session_id: impl Into<String>) -> Result<InternIngestionState> {
        let session_id = session_id.into();
        self.db
            .clone()
            .run(move |conn| load_state(conn, &session_id))
            .await
    }

    pub async fn resume_cursor(&self, session_id: impl Into<String>) -> Result<u64> {
        Ok(self.state(session_id).await?.remote_cursor)
    }

    /// Apply one poller update. Every returned event is already committed. An
    /// Events page and its source cursor are persisted in the same transaction.
    pub async fn apply(
        &self,
        session_id: impl Into<String>,
        update: PollUpdate,
    ) -> Result<Vec<AppEvent>> {
        let session_id = session_id.into();
        let db = self.db.clone();
        db.run_transaction(move |conn| apply_update(conn, &session_id, update))
            .await
    }

    /// Drain poller updates and forward committed journal events. A closed
    /// output receiver does not stop durable ingestion; a storage error does.
    pub async fn consume(
        &self,
        session_id: impl Into<String>,
        mut updates: mpsc::Receiver<PollUpdate>,
        committed: mpsc::Sender<AppEvent>,
    ) -> Result<()> {
        let session_id = session_id.into();
        while let Some(update) = updates.recv().await {
            for event in self.apply(session_id.clone(), update).await? {
                let _ = committed.send(event).await;
            }
        }
        Ok(())
    }
}

fn validate_binding(binding: &InternSessionBinding) -> Result<()> {
    if binding.session_id.trim().is_empty() {
        bail!("Intern local session id is required");
    }
    if binding.runtime_id.trim().is_empty() {
        bail!("Intern remote runtime id is required");
    }
    Ok(())
}

fn attach_session(
    conn: &Connection,
    binding: &InternSessionBinding,
) -> Result<InternIngestionState> {
    if binding.runtime_kind == RuntimeKind::Async {
        let canonical_session: Option<String> = conn
            .query_row(
                "SELECT id FROM sessions
                 WHERE remote_id = ?1
                   AND json_extract(target_json, '$.kind') = 'intern'
                   AND (json_extract(target_json, '$.mode') = 'async'
                        OR json_extract(target_json, '$.intern.runtimeKind') = 'async')
                 ORDER BY updated_at DESC, id ASC LIMIT 1",
                params![binding.runtime_id],
                |row| row.get(0),
            )
            .optional()?;
        if canonical_session
            .as_deref()
            .is_some_and(|session_id| session_id != binding.session_id)
        {
            bail!(
                "Async Intern runtime {} is already bound to local session {}",
                binding.runtime_id,
                canonical_session.expect("canonical session was checked as present")
            );
        }
    }
    let existing_remote: Option<String> = conn
        .query_row(
            "SELECT remote_id FROM sessions WHERE id = ?1",
            params![binding.session_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    if existing_remote
        .as_deref()
        .is_some_and(|remote| remote != binding.runtime_id)
    {
        bail!("Intern session is already bound to a different remote runtime");
    }

    let now = Utc::now().to_rfc3339();
    let existing: Option<(String, String)> = conn
        .query_row(
            "SELECT target_json, metadata_json FROM sessions WHERE id = ?1",
            params![binding.session_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let mut target = existing
        .as_ref()
        .and_then(|(target, _)| serde_json::from_str(target).ok())
        .unwrap_or_else(|| json!({"kind": "intern"}));
    let target_intern = object_entry(&mut target, "intern");
    target_intern.insert(
        "runtimeKind".into(),
        serde_json::to_value(binding.runtime_kind)?,
    );
    target_intern.insert(
        "runtimeId".into(),
        Value::String(binding.runtime_id.clone()),
    );
    let mut metadata = existing
        .as_ref()
        .and_then(|(_, metadata)| serde_json::from_str(metadata).ok())
        .unwrap_or_else(|| json!({}));
    let intern = object_entry(&mut metadata, "intern");
    intern.insert(
        "runtimeKind".into(),
        serde_json::to_value(binding.runtime_kind)?,
    );
    intern.insert(
        "runtimeId".into(),
        Value::String(binding.runtime_id.clone()),
    );
    conn.execute(
        "INSERT INTO sessions(
            id, title, kind, target_json, remote_id, status, latest_cursor,
            metadata_json, created_at, updated_at
         ) VALUES (?1, ?2, 'intern', ?3, ?4, 'ready', 0, ?5, ?6, ?6)
         ON CONFLICT(id) DO UPDATE SET
            remote_id = excluded.remote_id,
            kind = excluded.kind,
            target_json = excluded.target_json,
            metadata_json = excluded.metadata_json,
            updated_at = excluded.updated_at",
        params![
            binding.session_id,
            binding
                .title
                .clone()
                .unwrap_or_else(|| format!("Intern {}", binding.runtime_id)),
            target.to_string(),
            binding.runtime_id,
            metadata.to_string(),
            now,
        ],
    )?;
    conn.execute(
        "INSERT INTO source_cursors(session_id, source, cursor) VALUES (?1, ?2, 0)
         ON CONFLICT(session_id, source) DO NOTHING",
        params![binding.session_id, SOURCE],
    )?;
    load_state(conn, &binding.session_id)
}

fn apply_update(conn: &Connection, session_id: &str, update: PollUpdate) -> Result<Vec<AppEvent>> {
    let state = load_state(conn, session_id)?;
    match update {
        PollUpdate::Events {
            events,
            next_sequence,
        } => apply_events(conn, &state, events, next_sequence),
        PollUpdate::Projection { projection } => apply_projection(conn, &state, projection),
        PollUpdate::Retry {
            attempt,
            delay_ms,
            message,
        } => apply_diagnostic(
            conn,
            &state,
            "intern.poll_retry",
            None,
            json!({"attempt": attempt, "delayMs": delay_ms, "message": message}),
            Some(message),
            false,
        ),
        PollUpdate::Stopped { reason } => {
            let auth = reason == "authentication_failed";
            apply_diagnostic(
                conn,
                &state,
                if auth {
                    "intern.authentication_failed"
                } else {
                    "intern.poll_stopped"
                },
                Some("interrupted"),
                json!({"reason": reason, "recoverable": !auth}),
                Some(reason),
                true,
            )
        }
    }
}

fn apply_events(
    conn: &Connection,
    state: &InternIngestionState,
    events: Vec<NormalizedInternEvent>,
    next_sequence: u64,
) -> Result<Vec<AppEvent>> {
    let mut cursor = state.remote_cursor;
    let mut committed = Vec::new();
    for event in events {
        if event.runtime_id != state.runtime_id.as_deref().unwrap_or_default() {
            bail!("Intern event runtime identity drifted");
        }
        if event.remote_sequence <= cursor {
            verify_replay(conn, &state.session_id, &event)?;
            continue;
        }
        if event.remote_sequence != cursor.saturating_add(1) {
            bail!("Intern event sequence gap after {cursor}");
        }
        let remote_sequence = i64::try_from(event.remote_sequence)
            .context("Intern remote sequence exceeds SQLite range")?;
        let state_generation = i64::try_from(event.state_generation)
            .context("Intern state generation exceeds SQLite range")?;
        let appended = append_event(
            conn,
            EventAppend {
                // Intern event IDs are remote-runtime identities, not globally
                // unique desktop journal IDs. Namespace them by local session
                // so a legacy duplicate binding cannot collide with the global
                // `events.event_id` uniqueness constraint.
                event_id: Some(journal_event_id(&state.session_id, &event.event_id)),
                session_id: Some(state.session_id.clone()),
                run_id: None,
                source: EventSource::Intern,
                kind: event.kind,
                payload: event.payload,
                remote_sequence: Some(remote_sequence),
                command_id: nonempty(event.command_id),
                created_at: Some(event.created_at),
            },
        )?;
        conn.execute(
            "UPDATE sessions SET state_generation = MAX(COALESCE(state_generation, 0), ?1),
                    status = CASE WHEN status IN ('created', 'ready', 'interrupted') THEN 'running' ELSE status END,
                    updated_at = ?2
             WHERE id = ?3",
            params![state_generation, Utc::now().to_rfc3339(), state.session_id],
        )?;
        cursor = event.remote_sequence;
        committed.push(appended);
    }
    if next_sequence != cursor {
        bail!("Intern poll cursor does not match the committed event page");
    }
    conn.execute(
        "UPDATE source_cursors SET cursor = ?1 WHERE session_id = ?2 AND source = ?3",
        params![
            i64::try_from(cursor).context("Intern cursor exceeds SQLite range")?,
            state.session_id,
            SOURCE
        ],
    )?;
    Ok(committed)
}

fn verify_replay(conn: &Connection, session_id: &str, event: &NormalizedInternEvent) -> Result<()> {
    let existing: Option<String> = conn
        .query_row(
            "SELECT event_id FROM events
             WHERE session_id = ?1 AND source = ?2 AND remote_sequence = ?3",
            params![
                session_id,
                SOURCE,
                i64::try_from(event.remote_sequence)
                    .context("Intern cursor exceeds SQLite range")?
            ],
            |row| row.get(0),
        )
        .optional()?;
    let expected = journal_event_id(session_id, &event.event_id);
    match existing {
        // Accept raw pre-namespacing IDs so an upgraded database can replay its
        // already committed cursor without rewriting historical rows.
        Some(event_id) if event_id == expected || event_id == event.event_id => Ok(()),
        Some(_) => bail!("Intern replay changed the event identity"),
        None => bail!("Intern replay is older than the cursor but is not persisted"),
    }
}

fn journal_event_id(session_id: &str, remote_event_id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(session_id.as_bytes());
    hasher.update([0]);
    hasher.update(remote_event_id.as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    format!("evt_intern_{}", &digest[..32])
}

fn apply_projection(
    conn: &Connection,
    state: &InternIngestionState,
    projection: RuntimeProjection,
) -> Result<Vec<AppEvent>> {
    projection.validate_async_identity()?;
    let remote_id = projection
        .runtime_id()
        .context("Intern projection omitted runtime identity")?;
    if Some(remote_id) != state.runtime_id.as_deref() {
        bail!("Intern projection runtime identity drifted");
    }
    let generation = i64::try_from(projection.state_generation)
        .context("Intern projection generation exceeds SQLite range")?;
    if state
        .state_generation
        .is_some_and(|current| generation < current)
    {
        bail!("Intern projection generation regressed");
    }
    let projection_value = serde_json::to_value(&projection)?;
    let canonical_status = canonical_session_status(&projection.status);
    let mut metadata = load_metadata(conn, &state.session_id)?;
    let intern = object_entry(&mut metadata, "intern");
    let unchanged = state.state_generation == Some(generation)
        && state.status == canonical_status
        && intern.get("projection") == Some(&projection_value);
    if unchanged {
        return Ok(Vec::new());
    }
    intern.insert("projection".into(), projection_value.clone());
    intern.remove("lastDiagnostic");
    conn.execute(
        "UPDATE sessions SET status = ?1, state_generation = ?2,
                metadata_json = ?3, updated_at = ?4 WHERE id = ?5",
        params![
            canonical_status,
            generation,
            metadata.to_string(),
            Utc::now().to_rfc3339(),
            state.session_id
        ],
    )?;
    let event = append_event(
        conn,
        EventAppend {
            event_id: None,
            session_id: Some(state.session_id.clone()),
            run_id: None,
            source: EventSource::Intern,
            kind: "intern.projection_updated".into(),
            payload: projection_value,
            remote_sequence: None,
            command_id: None,
            created_at: None,
        },
    )?;
    Ok(vec![event])
}

fn apply_diagnostic(
    conn: &Connection,
    state: &InternIngestionState,
    kind: &str,
    status: Option<&str>,
    payload: Value,
    message: Option<String>,
    terminal: bool,
) -> Result<Vec<AppEvent>> {
    let mut metadata = load_metadata(conn, &state.session_id)?;
    let intern = object_entry(&mut metadata, "intern");
    if let Some(message) = message {
        intern.insert("lastDiagnostic".into(), Value::String(message));
    }
    if terminal {
        conn.execute(
            "UPDATE sessions SET status = ?1, metadata_json = ?2, updated_at = ?3 WHERE id = ?4",
            params![
                status.context("terminal Intern diagnostic requires a canonical status")?,
                metadata.to_string(),
                Utc::now().to_rfc3339(),
                state.session_id
            ],
        )?;
    } else if let Some(status) = status {
        conn.execute(
            "UPDATE sessions SET
                status = CASE WHEN status IN ('connecting', 'running', 'retrying') THEN ?1 ELSE status END,
                metadata_json = ?2, updated_at = ?3 WHERE id = ?4",
            params![
                status,
                metadata.to_string(),
                Utc::now().to_rfc3339(),
                state.session_id
            ],
        )?;
    } else {
        conn.execute(
            "UPDATE sessions SET metadata_json = ?1, updated_at = ?2 WHERE id = ?3",
            params![
                metadata.to_string(),
                Utc::now().to_rfc3339(),
                state.session_id
            ],
        )?;
    }
    Ok(vec![append_event(
        conn,
        EventAppend {
            event_id: None,
            session_id: Some(state.session_id.clone()),
            run_id: None,
            source: EventSource::Intern,
            kind: kind.into(),
            payload,
            remote_sequence: None,
            command_id: None,
            created_at: None,
        },
    )?])
}

fn load_state(conn: &Connection, session_id: &str) -> Result<InternIngestionState> {
    conn.query_row(
        "SELECT s.remote_id, s.status, s.state_generation,
                COALESCE(c.cursor, 0), s.metadata_json
         FROM sessions s
         LEFT JOIN source_cursors c ON c.session_id = s.id AND c.source = ?1
         WHERE s.id = ?2",
        params![SOURCE, session_id],
        |row| {
            let metadata: String = row.get(4)?;
            let last_diagnostic = serde_json::from_str::<Value>(&metadata)
                .ok()
                .and_then(|value| {
                    value
                        .pointer("/intern/lastDiagnostic")
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                });
            let cursor: i64 = row.get(3)?;
            Ok(InternIngestionState {
                session_id: session_id.to_owned(),
                runtime_id: row.get(0)?,
                status: row.get(1)?,
                state_generation: row.get(2)?,
                remote_cursor: u64::try_from(cursor).unwrap_or_default(),
                last_diagnostic,
            })
        },
    )
    .context("load Intern ingestion state")
}

fn load_metadata(conn: &Connection, session_id: &str) -> Result<Value> {
    let value: String = conn.query_row(
        "SELECT metadata_json FROM sessions WHERE id = ?1",
        params![session_id],
        |row| row.get(0),
    )?;
    Ok(serde_json::from_str(&value).unwrap_or_else(|_| json!({})))
}

fn object_entry<'a>(value: &'a mut Value, key: &str) -> &'a mut Map<String, Value> {
    if !value.is_object() {
        *value = json!({});
    }
    let root = value.as_object_mut().expect("object was initialized");
    let entry = root.entry(key).or_insert_with(|| json!({}));
    if !entry.is_object() {
        *entry = json!({});
    }
    entry.as_object_mut().expect("object was initialized")
}

fn nonempty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

fn canonical_session_status(provider_status: &str) -> &'static str {
    match provider_status.trim().to_ascii_lowercase().as_str() {
        "running" | "active" | "working" => "running",
        "failed" | "error" => "failed",
        "completed" | "complete" | "closed" | "succeeded" | "cancelled" | "canceled" => "closed",
        "paused" | "interrupted" | "needs_input" | "awaiting_input" | "stopped" => "interrupted",
        _ => "ready",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::intern::PollUpdate;
    use tempfile::tempdir;

    fn setup() -> (InternIngestion, Arc<Database>) {
        let dir = tempdir().unwrap();
        let root = dir.keep();
        let storage = crate::storage::Storage::open(root).unwrap();
        let db = storage.database().clone();
        (InternIngestion::new(db.clone()), db)
    }

    async fn attached() -> (InternIngestion, Arc<Database>) {
        let (store, db) = setup();
        store
            .attach(InternSessionBinding {
                session_id: "session-1".into(),
                runtime_id: "runtime-1".into(),
                runtime_kind: RuntimeKind::Sync,
                title: None,
            })
            .await
            .unwrap();
        (store, db)
    }

    fn event(sequence: u64, id: &str) -> NormalizedInternEvent {
        NormalizedInternEvent {
            event_id: id.into(),
            source: SOURCE.into(),
            kind: "agent_message".into(),
            payload: json!({"body": "hello"}),
            remote_sequence: sequence,
            command_id: "command-1".into(),
            created_at: "2026-08-08T00:00:00Z".into(),
            runtime_id: "runtime-1".into(),
            state_generation: sequence,
        }
    }

    #[tokio::test]
    async fn page_and_cursor_commit_atomically_and_replay_is_idempotent() {
        let (store, db) = attached().await;
        let update = PollUpdate::Events {
            events: vec![event(1, "remote-event-1"), event(2, "remote-event-2")],
            next_sequence: 2,
        };
        assert_eq!(
            store
                .apply("session-1", update.clone())
                .await
                .unwrap()
                .len(),
            2
        );
        assert_eq!(store.resume_cursor("session-1").await.unwrap(), 2);
        assert!(store.apply("session-1", update).await.unwrap().is_empty());
        let count: i64 = db
            .with_conn(|conn| Ok(conn.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn malformed_page_rolls_back_events_and_cursor() {
        let (store, db) = attached().await;
        let error = store
            .apply(
                "session-1",
                PollUpdate::Events {
                    events: vec![event(1, "remote-event-1"), event(3, "remote-event-3")],
                    next_sequence: 3,
                },
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("gap"));
        assert_eq!(store.resume_cursor("session-1").await.unwrap(), 0);
        let count: i64 = db
            .with_conn(|conn| Ok(conn.query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn async_runtime_rejects_a_second_local_binding() {
        let (store, _) = setup();
        store
            .attach(InternSessionBinding {
                session_id: "async-session-1".into(),
                runtime_id: "org-async-singleton".into(),
                runtime_kind: RuntimeKind::Async,
                title: None,
            })
            .await
            .unwrap();
        let error = store
            .attach(InternSessionBinding {
                session_id: "async-session-2".into(),
                runtime_id: "org-async-singleton".into(),
                runtime_kind: RuntimeKind::Async,
                title: None,
            })
            .await
            .unwrap_err();
        assert!(error.to_string().contains("already bound"));
    }

    #[tokio::test]
    async fn legacy_duplicate_bindings_namespace_remote_event_ids() {
        let (store, db) = setup();
        store
            .attach(InternSessionBinding {
                session_id: "async-session-1".into(),
                runtime_id: "runtime-1".into(),
                runtime_kind: RuntimeKind::Async,
                title: None,
            })
            .await
            .unwrap();
        // Simulate a database created before singleton binding enforcement.
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO sessions(id,title,target_json,remote_id,status,metadata_json,created_at,updated_at)
                 VALUES ('async-session-2','duplicate','{\"kind\":\"intern\",\"mode\":\"async\"}',
                         'runtime-1','ready','{}','2000-01-01T00:00:00Z','2000-01-01T00:00:00Z')",
                [],
            )?;
            conn.execute(
                "INSERT INTO source_cursors(session_id,source,cursor)
                 VALUES ('async-session-2','intern',0)",
                [],
            )?;
            Ok(())
        })
        .unwrap();
        // Restart reconciliation deterministically keeps the newest binding
        // and rejects only the legacy duplicate (rather than rejecting both).
        store
            .attach(InternSessionBinding {
                session_id: "async-session-1".into(),
                runtime_id: "runtime-1".into(),
                runtime_kind: RuntimeKind::Async,
                title: None,
            })
            .await
            .unwrap();
        assert!(store
            .attach(InternSessionBinding {
                session_id: "async-session-2".into(),
                runtime_id: "runtime-1".into(),
                runtime_kind: RuntimeKind::Async,
                title: None,
            })
            .await
            .unwrap_err()
            .to_string()
            .contains("already bound"));
        let update = PollUpdate::Events {
            events: vec![event(1, "org-event-1")],
            next_sequence: 1,
        };
        store
            .apply("async-session-1", update.clone())
            .await
            .unwrap();
        store.apply("async-session-2", update).await.unwrap();
        let ids: Vec<String> = db
            .with_conn(|conn| {
                let mut statement = conn.prepare(
                    "SELECT event_id FROM events WHERE source='intern' ORDER BY session_id",
                )?;
                let ids = statement
                    .query_map([], |row| row.get(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(ids)
            })
            .unwrap();
        assert_eq!(ids.len(), 2);
        assert_ne!(ids[0], ids[1]);
        assert!(ids.iter().all(|id| id.starts_with("evt_intern_")));
    }

    #[tokio::test]
    async fn projection_updates_status_and_rejects_generation_regression() {
        let (store, _) = attached().await;
        let projection = RuntimeProjection {
            sync_session_id: Some("runtime-1".into()),
            async_runtime_id: None,
            async_assignment_id: None,
            status: "running".into(),
            state_generation: 4,
            last_event_sequence: 0,
            extra: Map::new(),
        };
        assert_eq!(
            store
                .apply(
                    "session-1",
                    PollUpdate::Projection {
                        projection: projection.clone(),
                    },
                )
                .await
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            store.state("session-1").await.unwrap().state_generation,
            Some(4)
        );
        let mut regressed = projection;
        regressed.state_generation = 3;
        assert!(store
            .apply(
                "session-1",
                PollUpdate::Projection {
                    projection: regressed
                }
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("regressed"));
    }

    #[tokio::test]
    async fn stopped_and_auth_updates_are_durable_diagnostics() {
        let (store, _) = attached().await;
        let events = store
            .apply(
                "session-1",
                PollUpdate::Stopped {
                    reason: "authentication_failed".into(),
                },
            )
            .await
            .unwrap();
        assert_eq!(events[0].kind, "intern.authentication_failed");
        let state = store.state("session-1").await.unwrap();
        assert_eq!(state.status, "interrupted");
        assert_eq!(
            state.last_diagnostic.as_deref(),
            Some("authentication_failed")
        );
    }

    #[tokio::test]
    async fn identity_drift_rolls_back() {
        let (store, _) = attached().await;
        let mut drifted = event(1, "remote-event-1");
        drifted.runtime_id = "other-runtime".into();
        assert!(store
            .apply(
                "session-1",
                PollUpdate::Events {
                    events: vec![drifted],
                    next_sequence: 1,
                },
            )
            .await
            .unwrap_err()
            .to_string()
            .contains("identity drifted"));
        assert_eq!(store.resume_cursor("session-1").await.unwrap(), 0);
    }
}
