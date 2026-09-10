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

#[cfg(test)]
mod tests {
    use super::*;

    fn input(path: &std::path::Path) -> ProjectSourceRequestInput {
        ProjectSourceRequestInput {
            session_id: None,
            path: path.display().to_string(),
            reason: "Run this project's declared evaluation".into(),
            containers: true,
            recipes: false,
            attach_to_conversation: false,
        }
    }

    #[tokio::test]
    async fn requests_survive_reopen_dedupe_and_refuse_permission_rewrites() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("state.sqlite3");
        let db = Arc::new(Database::open(&path).unwrap());
        let first = request(&db, input(directory.path())).await.unwrap();
        let retry = request(&db, input(directory.path())).await.unwrap();
        assert_eq!(first, retry);
        let mut changed = input(directory.path());
        changed.recipes = true;
        assert!(request(&db, changed).await.is_err());
        assert_eq!(first.status, "pending");
        assert!(first.resolved_at.is_none());
        drop(db);
        let db = Arc::new(Database::open(&path).unwrap());
        assert_eq!(list(&db, None).await.unwrap(), vec![first.clone()]);
        let denied = deny(&db, &first.id).await.unwrap();
        assert_eq!(denied.status, "denied");
        assert!(denied.resolved_at.is_some());
        assert!(deny(&db, &first.id).await.is_err());
        let new_request = request(&db, input(directory.path())).await.unwrap();
        assert_ne!(new_request.id, first.id);
        let events = db
            .with_conn(|conn| {
                let mut statement = conn.prepare(
                    "SELECT kind FROM events WHERE kind LIKE 'project_source.%' ORDER BY sequence",
                )?;
                let rows = statement
                    .query_map([], |row| row.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap();
        assert_eq!(
            events,
            [
                "project_source.requested",
                "project_source.denied",
                "project_source.requested"
            ]
        );
    }

    #[tokio::test]
    async fn concurrent_retries_create_one_pending_request() {
        let directory = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open(directory.path().join("state.sqlite3")).unwrap());
        let (one, two) = tokio::join!(
            request(&db, input(directory.path())),
            request(&db, input(directory.path()))
        );
        assert_eq!(one.unwrap().id, two.unwrap().id);
        assert_eq!(list(&db, None).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn denial_and_request_rows_roll_back_when_their_journal_write_fails() {
        let directory = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open(directory.path().join("state.sqlite3")).unwrap());
        let pending = request(&db, input(directory.path())).await.unwrap();
        db.with_conn(|conn| {
            conn.execute_batch("CREATE TRIGGER reject_source_journal BEFORE INSERT ON events BEGIN SELECT RAISE(ABORT,'fixture journal failure'); END;")?;
            Ok(())
        }).unwrap();
        assert!(deny(&db, &pending.id).await.is_err());
        assert_eq!(list(&db, None).await.unwrap(), vec![pending]);
        let other = directory.path().join("another-source");
        std::fs::create_dir(&other).unwrap();
        assert!(request(&db, input(&other)).await.is_err());
        assert_eq!(list(&db, None).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn invalid_requests_do_not_create_rows() {
        let directory = tempfile::tempdir().unwrap();
        let db = Arc::new(Database::open(directory.path().join("state.sqlite3")).unwrap());
        for case in 0..5 {
            let mut value = input(directory.path());
            match case {
                0 => value.containers = false,
                1 => value.reason = " ".into(),
                2 => value.reason = "x".repeat(2049),
                3 => value.attach_to_conversation = true,
                _ => value.session_id = Some("missing-session".into()),
            }
            assert!(request(&db, value).await.is_err());
        }
        assert!(list(&db, None).await.unwrap().is_empty());
    }
}
