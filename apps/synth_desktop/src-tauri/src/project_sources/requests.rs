//! Pending source requests are evidence, never executable authority.
//! State transitions and their journal entries commit in one SQLite transaction.
use super::canonical_project_root;
use crate::storage::{append_event, Database, EventAppend};
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub(crate) static RESOLUTION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub(super) async fn audit(
    db: &Arc<Database>,
    kind: &'static str,
    payload: serde_json::Value,
) -> Result<()> {
    db.run_transaction(move |conn| {
        append_event(conn, EventAppend::system(kind, payload))?;
        Ok(())
    })
    .await
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSourceRequest {
    pub id: String,
    pub session_id: Option<String>,
    pub requested_path: String,
    pub canonical_path: String,
    pub reason: String,
    pub containers: bool,
    pub recipes: bool,
    pub attach_to_conversation: bool,
    pub status: String,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectSourceRequestInput {
    pub session_id: Option<String>,
    pub path: String,
    pub reason: String,
    pub containers: bool,
    pub recipes: bool,
    #[serde(default)]
    pub attach_to_conversation: bool,
}

const COLUMNS: &str = "id,session_id,requested_path,canonical_path,reason,containers,recipes,attach_to_conversation,status,created_at,resolved_at";

fn row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectSourceRequest> {
    Ok(ProjectSourceRequest {
        id: row.get(0)?,
        session_id: row.get(1)?,
        requested_path: row.get(2)?,
        canonical_path: row.get(3)?,
        reason: row.get(4)?,
        containers: row.get(5)?,
        recipes: row.get(6)?,
        attach_to_conversation: row.get(7)?,
        status: row.get(8)?,
        created_at: row.get(9)?,
        resolved_at: row.get(10)?,
    })
}

pub(super) fn load(conn: &Connection, id: &str) -> Result<ProjectSourceRequest> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM project_source_requests WHERE id=?1"),
        [id],
        row,
    )
    .optional()?
    .ok_or_else(|| anyhow!("project source request was not found"))
}

pub async fn request(
    db: &Arc<Database>,
    input: ProjectSourceRequestInput,
) -> Result<ProjectSourceRequest> {
    if !input.containers && !input.recipes {
        bail!("request containers, recipes, or both");
    }
    let reason = input.reason.trim().to_owned();
    if reason.is_empty() || reason.len() > 2048 {
        bail!("project source request reason must be 1–2048 bytes");
    }
    let session_id = input.session_id.map(|id| id.trim().to_owned());
    if session_id.as_deref() == Some("") {
        bail!("session ID must not be blank");
    }
    if input.attach_to_conversation && session_id.is_none() {
        bail!("attachment requires a conversation");
    }
    let canonical = canonical_project_root(&input.path)?.display().to_string();
    db.run_transaction(move |conn| {
        if let Some(session) = &session_id {
            let exists: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sessions WHERE id=?1)", [session], |row| row.get(0))?;
            if !exists { bail!("source request conversation was not found"); }
        }
        let existing = conn.query_row(
            &format!("SELECT {COLUMNS} FROM project_source_requests WHERE canonical_path=?1 AND session_id IS ?2 AND status='pending'"),
            params![canonical, session_id], row,
        ).optional()?;
        if let Some(existing) = existing {
            if existing.containers != input.containers || existing.recipes != input.recipes || existing.attach_to_conversation != input.attach_to_conversation {
                bail!("a pending request for this source has different permissions; resolve it before requesting a changed grant");
            }
            return Ok(existing);
        }
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute("INSERT INTO project_source_requests(id,session_id,requested_path,canonical_path,reason,containers,recipes,attach_to_conversation,status,created_at)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'pending',datetime('now'))",
            params![id, session_id, input.path.trim(), canonical, reason, input.containers, input.recipes, input.attach_to_conversation])?;
        let request = load(conn, &id)?;
        append_event(conn, EventAppend::system("project_source.requested", serde_json::to_value(&request)?))?;
        Ok(request)
    }).await
}

pub async fn list(
    db: &Arc<Database>,
    session_id: Option<String>,
) -> Result<Vec<ProjectSourceRequest>> {
    db.run(move |conn| {
        let mut statement = conn.prepare(&format!("SELECT {COLUMNS} FROM project_source_requests WHERE (?1 IS NULL OR session_id=?1) ORDER BY created_at DESC,id DESC"))?;
        let rows = statement.query_map([session_id], row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }).await
}

pub async fn deny(db: &Arc<Database>, id: &str) -> Result<ProjectSourceRequest> {
    let _resolution = RESOLUTION.lock().await;
    let id = id.to_owned();
    db.run_transaction(move |conn| {
        let changed = conn.execute("UPDATE project_source_requests SET status='denied',resolved_at=datetime('now') WHERE id=?1 AND status='pending'", [&id])?;
        if changed != 1 { bail!("pending project source request was not found"); }
        let request = load(conn, &id)?;
        append_event(conn, EventAppend::system("project_source.denied", serde_json::to_value(&request)?))?;
        Ok(request)
    }).await
}

