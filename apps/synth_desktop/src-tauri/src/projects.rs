//! Rust-owned local project/workspace registry.

use crate::storage::{append_event, AppEvent, Database, EventAppend, EventSource};
use anyhow::{anyhow, bail, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRecord {
    pub id: String,
    pub name: String,
    pub path: String,
    pub vcs: Option<String>,
    pub metadata: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectCreateRequest {
    pub path: String,
    pub name: Option<String>,
    pub vcs: Option<String>,
    #[serde(default)]
    pub metadata: Option<Value>,
}

#[derive(Clone)]
pub struct ProjectStore {
    db: Arc<Database>,
}

impl ProjectStore {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub async fn list(&self) -> Result<Vec<ProjectRecord>> {
        self.db.clone().run(list_projects).await
    }

    pub async fn get(&self, id: String) -> Result<ProjectRecord> {
        self.db
            .clone()
            .run(move |conn| load_project(conn, &id))
            .await
    }

    pub async fn create(&self, request: ProjectCreateRequest) -> Result<(ProjectRecord, AppEvent)> {
        let path = request.path.trim().to_owned();
        if path.is_empty() {
            bail!("project path is required");
        }
        let name = request
            .name
            .filter(|name| !name.trim().is_empty())
            .unwrap_or_else(|| {
                std::path::Path::new(&path)
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or(&path)
                    .to_owned()
            });
        let id = format!("proj_{}", Uuid::new_v4().simple());
        let metadata = request.metadata.unwrap_or_else(|| json!({}));
        let db = self.db.clone();
        db.run_transaction(move |conn| {
            let now = Utc::now().to_rfc3339();
            conn.execute(
                "INSERT INTO projects(id,name,path,vcs,metadata_json,created_at,updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?6)",
                params![id, name, path, request.vcs, metadata.to_string(), now],
            )?;
            let project = load_project(conn, &id)?;
            let event = append_event(conn, EventAppend {
                event_id: None, session_id: None, run_id: None, source: EventSource::Local,
                kind: "project.created".into(),
                payload: json!({"projectId": project.id, "name": project.name, "path": project.path}),
                remote_sequence: None, command_id: None, created_at: Some(now),
            })?;
            Ok((project, event))
        }).await
    }

    pub async fn delete(&self, id: String) -> Result<(bool, AppEvent)> {
        let db = self.db.clone();
        db.run_transaction(move |conn| {
            let project = load_project(conn, &id)?;
            let deleted = conn.execute("DELETE FROM projects WHERE id = ?1", params![id])? > 0;
            let event = append_event(
                conn,
                EventAppend {
                    event_id: None,
                    session_id: None,
                    run_id: None,
                    source: EventSource::Local,
                    kind: "project.deleted".into(),
                    payload: json!({"projectId": project.id}),
                    remote_sequence: None,
                    command_id: None,
                    created_at: None,
                },
            )?;
            Ok((deleted, event))
        })
        .await
    }
}

fn project_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ProjectRecord> {
    let raw: String = row.get(4)?;
    Ok(ProjectRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        path: row.get(2)?,
        vcs: row.get(3)?,
        metadata: serde_json::from_str(&raw).unwrap_or_else(|_| json!({})),
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn load_project(conn: &Connection, id: &str) -> Result<ProjectRecord> {
    conn.query_row(
        "SELECT id,name,path,vcs,metadata_json,created_at,updated_at FROM projects WHERE id=?1",
        params![id],
        project_from_row,
    )
    .optional()?
    .ok_or_else(|| anyhow!("project not found: {id}"))
}

fn list_projects(conn: &Connection) -> Result<Vec<ProjectRecord>> {
    let mut statement = conn.prepare(
        "SELECT id,name,path,vcs,metadata_json,created_at,updated_at FROM projects ORDER BY updated_at DESC,id"
    )?;
    let projects = statement
        .query_map([], project_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(projects)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;
    use tempfile::tempdir;

    #[tokio::test]
    async fn project_mutations_commit_with_events() {
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let store = ProjectStore::new(storage.database().clone());
        let (created, event) = store
            .create(ProjectCreateRequest {
                path: "/tmp/example".into(),
                name: None,
                vcs: Some("git".into()),
                metadata: None,
            })
            .await
            .unwrap();
        assert_eq!(created.name, "example");
        assert_eq!(event.kind, "project.created");
        assert_eq!(store.list().await.unwrap(), vec![created.clone()]);
        let (deleted, event) = store.delete(created.id).await.unwrap();
        assert!(deleted);
        assert_eq!(event.kind, "project.deleted");
        assert!(store.list().await.unwrap().is_empty());
    }
}
