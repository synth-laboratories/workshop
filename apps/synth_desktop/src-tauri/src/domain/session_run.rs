use super::{RuntimeTarget, SessionKind};
use crate::storage::{
    append_event, AppEvent, CommandReceiptRecord, Database, EventAppend, EventSource, RunRecord,
    SessionRecord,
};
use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct DomainMutation<T> {
    pub value: T,
    pub event: Option<AppEvent>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Created,
    Ready,
    Running,
    Interrupted,
    Failed,
    Closed,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionTitleOrigin {
    Default,
    Automatic,
    Manual,
}

impl SessionTitleOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Automatic => "automatic",
            Self::Manual => "manual",
        }
    }
}

impl SessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Ready => "ready",
            Self::Running => "running",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
            Self::Closed => "closed",
        }
    }

    pub fn parse(value: &str) -> Result<Self> {
        match value {
            "created" => Ok(Self::Created),
            "ready" => Ok(Self::Ready),
            "running" => Ok(Self::Running),
            "interrupted" => Ok(Self::Interrupted),
            "failed" => Ok(Self::Failed),
            "closed" => Ok(Self::Closed),
            _ => bail!("unknown session status: {value}"),
        }
    }

    pub fn equals_str(self, value: &str) -> bool {
        self.as_str() == value
    }

    fn can_transition_to(self, next: Self) -> bool {
        self == next
            || matches!(
                (self, next),
                (
                    Self::Created,
                    Self::Ready | Self::Running | Self::Failed | Self::Closed
                ) | (Self::Ready, Self::Running | Self::Failed | Self::Closed)
                    | (
                        Self::Running,
                        Self::Ready | Self::Interrupted | Self::Failed | Self::Closed
                    )
                    | (
                        Self::Interrupted,
                        Self::Ready | Self::Running | Self::Closed
                    )
                    | (Self::Failed, Self::Ready | Self::Running | Self::Closed)
            )
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Created,
    Running,
    Completed,
    Interrupted,
    Failed,
    Cancelled,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Interrupted => "interrupted",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "created" => Ok(Self::Created),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "interrupted" => Ok(Self::Interrupted),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => bail!("unknown run status: {value}"),
        }
    }

    fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Interrupted | Self::Failed | Self::Cancelled
        )
    }

    fn can_transition_to(self, next: Self) -> bool {
        self == next
            || matches!(
                (self, next),
                (
                    Self::Created,
                    Self::Running | Self::Cancelled | Self::Failed
                ) | (
                    Self::Running,
                    Self::Completed | Self::Interrupted | Self::Failed | Self::Cancelled
                )
            )
    }
}

#[derive(Clone, Debug)]
pub struct SessionCreate {
    pub id: String,
    pub title: String,
    pub kind: SessionKind,
    pub target: RuntimeTarget,
    pub project_id: Option<String>,
    pub remote_id: Option<String>,
    pub codex_thread_id: Option<String>,
    pub status: SessionStatus,
    pub state_generation: Option<i64>,
    pub metadata: Value,
    pub source: EventSource,
}

#[derive(Clone, Debug)]
pub struct RunCreate {
    pub id: String,
    pub session_id: String,
    pub mode: String,
    pub model: Option<String>,
    pub adapter: Option<String>,
    pub metadata: Value,
    pub source: EventSource,
}

#[derive(Clone, Debug)]
pub struct CommandReceiptInput {
    pub command_id: String,
    pub session_id: String,
    pub run_id: Option<String>,
    pub source: EventSource,
    pub kind: String,
    pub request: Value,
}

#[derive(Clone)]
pub struct SessionService {
    db: Arc<Database>,
}

impl SessionService {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub async fn create_or_update(
        &self,
        input: SessionCreate,
    ) -> Result<DomainMutation<SessionRecord>> {
        validate_id("session", &input.id)?;
        if input.title.trim().is_empty() {
            bail!("session title must not be empty");
        }
        let db = self.db.clone();
        db.run_transaction(move |conn| upsert_session(conn, input))
            .await
    }

    pub async fn get(&self, id: String) -> Result<Option<SessionRecord>> {
        let db = self.db.clone();
        db.run(move |conn| load_session(conn, &id).optional().map_err(Into::into))
            .await
    }

    pub async fn list(&self, limit: i64) -> Result<Vec<SessionRecord>> {
        let db = self.db.clone();
        db.run(move |conn| list_sessions(conn, limit.clamp(1, 2_000)))
            .await
    }

    /// Changes the user-facing title and its provenance in one transaction.
    /// Automatic titles may only replace a default title; a manual title is
    /// therefore never overwritten by a delayed automatic naming attempt.
    pub async fn set_title(
        &self,
        id: String,
        title: String,
        origin: SessionTitleOrigin,
    ) -> Result<DomainMutation<SessionRecord>> {
        let title = title.trim().to_owned();
        if title.is_empty() {
            bail!("session title must not be empty");
        }
        let db = self.db.clone();
        db.run_transaction(move |conn| {
            let current = load_session(conn, &id).context("session not found")?;
            let current_origin = current
                .metadata
                .get("titleOrigin")
                .and_then(Value::as_str)
                .unwrap_or("legacy");
            if origin == SessionTitleOrigin::Automatic && current_origin != "default" {
                return Ok(DomainMutation {
                    value: current,
                    event: None,
                });
            }
            if current.title == title && current_origin == origin.as_str() {
                return Ok(DomainMutation {
                    value: current,
                    event: None,
                });
            }
            let previous_title = current.title.clone();
            let mut metadata = current.metadata.clone();
            let object = metadata
                .as_object_mut()
                .ok_or_else(|| anyhow!("session metadata must be an object"))?;
            object.insert("titleOrigin".into(), json!(origin.as_str()));
            let now = Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE sessions SET title = ?1, metadata_json = ?2, updated_at = ?3 WHERE id = ?4",
                params![title, metadata.to_string(), now, id],
            )?;
            let event = append_event(
                conn,
                EventAppend {
                    event_id: None,
                    session_id: Some(id.clone()),
                    run_id: current.active_run_id,
                    source: EventSource::Codex,
                    kind: "session.title_changed".into(),
                    payload: json!({
                        "from": previous_title,
                        "to": title,
                        "origin": origin.as_str(),
                    }),
                    remote_sequence: None,
                    command_id: None,
                    created_at: Some(now),
                },
            )?;
            Ok(DomainMutation {
                value: load_session(conn, &id)?,
                event: Some(event),
            })
        })
        .await
    }

    pub async fn transition(
        &self,
        id: String,
        next: SessionStatus,
        source: EventSource,
        detail: Value,
    ) -> Result<DomainMutation<SessionRecord>> {
        let db = self.db.clone();
        db.run_transaction(move |conn| {
            let current_record = load_session(conn, &id).context("session not found")?;
            let current = SessionStatus::parse(&current_record.status)?;
            if !current.can_transition_to(next) {
                bail!(
                    "invalid session transition: {} -> {}",
                    current.as_str(),
                    next.as_str()
                );
            }
            if current == next {
                return Ok(DomainMutation {
                    value: current_record,
                    event: None,
                });
            }
            let now = Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE sessions SET status = ?1, updated_at = ?2 WHERE id = ?3",
                params![next.as_str(), now, id],
            )?;
            let event = append_event(
                conn,
                EventAppend {
                    event_id: None,
                    session_id: Some(id.clone()),
                    run_id: current_record.active_run_id.clone(),
                    source,
                    kind: "session.status_changed".into(),
                    payload: json!({
                        "from": current.as_str(),
                        "to": next.as_str(),
                        "detail": detail,
                    }),
                    remote_sequence: None,
                    command_id: None,
                    created_at: Some(now),
                },
            )?;
            Ok(DomainMutation {
                value: load_session(conn, &id)?,
                event: Some(event),
            })
        })
        .await
    }

    pub async fn source_cursor(&self, session_id: String, source: EventSource) -> Result<i64> {
        let db = self.db.clone();
        db.run(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT cursor FROM source_cursors WHERE session_id = ?1 AND source = ?2",
                    params![session_id, source.as_str()],
                    |row| row.get(0),
                )
                .optional()?
                .unwrap_or(0))
        })
        .await
    }

    pub async fn advance_source_cursor(
        &self,
        session_id: String,
        source: EventSource,
        cursor: i64,
    ) -> Result<DomainMutation<i64>> {
        if cursor < 0 {
            bail!("source cursor must be non-negative");
        }
        let db = self.db.clone();
        db.run_transaction(move |conn| {
            load_session(conn, &session_id).context("session not found")?;
            let current: i64 = conn
                .query_row(
                    "SELECT cursor FROM source_cursors WHERE session_id = ?1 AND source = ?2",
                    params![session_id, source.as_str()],
                    |row| row.get(0),
                )
                .optional()?
                .unwrap_or(0);
            if cursor < current {
                bail!("source cursor regression: {current} -> {cursor}");
            }
            if cursor == current {
                return Ok(DomainMutation {
                    value: current,
                    event: None,
                });
            }
            conn.execute(
                "INSERT INTO source_cursors(session_id, source, cursor) VALUES (?1, ?2, ?3)
                 ON CONFLICT(session_id, source) DO UPDATE SET cursor = excluded.cursor",
                params![session_id, source.as_str(), cursor],
            )?;
            let event = append_event(
                conn,
                EventAppend {
                    event_id: None,
                    session_id: Some(session_id),
                    run_id: None,
                    source: source.clone(),
                    kind: "session.source_cursor_advanced".into(),
                    payload: json!({"source": source.as_str(), "from": current, "to": cursor}),
                    remote_sequence: None,
                    command_id: None,
                    created_at: None,
                },
            )?;
            Ok(DomainMutation {
                value: cursor,
                event: Some(event),
            })
        })
        .await
    }
}

#[derive(Clone)]
pub struct RunService {
    db: Arc<Database>,
}

impl RunService {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub async fn start(&self, input: RunCreate) -> Result<DomainMutation<RunRecord>> {
        validate_id("run", &input.id)?;
        if input.mode.trim().is_empty() {
            bail!("run mode must not be empty");
        }
        let db = self.db.clone();
        db.run_transaction(move |conn| start_run(conn, input)).await
    }

    pub async fn get(&self, id: String) -> Result<Option<RunRecord>> {
        let db = self.db.clone();
        db.run(move |conn| load_run(conn, &id).optional().map_err(Into::into))
            .await
    }

    pub async fn list_for_session(&self, session_id: String, limit: i64) -> Result<Vec<RunRecord>> {
        let db = self.db.clone();
        db.run(move |conn| list_runs(conn, &session_id, limit.clamp(1, 2_000)))
            .await
    }

    pub async fn transition(
        &self,
        id: String,
        next: RunStatus,
        outcome: Option<Value>,
        source: EventSource,
    ) -> Result<DomainMutation<RunRecord>> {
        let db = self.db.clone();
        db.run_transaction(move |conn| transition_run(conn, &id, next, outcome, source))
            .await
    }

    pub async fn accept_command(
        &self,
        input: CommandReceiptInput,
    ) -> Result<DomainMutation<CommandReceiptRecord>> {
        validate_id("command", &input.command_id)?;
        let db = self.db.clone();
        db.run_transaction(move |conn| accept_command(conn, input))
            .await
    }

    pub async fn resolve_command(
        &self,
        command_id: String,
        status: String,
        response: Value,
        remote_cursor: Option<i64>,
    ) -> Result<DomainMutation<CommandReceiptRecord>> {
        if !matches!(status.as_str(), "completed" | "failed" | "rejected") {
            bail!("invalid terminal command status: {status}");
        }
        let db = self.db.clone();
        db.run_transaction(move |conn| {
            let current = load_receipt(conn, &command_id).context("command receipt not found")?;
            if current.status != "accepted" {
                if current.status == status && current.response.as_ref() == Some(&response) {
                    return Ok(DomainMutation {
                        value: current,
                        event: None,
                    });
                }
                bail!("command receipt is already terminal: {}", current.status);
            }
            let now = Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE command_receipts
                 SET status = ?1, response_json = ?2, remote_cursor = ?3, updated_at = ?4
                 WHERE command_id = ?5",
                params![status, response.to_string(), remote_cursor, now, command_id],
            )?;
            let event = append_event(
                conn,
                EventAppend {
                    event_id: None,
                    session_id: Some(current.session_id.clone()),
                    run_id: current.run_id.clone(),
                    source: current.source.clone(),
                    kind: "command.resolved".into(),
                    payload: json!({"commandId": command_id, "status": status, "response": response, "remoteCursor": remote_cursor}),
                    remote_sequence: None,
                    command_id: Some(command_id.clone()),
                    created_at: Some(now),
                },
            )?;
            Ok(DomainMutation {
                value: load_receipt(conn, &command_id)?,
                event: Some(event),
            })
        })
        .await
    }

    pub async fn command_receipt(
        &self,
        command_id: String,
    ) -> Result<Option<CommandReceiptRecord>> {
        let db = self.db.clone();
        db.run(move |conn| {
            load_receipt(conn, &command_id)
                .optional()
                .map_err(Into::into)
        })
        .await
    }
}

fn upsert_session(
    conn: &Connection,
    input: SessionCreate,
) -> Result<DomainMutation<SessionRecord>> {
    let now = Utc::now().to_rfc3339();
    let existing = load_session(conn, &input.id).optional()?;
    if let Some(record) = &existing {
        let current = SessionStatus::parse(&record.status)?;
        if !current.can_transition_to(input.status) {
            bail!(
                "invalid session transition: {} -> {}",
                current.as_str(),
                input.status.as_str()
            );
        }
    }
    let target_json = input.target.to_json_value().to_string();
    let runtime_target_kind = input.target.kind_str();
    conn.execute(
        "INSERT INTO sessions(
            id, title, kind, target_json, runtime_target_kind, project_id, remote_id, codex_thread_id, status,
            state_generation, latest_cursor, metadata_json, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, ?11, ?12, ?12)
         ON CONFLICT(id) DO UPDATE SET
            title = excluded.title,
            kind = excluded.kind,
            target_json = excluded.target_json,
            runtime_target_kind = excluded.runtime_target_kind,
            project_id = COALESCE(excluded.project_id, sessions.project_id),
            remote_id = COALESCE(excluded.remote_id, sessions.remote_id),
            codex_thread_id = COALESCE(excluded.codex_thread_id, sessions.codex_thread_id),
            status = excluded.status,
            state_generation = COALESCE(excluded.state_generation, sessions.state_generation),
            metadata_json = excluded.metadata_json,
            updated_at = excluded.updated_at",
        params![
            input.id,
            input.title,
            input.kind.as_str(),
            target_json,
            runtime_target_kind,
            input.project_id,
            input.remote_id,
            input.codex_thread_id,
            input.status.as_str(),
            input.state_generation,
            input.metadata.to_string(),
            now,
        ],
    )?;
    let kind = if existing.is_some() {
        "session.updated"
    } else {
        "session.created"
    };
    let event = append_event(
        conn,
        EventAppend {
            event_id: None,
            session_id: Some(input.id.clone()),
            run_id: None,
            source: input.source,
            kind: kind.into(),
            payload: json!({"status": input.status.as_str()}),
            remote_sequence: None,
            command_id: None,
            created_at: Some(now),
        },
    )?;
    Ok(DomainMutation {
        value: load_session(conn, &input.id)?,
        event: Some(event),
    })
}

fn start_run(conn: &Connection, input: RunCreate) -> Result<DomainMutation<RunRecord>> {
    let session = load_session(conn, &input.session_id).context("session not found")?;
    let session_status = SessionStatus::parse(&session.status)?;
    if !session_status.can_transition_to(SessionStatus::Running) {
        bail!("session cannot start a run from {}", session.status);
    }
    if let Some(active) = session.active_run_id {
        bail!("session already has active run: {active}");
    }
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO runs(
            id, session_id, mode, status, latest_cursor, model, adapter, metadata_json,
            created_at, started_at, updated_at
         ) VALUES (?1, ?2, ?3, 'running', 0, ?4, ?5, ?6, ?7, ?7, ?7)",
        params![
            input.id,
            input.session_id,
            input.mode,
            input.model,
            input.adapter,
            input.metadata.to_string(),
            now,
        ],
    )?;
    conn.execute(
        "UPDATE sessions SET status = 'running', active_run_id = ?1, updated_at = ?2 WHERE id = ?3",
        params![input.id, now, input.session_id],
    )?;
    let event = append_event(
        conn,
        EventAppend {
            event_id: None,
            session_id: Some(input.session_id.clone()),
            run_id: Some(input.id.clone()),
            source: input.source,
            kind: "run.started".into(),
            payload: json!({"runId": input.id, "mode": input.mode}),
            remote_sequence: None,
            command_id: None,
            created_at: Some(now),
        },
    )?;
    Ok(DomainMutation {
        value: load_run(conn, &input.id)?,
        event: Some(event),
    })
}

fn transition_run(
    conn: &Connection,
    id: &str,
    next: RunStatus,
    outcome: Option<Value>,
    source: EventSource,
) -> Result<DomainMutation<RunRecord>> {
    let current_record = load_run(conn, id).context("run not found")?;
    let current = RunStatus::parse(&current_record.status)?;
    if !current.can_transition_to(next) {
        bail!(
            "invalid run transition: {} -> {}",
            current.as_str(),
            next.as_str()
        );
    }
    if current == next {
        return Ok(DomainMutation {
            value: current_record,
            event: None,
        });
    }
    let now = Utc::now().to_rfc3339();
    let completed_at = next.terminal().then_some(now.clone());
    conn.execute(
        "UPDATE runs SET status = ?1, outcome_json = COALESCE(?2, outcome_json),
            completed_at = ?3, updated_at = ?4 WHERE id = ?5",
        params![
            next.as_str(),
            outcome.as_ref().map(Value::to_string),
            completed_at,
            now,
            id,
        ],
    )?;
    if next.terminal() {
        let session_status = match next {
            RunStatus::Completed | RunStatus::Cancelled => SessionStatus::Ready,
            RunStatus::Interrupted => SessionStatus::Interrupted,
            RunStatus::Failed => SessionStatus::Failed,
            _ => unreachable!(),
        };
        conn.execute(
            "UPDATE sessions SET status = ?1, active_run_id = NULL, updated_at = ?2
             WHERE id = ?3 AND active_run_id = ?4",
            params![session_status.as_str(), now, current_record.session_id, id],
        )?;
    }
    let event = append_event(
        conn,
        EventAppend {
            event_id: None,
            session_id: Some(current_record.session_id.clone()),
            run_id: Some(id.to_owned()),
            source,
            kind: "run.status_changed".into(),
            payload: json!({"runId": id, "from": current.as_str(), "to": next.as_str(), "outcome": outcome}),
            remote_sequence: None,
            command_id: None,
            created_at: Some(now),
        },
    )?;
    Ok(DomainMutation {
        value: load_run(conn, id)?,
        event: Some(event),
    })
}

fn accept_command(
    conn: &Connection,
    input: CommandReceiptInput,
) -> Result<DomainMutation<CommandReceiptRecord>> {
    load_session(conn, &input.session_id).context("session not found")?;
    if let Some(existing) = load_receipt(conn, &input.command_id).optional()? {
        if existing.session_id == input.session_id
            && existing.run_id == input.run_id
            && existing.source == input.source
            && existing.kind == input.kind
            && existing.request == input.request
        {
            return Ok(DomainMutation {
                value: existing,
                event: None,
            });
        }
        bail!(
            "command id reused with different input: {}",
            input.command_id
        );
    }
    if let Some(run_id) = &input.run_id {
        let run = load_run(conn, run_id).context("run not found")?;
        if run.session_id != input.session_id {
            bail!("command run does not belong to session");
        }
    }
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO command_receipts(
            command_id, session_id, run_id, source, kind, status, request_json,
            created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, 'accepted', ?6, ?7, ?7)",
        params![
            input.command_id,
            input.session_id,
            input.run_id,
            input.source.as_str(),
            input.kind,
            input.request.to_string(),
            now,
        ],
    )?;
    let event = append_event(
        conn,
        EventAppend {
            event_id: None,
            session_id: Some(input.session_id.clone()),
            run_id: input.run_id.clone(),
            source: input.source,
            kind: "command.accepted".into(),
            payload: json!({"commandId": input.command_id, "kind": input.kind}),
            remote_sequence: None,
            command_id: Some(input.command_id.clone()),
            created_at: Some(now),
        },
    )?;
    Ok(DomainMutation {
        value: load_receipt(conn, &input.command_id)?,
        event: Some(event),
    })
}

fn load_session(conn: &Connection, id: &str) -> rusqlite::Result<SessionRecord> {
    conn.query_row(
        "SELECT id, title, kind, target_json, project_id, remote_id, codex_thread_id, status,
                state_generation, latest_cursor, active_run_id, metadata_json, created_at, updated_at
         FROM sessions WHERE id = ?1",
        params![id],
        session_from_row,
    )
}

fn list_sessions(conn: &Connection, limit: i64) -> Result<Vec<SessionRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, kind, target_json, project_id, remote_id, codex_thread_id, status,
                state_generation, latest_cursor, active_run_id, metadata_json, created_at, updated_at
         FROM sessions ORDER BY updated_at DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit], session_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn session_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SessionRecord> {
    Ok(SessionRecord {
        id: row.get(0)?,
        title: row.get(1)?,
        kind: row.get(2)?,
        target: parse_target_json(row.get(3)?),
        project_id: row.get(4)?,
        remote_id: row.get(5)?,
        codex_thread_id: row.get(6)?,
        status: row.get(7)?,
        state_generation: row.get(8)?,
        latest_cursor: row.get(9)?,
        active_run_id: row.get(10)?,
        metadata: json_value(row.get(11)?),
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

fn load_run(conn: &Connection, id: &str) -> rusqlite::Result<RunRecord> {
    conn.query_row(
        "SELECT id, session_id, mode, status, latest_cursor, checkpoint_json, outcome_json,
                model, adapter, metadata_json, created_at, started_at, completed_at,
                COALESCE(updated_at, completed_at, started_at, created_at)
         FROM runs WHERE id = ?1",
        params![id],
        |row| run_from_row(row),
    )
}

fn list_runs(conn: &Connection, session_id: &str, limit: i64) -> Result<Vec<RunRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, session_id, mode, status, latest_cursor, checkpoint_json, outcome_json,
                model, adapter, metadata_json, created_at, started_at, completed_at,
                COALESCE(updated_at, completed_at, started_at, created_at)
         FROM runs WHERE session_id = ?1 ORDER BY created_at DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![session_id, limit], run_from_row)?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

fn run_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RunRecord> {
    Ok(RunRecord {
        id: row.get(0)?,
        session_id: row.get(1)?,
        mode: row.get(2)?,
        status: row.get(3)?,
        latest_cursor: row.get(4)?,
        checkpoint: optional_json(row.get(5)?),
        outcome: optional_json(row.get(6)?),
        model: row.get(7)?,
        adapter: row.get(8)?,
        metadata: json_value(row.get(9)?),
        created_at: row.get(10)?,
        started_at: row.get(11)?,
        completed_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}

fn load_receipt(conn: &Connection, id: &str) -> rusqlite::Result<CommandReceiptRecord> {
    conn.query_row(
        "SELECT command_id, session_id, run_id, source, kind, status, request_json,
                response_json, remote_cursor, created_at, updated_at
         FROM command_receipts WHERE command_id = ?1",
        params![id],
        |row| {
            Ok(CommandReceiptRecord {
                command_id: row.get(0)?,
                session_id: row.get(1)?,
                run_id: row.get(2)?,
                source: EventSource::parse(&row.get::<_, String>(3)?),
                kind: row.get(4)?,
                status: row.get(5)?,
                request: json_value(row.get(6)?),
                response: optional_json(row.get(7)?),
                remote_cursor: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            })
        },
    )
}

fn validate_id(kind: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 256 {
        return Err(anyhow!("{kind} id must contain 1..=256 characters"));
    }
    Ok(())
}

fn json_value(raw: String) -> Value {
    serde_json::from_str(&raw).unwrap_or(Value::Null)
}

fn parse_target_json(raw: String) -> RuntimeTarget {
    let value = json_value(raw);
    RuntimeTarget::from_json_value_lenient(&value)
}

fn optional_json(raw: Option<String>) -> Option<Value> {
    raw.and_then(|value| serde_json::from_str(&value).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;
    use tempfile::tempdir;

    fn services() -> (tempfile::TempDir, SessionService, RunService) {
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let db = storage.database().clone();
        (dir, SessionService::new(db.clone()), RunService::new(db))
    }

    async fn create_session(sessions: &SessionService) -> DomainMutation<SessionRecord> {
        sessions
            .create_or_update(SessionCreate {
                id: "session-1".into(),
                title: "Test".into(),
                kind: SessionKind::Codex,
                target: RuntimeTarget::local_laguna(),
                project_id: None,
                remote_id: None,
                codex_thread_id: Some("thread-1".into()),
                status: SessionStatus::Ready,
                state_generation: None,
                metadata: json!({"titleOrigin":"default"}),
                source: EventSource::Codex,
            })
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn automatic_title_only_replaces_a_default_title() {
        let (_dir, sessions, _runs) = services();
        create_session(&sessions).await;
        let automatic = sessions
            .set_title(
                "session-1".into(),
                "Inspect Craftax rollouts".into(),
                SessionTitleOrigin::Automatic,
            )
            .await
            .unwrap();
        assert_eq!(automatic.value.title, "Inspect Craftax rollouts");
        assert_eq!(automatic.value.metadata["titleOrigin"], "automatic");

        let ignored = sessions
            .set_title(
                "session-1".into(),
                "A delayed automatic title".into(),
                SessionTitleOrigin::Automatic,
            )
            .await
            .unwrap();
        assert!(ignored.event.is_none());
        assert_eq!(ignored.value.title, "Inspect Craftax rollouts");
    }

    #[tokio::test]
    async fn manual_title_is_authoritative() {
        let (_dir, sessions, _runs) = services();
        create_session(&sessions).await;
        sessions
            .set_title(
                "session-1".into(),
                "My Craftax investigation".into(),
                SessionTitleOrigin::Manual,
            )
            .await
            .unwrap();
        let ignored = sessions
            .set_title(
                "session-1".into(),
                "Run two rollouts".into(),
                SessionTitleOrigin::Automatic,
            )
            .await
            .unwrap();
        assert_eq!(ignored.value.title, "My Craftax investigation");
        assert_eq!(ignored.value.metadata["titleOrigin"], "manual");
    }

    #[tokio::test]
    async fn session_mutation_and_event_commit_together() {
        let (_dir, sessions, _) = services();
        let created = create_session(&sessions).await;
        assert_eq!(created.value.status, "ready");
        assert_eq!(created.event.unwrap().kind, "session.created");
        assert_eq!(created.value.latest_cursor, 1);
    }

    #[tokio::test]
    async fn invalid_session_transition_rolls_back_without_event() {
        let (_dir, sessions, _) = services();
        create_session(&sessions).await;
        sessions
            .transition(
                "session-1".into(),
                SessionStatus::Closed,
                EventSource::Codex,
                json!({}),
            )
            .await
            .unwrap();
        assert!(sessions
            .transition(
                "session-1".into(),
                SessionStatus::Running,
                EventSource::Codex,
                json!({}),
            )
            .await
            .is_err());
        assert_eq!(
            sessions
                .get("session-1".into())
                .await
                .unwrap()
                .unwrap()
                .latest_cursor,
            2
        );
    }

    #[tokio::test]
    async fn run_owns_session_active_run_and_terminal_status() {
        let (_dir, sessions, runs) = services();
        create_session(&sessions).await;
        let started = runs
            .start(RunCreate {
                id: "run-1".into(),
                session_id: "session-1".into(),
                mode: "codex_turn".into(),
                model: Some("laguna".into()),
                adapter: None,
                metadata: json!({}),
                source: EventSource::Codex,
            })
            .await
            .unwrap();
        assert_eq!(started.value.status, "running");
        assert_eq!(
            sessions
                .get("session-1".into())
                .await
                .unwrap()
                .unwrap()
                .active_run_id
                .as_deref(),
            Some("run-1")
        );
        runs.transition(
            "run-1".into(),
            RunStatus::Completed,
            Some(json!({"ok":true})),
            EventSource::Codex,
        )
        .await
        .unwrap();
        let session = sessions.get("session-1".into()).await.unwrap().unwrap();
        assert_eq!(session.status, "ready");
        assert!(session.active_run_id.is_none());
    }

    #[tokio::test]
    async fn second_active_run_is_rejected_atomically() {
        let (_dir, sessions, runs) = services();
        create_session(&sessions).await;
        let input = |id: &str| RunCreate {
            id: id.into(),
            session_id: "session-1".into(),
            mode: "test".into(),
            model: None,
            adapter: None,
            metadata: json!({}),
            source: EventSource::Local,
        };
        runs.start(input("run-1")).await.unwrap();
        assert!(runs.start(input("run-2")).await.is_err());
        assert!(runs.get("run-2".into()).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn command_receipts_are_idempotent_and_detect_conflicts() {
        let (_dir, sessions, runs) = services();
        create_session(&sessions).await;
        let input = CommandReceiptInput {
            command_id: "command-1".into(),
            session_id: "session-1".into(),
            run_id: None,
            source: EventSource::Intern,
            kind: "message".into(),
            request: json!({"text":"hi"}),
        };
        assert!(runs
            .accept_command(input.clone())
            .await
            .unwrap()
            .event
            .is_some());
        assert!(runs
            .accept_command(input.clone())
            .await
            .unwrap()
            .event
            .is_none());
        let mut conflict = input;
        conflict.request = json!({"text":"different"});
        assert!(runs.accept_command(conflict).await.is_err());
        let resolved = runs
            .resolve_command(
                "command-1".into(),
                "completed".into(),
                json!({"ok":true}),
                Some(9),
            )
            .await
            .unwrap();
        assert_eq!(resolved.value.remote_cursor, Some(9));
    }

    #[tokio::test]
    async fn source_cursors_only_advance() {
        let (_dir, sessions, _) = services();
        create_session(&sessions).await;
        sessions
            .advance_source_cursor("session-1".into(), EventSource::Intern, 4)
            .await
            .unwrap();
        assert!(sessions
            .advance_source_cursor("session-1".into(), EventSource::Intern, 3)
            .await
            .is_err());
        assert_eq!(
            sessions
                .source_cursor("session-1".into(), EventSource::Intern)
                .await
                .unwrap(),
            4
        );
    }
}
