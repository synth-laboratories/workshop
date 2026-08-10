use crate::storage::Database;
use anyhow::{anyhow, Context, Result};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceAccessMode {
    ReadOnly,
    ReadWrite,
}

impl WorkspaceAccessMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::ReadWrite => "read_write",
        }
    }
    fn parse(value: &str) -> Result<Self> {
        match value {
            "read_only" => Ok(Self::ReadOnly),
            "read_write" => Ok(Self::ReadWrite),
            _ => Err(anyhow!("invalid workspace access mode")),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AttachmentSource {
    UserPicker,
    RecentFolder,
    AgentRequest,
    MigratedDefault,
}
impl AttachmentSource {
    fn as_str(self) -> &'static str {
        match self {
            Self::UserPicker => "user_picker",
            Self::RecentFolder => "recent_folder",
            Self::AgentRequest => "agent_request",
            Self::MigratedDefault => "migrated_default",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceAttachment {
    pub path: String,
    pub access: WorkspaceAccessMode,
    pub source: AttachmentSource,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConversationWorkspaceScope {
    pub session_id: String,
    pub workspace: String,
    pub attachments: Vec<WorkspaceAttachment>,
    pub revision: i64,
    pub bound_revision: i64,
    pub binding_status: String,
    pub binding_error: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceGrantRequest {
    pub id: String,
    pub session_id: String,
    pub path: String,
    pub access: WorkspaceAccessMode,
    pub reason: String,
    pub status: String,
    pub created_at: String,
    pub resolved_at: Option<String>,
}

pub fn canonical_directory(raw: &str) -> Result<PathBuf> {
    let path = Path::new(raw);
    if !path.is_absolute() {
        return Err(anyhow!("workspace paths must be absolute"));
    }
    let canonical = path
        .canonicalize()
        .with_context(|| format!("workspace folder does not exist: {raw}"))?;
    if !canonical.is_dir() {
        return Err(anyhow!("workspace path is not a directory"));
    }
    Ok(canonical)
}

fn within(path: &Path, root: &Path) -> bool {
    path == root || path.starts_with(root)
}

fn validate_supported_access(access: WorkspaceAccessMode) -> Result<()> {
    if access == WorkspaceAccessMode::ReadOnly {
        return Err(anyhow!(
            "read-only attachments are unsupported until the sandbox can enforce them"
        ));
    }
    Ok(())
}

pub async fn get(
    db: &Arc<Database>,
    session_id: &str,
) -> Result<Option<ConversationWorkspaceScope>> {
    let id = session_id.to_owned();
    db.run_transaction(move |conn| {
        if load(conn, &id)?.is_none() {
            let _ = initialize_from_session(conn, &id)?;
        }
        load(conn, &id)
    })
    .await
}

/// Lazily migrates conversations created before workspace scopes existed.
/// Session metadata remains the durable source for their original cwd.
fn initialize_from_session(conn: &rusqlite::Connection, session_id: &str) -> Result<bool> {
    let values = conn
        .query_row(
            "SELECT metadata_json, target_json FROM sessions WHERE id=?1",
            [session_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?;
    let Some((metadata, target)) = values else {
        return Ok(false);
    };
    let metadata: serde_json::Value = serde_json::from_str(&metadata).unwrap_or_default();
    let target: serde_json::Value = serde_json::from_str(&target).unwrap_or_default();
    let workspace = metadata
        .get("workspace")
        .and_then(serde_json::Value::as_str)
        .or_else(|| target.get("workspace").and_then(serde_json::Value::as_str));
    let Some(workspace) = workspace else {
        return Ok(false);
    };
    initialize(conn, session_id, workspace)?;
    // This scope describes the already-running known-good generation.
    conn.execute("UPDATE conversation_workspace_scopes SET bound_revision=revision,binding_status='active' WHERE session_id=?1", [session_id])?;
    Ok(true)
}

pub async fn provision(
    db: &Arc<Database>,
    session_id: &str,
    workspace: &str,
) -> Result<ConversationWorkspaceScope> {
    let id = session_id.to_owned();
    let workspace = canonical_directory(workspace)?
        .to_string_lossy()
        .into_owned();
    db.run_transaction(move |conn| {
        initialize(conn, &id, &workspace)?;
        load(conn, &id)?.ok_or_else(|| anyhow!("scope was not created"))
    })
    .await
}

pub async fn mark_bound(db: &Arc<Database>, session_id: &str) -> Result<()> {
    let id = session_id.to_owned();
    db.run(move|conn|{conn.execute("UPDATE conversation_workspace_scopes SET bound_revision=revision,binding_status='active',binding_error=NULL,updated_at=datetime('now') WHERE session_id=?1",[id])?;Ok(())}).await
}

pub fn writable_roots(scope: &ConversationWorkspaceScope) -> Vec<String> {
    scope
        .attachments
        .iter()
        .filter(|item| item.access == WorkspaceAccessMode::ReadWrite)
        .map(|item| item.path.clone())
        .collect()
}

fn load(
    conn: &rusqlite::Connection,
    session_id: &str,
) -> Result<Option<ConversationWorkspaceScope>> {
    let row = conn.query_row("SELECT workspace, revision, bound_revision, binding_status, binding_error FROM conversation_workspace_scopes WHERE session_id=?1", [session_id], |r| Ok((r.get::<_,String>(0)?, r.get::<_,i64>(1)?, r.get::<_,i64>(2)?, r.get::<_,String>(3)?, r.get::<_,Option<String>>(4)?))).optional()?;
    let Some((workspace, revision, bound_revision, binding_status, binding_error)) = row else {
        return Ok(None);
    };
    let mut stmt = conn.prepare("SELECT path, access, source, created_at FROM workspace_attachments WHERE session_id=?1 ORDER BY created_at, path")?;
    let attachments = stmt
        .query_map([session_id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?
        .map(|row| {
            let (path, access, source, created_at) = row?;
            Ok(WorkspaceAttachment {
                path,
                access: WorkspaceAccessMode::parse(&access)?,
                source: match source.as_str() {
                    "agent_request" => AttachmentSource::AgentRequest,
                    "migrated_default" => AttachmentSource::MigratedDefault,
                    "recent_folder" => AttachmentSource::RecentFolder,
                    _ => AttachmentSource::UserPicker,
                },
                created_at,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(ConversationWorkspaceScope {
        session_id: session_id.into(),
        workspace,
        attachments,
        revision,
        bound_revision,
        binding_status,
        binding_error,
    }))
}

fn reject_overlap(conn: &rusqlite::Connection, session_id: &str, candidate: &Path) -> Result<()> {
    let workspace: String = conn
        .query_row(
            "SELECT workspace FROM conversation_workspace_scopes WHERE session_id=?1",
            [session_id],
            |row| row.get(0),
        )
        .context("conversation workspace scope is not initialized")?;
    let workspace = PathBuf::from(workspace);
    if within(candidate, &workspace) || within(&workspace, candidate) {
        return Err(anyhow!("attachment overlaps the working workspace"));
    }
    let mut stmt = conn.prepare("SELECT path FROM workspace_attachments WHERE session_id=?1")?;
    let paths = stmt
        .query_map([session_id], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if paths
        .into_iter()
        .map(PathBuf::from)
        .any(|path| within(candidate, &path) || within(&path, candidate))
    {
        return Err(anyhow!("attachment overlaps an existing attachment"));
    }
    Ok(())
}

pub async fn attach(
    db: &Arc<Database>,
    session_id: &str,
    raw: &str,
    access: WorkspaceAccessMode,
    source: AttachmentSource,
) -> Result<ConversationWorkspaceScope> {
    let path = canonical_directory(raw)?;
    // The native picker is the explicit user grant. Requiring this path to
    // already be in the default policy would make one-conversation grants
    // impossible and turn Add folder into a circular workflow.
    validate_supported_access(access)?;
    let id = session_id.to_owned();
    let path = path.to_string_lossy().into_owned();
    db.run_transaction(move |conn| {
        if load(conn, &id)?.is_none() && !initialize_from_session(conn, &id)? {
            return Err(anyhow!("conversation has no durable workspace metadata"));
        }
        reject_overlap(conn, &id, Path::new(&path))?;
        conn.execute("INSERT INTO workspace_attachments(session_id,path,access,source,created_at) VALUES(?1,?2,?3,?4,datetime('now'))", params![id,path,access.as_str(),source.as_str()]).context("attachment is already present")?;
        conn.execute("UPDATE conversation_workspace_scopes SET revision=revision+1,binding_status='pending',binding_error=NULL,updated_at=datetime('now') WHERE session_id=?1", [&id])?;
        load(conn,&id)?.ok_or_else(|| anyhow!("scope disappeared"))
    }).await
}

pub async fn recent_folders(db: &Arc<Database>) -> Result<Vec<String>> {
    db.run(|conn| {
        let mut stmt = conn.prepare(
            "SELECT path FROM workspace_attachments GROUP BY path ORDER BY MAX(created_at) DESC, path LIMIT 5",
        )?;
        let paths = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(paths)
    })
    .await
}

/// Attaches a folder selected from the native app's recent-folder list. The
/// history lookup is the trust boundary: callers cannot use this endpoint to
/// grant an arbitrary path that the user has never selected before.
pub async fn attach_recent(
    db: &Arc<Database>,
    session_id: &str,
    raw: &str,
) -> Result<ConversationWorkspaceScope> {
    let canonical = canonical_directory(raw)?.to_string_lossy().into_owned();
    if !recent_folders(db).await?.iter().any(|path| path == &canonical) {
        return Err(anyhow!("folder is not in recent workspace history"));
    }
    attach(
        db,
        session_id,
        &canonical,
        WorkspaceAccessMode::ReadWrite,
        AttachmentSource::RecentFolder,
    )
    .await
}

pub async fn remove_attachment(
    db: &Arc<Database>,
    session_id: &str,
    raw: &str,
) -> Result<ConversationWorkspaceScope> {
    let path = canonical_directory(raw)?.to_string_lossy().into_owned();
    let id = session_id.to_owned();
    db.run_transaction(move |conn| { let changed=conn.execute("DELETE FROM workspace_attachments WHERE session_id=?1 AND path=?2", params![id,path])?; if changed==0{return Err(anyhow!("attachment was not found"));} conn.execute("UPDATE conversation_workspace_scopes SET revision=revision+1,binding_status='pending',binding_error=NULL,updated_at=datetime('now') WHERE session_id=?1",[&id])?; load(conn,&id)?.ok_or_else(||anyhow!("scope disappeared")) }).await
}

pub async fn request_grant(
    db: &Arc<Database>,
    session_id: &str,
    raw: &str,
    access: WorkspaceAccessMode,
    reason: &str,
) -> Result<WorkspaceGrantRequest> {
    let path = canonical_directory(raw)?;
    // This only creates a pending record. Scope is unchanged until approval
    // reconfirms the exact path through the native picker.
    validate_supported_access(access)?;
    let reason = reason.trim();
    if reason.is_empty() {
        return Err(anyhow!("an access request must include a reason"));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let session_id = session_id.to_owned();
    let path = path.to_string_lossy().into_owned();
    let reason = reason.to_owned();
    db.run_transaction(move |conn| { conn.execute("INSERT INTO workspace_grant_requests(id,session_id,path,access,reason,status,created_at) VALUES(?1,?2,?3,?4,?5,'pending',datetime('now'))",params![id,session_id,path,access.as_str(),reason])?; grant(conn,&id)?.ok_or_else(||anyhow!("grant request was not created")) }).await
}

pub async fn list_grants(
    db: &Arc<Database>,
    session_id: &str,
) -> Result<Vec<WorkspaceGrantRequest>> {
    let id = session_id.to_owned();
    db.run(move |conn| {
        let mut stmt = conn.prepare("SELECT id,session_id,path,access,reason,status,created_at,resolved_at FROM workspace_grant_requests WHERE session_id=?1 ORDER BY created_at DESC")?;
        let rows = stmt
            .query_map([id], grant_row)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }).await
}

fn grant_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<WorkspaceGrantRequest> {
    let access: String = row.get(3)?;
    Ok(WorkspaceGrantRequest {
        id: row.get(0)?,
        session_id: row.get(1)?,
        path: row.get(2)?,
        access: if access == "read_write" {
            WorkspaceAccessMode::ReadWrite
        } else {
            WorkspaceAccessMode::ReadOnly
        },
        reason: row.get(4)?,
        status: row.get(5)?,
        created_at: row.get(6)?,
        resolved_at: row.get(7)?,
    })
}
fn grant(conn: &rusqlite::Connection, id: &str) -> Result<Option<WorkspaceGrantRequest>> {
    Ok(conn.query_row("SELECT id,session_id,path,access,reason,status,created_at,resolved_at FROM workspace_grant_requests WHERE id=?1",[id],grant_row).optional()?)
}

pub async fn deny_grant(db: &Arc<Database>, request_id: &str) -> Result<WorkspaceGrantRequest> {
    let id = request_id.to_owned();
    db.run_transaction(move|conn|{let changed=conn.execute("UPDATE workspace_grant_requests SET status='denied',resolved_at=datetime('now') WHERE id=?1 AND status='pending'",[&id])?;if changed==0{return Err(anyhow!("pending access request was not found"));}grant(conn,&id)?.ok_or_else(||anyhow!("access request disappeared"))}).await
}

pub async fn approve_grant(
    db: &Arc<Database>,
    request_id: &str,
    confirmed_path: &str,
) -> Result<ConversationWorkspaceScope> {
    let confirmed = canonical_directory(confirmed_path)?
        .to_string_lossy()
        .into_owned();
    let id = request_id.to_owned();
    db.run_transaction(move|conn|{let request=grant(conn,&id)?.ok_or_else(||anyhow!("access request was not found"))?;if request.status!="pending"{return Err(anyhow!("access request is no longer pending"));}if request.path!=confirmed{return Err(anyhow!("selected folder does not match the requested folder"));}reject_overlap(conn,&request.session_id,Path::new(&request.path))?;conn.execute("INSERT INTO workspace_attachments(session_id,path,access,source,created_at) VALUES(?1,?2,?3,'agent_request',datetime('now'))",params![request.session_id,request.path,request.access.as_str()])?;conn.execute("UPDATE workspace_grant_requests SET status='approved',resolved_at=datetime('now') WHERE id=?1",[&id])?;conn.execute("UPDATE conversation_workspace_scopes SET revision=revision+1,binding_status='pending',binding_error=NULL,updated_at=datetime('now') WHERE session_id=?1",[&request.session_id])?;load(conn,&request.session_id)?.ok_or_else(||anyhow!("scope disappeared"))}).await
}

pub fn initialize(conn: &rusqlite::Connection, session_id: &str, workspace: &str) -> Result<()> {
    let workspace = canonical_directory(workspace)?
        .to_string_lossy()
        .into_owned();
    conn.execute("INSERT OR IGNORE INTO conversation_workspace_scopes(session_id,workspace,created_at,updated_at) VALUES(?1,?2,datetime('now'),datetime('now'))", params![session_id,workspace])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonicalization_rejects_relative_missing_and_files() {
        assert!(canonical_directory("relative").is_err());
        assert!(canonical_directory("/definitely/not/here").is_err());
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("file");
        std::fs::write(&file, "x").unwrap();
        assert!(canonical_directory(file.to_str().unwrap()).is_err());
        assert!(canonical_directory(temp.path().to_str().unwrap()).is_ok());
    }

    #[tokio::test]
    async fn existing_session_scope_is_initialized_from_durable_metadata() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let storage = crate::storage::Storage::open(root.path()).unwrap();
        storage.database().with_conn(|conn| {
            conn.execute(
                "INSERT INTO sessions(id,title,target_json,status,metadata_json,created_at,updated_at) VALUES('legacy','Legacy','{}','ready',?1,'now','now')",
                [serde_json::json!({"workspace": workspace.path()}).to_string()],
            )?;
            Ok(())
        }).unwrap();

        let scope = get(storage.database(), "legacy").await.unwrap().unwrap();
        assert_eq!(
            scope.workspace,
            workspace.path().canonicalize().unwrap().to_string_lossy()
        );
        assert_eq!(scope.binding_status, "active");
        assert_eq!(scope.bound_revision, scope.revision);
    }

    #[tokio::test]
    async fn recent_folders_returns_only_the_five_most_recent_unique_paths() {
        let root = tempfile::tempdir().unwrap();
        let storage = crate::storage::Storage::open(root.path()).unwrap();
        storage.database().with_conn(|conn| {
            for index in 0..6 {
                let session = format!("recent-{index}");
                let path = format!("/tmp/recent-{index}");
                conn.execute(
                    "INSERT INTO sessions(id,title,target_json,status,metadata_json,created_at,updated_at) VALUES(?1,?1,'{}','ready','{}','now','now')",
                    [&session],
                )?;
                conn.execute(
                    "INSERT INTO workspace_attachments(session_id,path,access,source,created_at) VALUES(?1,?2,'read_write','user_picker',?3)",
                    params![session, path, format!("2026-01-0{}", index + 1)],
                )?;
            }
            Ok(())
        }).unwrap();

        let recent = recent_folders(storage.database()).await.unwrap();
        assert_eq!(recent.len(), 5);
        assert_eq!(recent[0], "/tmp/recent-5");
        assert!(!recent.contains(&"/tmp/recent-0".to_owned()));
    }
}
