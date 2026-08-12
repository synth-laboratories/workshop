use super::database::Database;
use super::models::{AppEvent, EventSource, SessionRecord, APP_EVENT_SCHEMA_VERSION};
use crate::domain::RuntimeTarget;
use anyhow::{Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct EventAppend {
    pub event_id: Option<String>,
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    pub source: EventSource,
    pub kind: String,
    pub payload: Value,
    pub remote_sequence: Option<i64>,
    pub command_id: Option<String>,
    pub created_at: Option<String>,
}

impl EventAppend {
    pub fn system(kind: impl Into<String>, payload: Value) -> Self {
        Self {
            event_id: None,
            session_id: None,
            run_id: None,
            source: EventSource::System,
            kind: kind.into(),
            payload,
            remote_sequence: None,
            command_id: None,
            created_at: None,
        }
    }

    pub fn codex(session_id: impl Into<String>, kind: impl Into<String>, payload: Value) -> Self {
        Self {
            event_id: None,
            session_id: Some(session_id.into()),
            run_id: None,
            source: EventSource::Codex,
            kind: kind.into(),
            payload,
            remote_sequence: None,
            command_id: None,
            created_at: None,
        }
    }
}

#[derive(Clone)]
pub struct EventJournal {
    db: Arc<Database>,
}

impl EventJournal {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub async fn append(&self, input: EventAppend) -> Result<AppEvent> {
        let db = self.db.clone();
        db.run_transaction(move |conn| append_event(conn, input))
            .await
    }

    pub async fn events_after(&self, after_sequence: i64, limit: i64) -> Result<Vec<AppEvent>> {
        let db = self.db.clone();
        db.run(move |conn| list_events_after(conn, after_sequence, limit))
            .await
    }

    pub async fn session_events_after(
        &self,
        session_id: String,
        after_session_sequence: i64,
        limit: i64,
    ) -> Result<Vec<AppEvent>> {
        let db = self.db.clone();
        db.run(move |conn| {
            list_session_events_after(conn, &session_id, after_session_sequence, limit)
        })
        .await
    }

    pub async fn upsert_codex_session(
        &self,
        session_id: String,
        thread_id: String,
        title: String,
        model: String,
        workspace: String,
        status: String,
    ) -> Result<SessionRecord> {
        let db = self.db.clone();
        db.run_transaction(move |conn| {
            upsert_codex_session(
                conn,
                &session_id,
                &thread_id,
                &title,
                &model,
                &workspace,
                &status,
            )
        })
        .await
    }

    pub async fn set_session_status(&self, session_id: String, status: String) -> Result<()> {
        let db = self.db.clone();
        db.run(move |conn| {
            conn.execute(
                "UPDATE sessions SET status = ?1, updated_at = ?2 WHERE id = ?3",
                params![status, Utc::now().to_rfc3339(), session_id],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn head_sequence(&self) -> Result<i64> {
        let db = self.db.clone();
        db.run(|conn| {
            let head: i64 = conn
                .query_row("SELECT COALESCE(MAX(sequence), 0) FROM events", [], |row| {
                    row.get(0)
                })
                .optional()?
                .unwrap_or(0);
            Ok(head)
        })
        .await
    }
}

/// Append an event using the caller's connection.
///
/// Domain stores use this inside their own transaction so the state mutation and
/// its durable journal record commit (or roll back) together.
pub(crate) fn append_event(conn: &Connection, input: EventAppend) -> Result<AppEvent> {
    let event_id = input
        .event_id
        .unwrap_or_else(|| format!("evt_{}", Uuid::new_v4()));
    let created_at = input.created_at.unwrap_or_else(|| Utc::now().to_rfc3339());
    let payload_json = serde_json::to_string(&input.payload)?;

    if let (Some(session_id), Some(remote_sequence)) =
        (input.session_id.as_ref(), input.remote_sequence)
    {
        if let Some(existing) = conn
            .query_row(
                "SELECT sequence FROM events
                 WHERE session_id = ?1 AND source = ?2 AND remote_sequence = ?3",
                params![session_id, input.source.as_str(), remote_sequence],
                |row| row.get::<_, i64>(0),
            )
            .optional()?
        {
            return load_event(conn, existing);
        }
    }

    let session_sequence = if let Some(session_id) = input.session_id.as_ref() {
        ensure_session_exists(conn, session_id)?;
        let next: i64 = conn.query_row(
            "SELECT latest_cursor + 1 FROM sessions WHERE id = ?1",
            params![session_id],
            |row| row.get(0),
        )?;
        conn.execute(
            "UPDATE sessions SET latest_cursor = ?1, updated_at = ?2 WHERE id = ?3",
            params![next, &created_at, session_id],
        )?;
        Some(next)
    } else {
        None
    };

    conn.execute(
        "INSERT INTO events(
            event_id, session_id, session_sequence, run_id, source, kind,
            payload_json, remote_sequence, command_id, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            event_id,
            input.session_id,
            session_sequence,
            input.run_id,
            input.source.as_str(),
            input.kind,
            payload_json,
            input.remote_sequence,
            input.command_id,
            created_at,
        ],
    )
    .context("insert journal event")?;

    let sequence: i64 = conn.query_row("SELECT last_insert_rowid()", [], |row| row.get(0))?;
    Ok(AppEvent {
        schema_version: APP_EVENT_SCHEMA_VERSION.to_string(),
        sequence,
        event_id,
        session_id: input.session_id,
        session_sequence,
        run_id: input.run_id,
        source: input.source,
        kind: input.kind,
        payload: input.payload,
        remote_sequence: input.remote_sequence,
        command_id: input.command_id,
        created_at,
    })
}

fn ensure_session_exists(conn: &Connection, session_id: &str) -> Result<()> {
    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM sessions WHERE id = ?1",
            params![session_id],
            |_| Ok(true),
        )
        .optional()?
        .unwrap_or(false);
    if exists {
        return Ok(());
    }
    let now = Utc::now().to_rfc3339();
    let target = RuntimeTarget::local_laguna();
    conn.execute(
        "INSERT INTO sessions(
            id, title, target_json, runtime_target_kind, status, latest_cursor, metadata_json, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, 'created', 0, '{}', ?5, ?5)",
        params![
            session_id,
            format!("Session {session_id}"),
            target.to_json_value().to_string(),
            target.kind_str(),
            now
        ],
    )?;
    Ok(())
}

fn upsert_codex_session(
    conn: &Connection,
    session_id: &str,
    thread_id: &str,
    title: &str,
    model: &str,
    workspace: &str,
    status: &str,
) -> Result<SessionRecord> {
    let now = Utc::now().to_rfc3339();
    let target = RuntimeTarget::local_laguna();
    let metadata = json!({ "workspace": workspace, "model": model });
    conn.execute(
        "INSERT INTO sessions(
            id, title, target_json, runtime_target_kind, codex_thread_id, status, latest_cursor,
            metadata_json, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?8, ?8)
         ON CONFLICT(id) DO UPDATE SET
            title = excluded.title,
            target_json = excluded.target_json,
            runtime_target_kind = excluded.runtime_target_kind,
            codex_thread_id = excluded.codex_thread_id,
            status = excluded.status,
            metadata_json = excluded.metadata_json,
            updated_at = excluded.updated_at",
        params![
            session_id,
            title,
            target.to_json_value().to_string(),
            target.kind_str(),
            thread_id,
            status,
            metadata.to_string(),
            now
        ],
    )?;
    load_session(conn, session_id)
}

fn load_session(conn: &Connection, session_id: &str) -> Result<SessionRecord> {
    conn.query_row(
        "SELECT id, title, target_json, project_id, remote_id, codex_thread_id, status,
                state_generation, latest_cursor, active_run_id, metadata_json, created_at, updated_at
         FROM sessions WHERE id = ?1",
        params![session_id],
        |row| {
            Ok(SessionRecord {
                id: row.get(0)?,
                title: row.get(1)?,
                target: {
                    let raw: String = row.get(2)?;
                    let value: Value =
                        serde_json::from_str(&raw).unwrap_or(Value::Null);
                    RuntimeTarget::from_json_value_lenient(&value)
                },
                project_id: row.get(3)?,
                remote_id: row.get(4)?,
                codex_thread_id: row.get(5)?,
                status: row.get(6)?,
                state_generation: row.get(7)?,
                latest_cursor: row.get(8)?,
                active_run_id: row.get(9)?,
                metadata: serde_json::from_str(&row.get::<_, String>(10)?).unwrap_or(json!({})),
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
            })
        },
    )
    .context("load session")
}

fn load_event(conn: &Connection, sequence: i64) -> Result<AppEvent> {
    conn.query_row(
        "SELECT sequence, event_id, session_id, session_sequence, run_id, source, kind,
                payload_json, remote_sequence, command_id, created_at
         FROM events WHERE sequence = ?1",
        params![sequence],
        |row| {
            Ok(AppEvent {
                schema_version: APP_EVENT_SCHEMA_VERSION.to_string(),
                sequence: row.get(0)?,
                event_id: row.get(1)?,
                session_id: row.get(2)?,
                session_sequence: row.get(3)?,
                run_id: row.get(4)?,
                source: EventSource::parse(&row.get::<_, String>(5)?),
                kind: row.get(6)?,
                payload: serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or(Value::Null),
                remote_sequence: row.get(8)?,
                command_id: row.get(9)?,
                created_at: row.get(10)?,
            })
        },
    )
    .context("load event")
}

fn list_events_after(conn: &Connection, after_sequence: i64, limit: i64) -> Result<Vec<AppEvent>> {
    let mut stmt = conn.prepare(
        "SELECT sequence, event_id, session_id, session_sequence, run_id, source, kind,
                payload_json, remote_sequence, command_id, created_at
         FROM events
         WHERE sequence > ?1
         ORDER BY sequence ASC
         LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![after_sequence, limit], |row| {
        Ok(AppEvent {
            schema_version: APP_EVENT_SCHEMA_VERSION.to_string(),
            sequence: row.get(0)?,
            event_id: row.get(1)?,
            session_id: row.get(2)?,
            session_sequence: row.get(3)?,
            run_id: row.get(4)?,
            source: EventSource::parse(&row.get::<_, String>(5)?),
            kind: row.get(6)?,
            payload: serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or(Value::Null),
            remote_sequence: row.get(8)?,
            command_id: row.get(9)?,
            created_at: row.get(10)?,
        })
    })?;
    let mut events = Vec::new();
    for row in rows {
        events.push(row?);
    }
    Ok(events)
}

fn list_session_events_after(
    conn: &Connection,
    session_id: &str,
    after_session_sequence: i64,
    limit: i64,
) -> Result<Vec<AppEvent>> {
    let mut stmt = conn.prepare(
        "SELECT sequence, event_id, session_id, session_sequence, run_id, source, kind,
                payload_json, remote_sequence, command_id, created_at
         FROM events
         WHERE session_id = ?1 AND session_sequence > ?2
         ORDER BY session_sequence ASC
         LIMIT ?3",
    )?;
    let rows = stmt.query_map(params![session_id, after_session_sequence, limit], |row| {
        Ok(AppEvent {
            schema_version: APP_EVENT_SCHEMA_VERSION.to_string(),
            sequence: row.get(0)?,
            event_id: row.get(1)?,
            session_id: row.get(2)?,
            session_sequence: row.get(3)?,
            run_id: row.get(4)?,
            source: EventSource::parse(&row.get::<_, String>(5)?),
            kind: row.get(6)?,
            payload: serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or(Value::Null),
            remote_sequence: row.get(8)?,
            command_id: row.get(9)?,
            created_at: row.get(10)?,
        })
    })?;
    let mut events = Vec::new();
    for row in rows {
        events.push(row?);
    }
    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;
    use tempfile::tempdir;

    #[tokio::test]
    async fn append_assigns_global_and_session_sequences() {
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let journal = EventJournal::new(storage.database().clone());
        journal
            .upsert_codex_session(
                "sess_1".into(),
                "thread_1".into(),
                "Demo".into(),
                "laguna-xs-2.1".into(),
                "/tmp/ws".into(),
                "ready".into(),
            )
            .await
            .unwrap();
        let first = journal
            .append(EventAppend::codex(
                "sess_1",
                "item/agentMessage/delta",
                json!({"delta": "hi"}),
            ))
            .await
            .unwrap();
        let second = journal
            .append(EventAppend::codex(
                "sess_1",
                "turn/completed",
                json!({"ok": true}),
            ))
            .await
            .unwrap();
        assert_eq!(first.sequence, 1);
        assert_eq!(first.session_sequence, Some(1));
        assert_eq!(second.sequence, 2);
        assert_eq!(second.session_sequence, Some(2));
        let page = journal.events_after(0, 10).await.unwrap();
        assert_eq!(page.len(), 2);
    }

    #[tokio::test]
    async fn remote_dedupe_is_idempotent() {
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let journal = EventJournal::new(storage.database().clone());
        let input = EventAppend {
            event_id: Some("evt_remote_1".into()),
            session_id: Some("sess_remote".into()),
            run_id: None,
            source: EventSource::Intern,
            kind: "agent_message".into(),
            payload: json!({"body": "hello"}),
            remote_sequence: Some(7),
            command_id: None,
            created_at: None,
        };
        let a = journal.append(input.clone()).await.unwrap();
        let b = journal.append(input).await.unwrap();
        assert_eq!(a.sequence, b.sequence);
        assert_eq!(a.event_id, b.event_id);
        assert_eq!(journal.head_sequence().await.unwrap(), 1);
    }
}
