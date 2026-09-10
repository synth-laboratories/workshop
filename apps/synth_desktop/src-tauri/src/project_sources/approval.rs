use super::{
    canonical_project_root, catalog, compensate, inspection, requests, ProjectSourceCatalog,
    RootOrigin,
};
use crate::{
    storage::{append_event, Database, EventAppend},
    synth_config, workspace_scope,
};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::sync::Arc;

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSourceApproval {
    pub request: requests::ProjectSourceRequest,
    pub source: inspection::ProjectSourceRow,
    pub catalog: ProjectSourceCatalog,
    pub scope: Option<workspace_scope::ConversationWorkspaceScope>,
    pub attachment_error: Option<String>,
}

pub async fn approve(db: &Arc<Database>, id: &str, picked: &str) -> Result<ProjectSourceApproval> {
    let request = approve_core(db, id, picked, synth_config::begin_project_source_grant).await?;
    let (scope, attachment_error) = if request.attach_to_conversation {
        match workspace_scope::attach(
            db,
            request
                .session_id
                .as_deref()
                .context("approved attachment has no session")?,
            &request.canonical_path,
            workspace_scope::WorkspaceAccessMode::ReadWrite,
            workspace_scope::AttachmentSource::AgentRequest,
        )
        .await
        {
            Ok(scope) => (Some(scope), None),
            Err(error) => (
                None,
                Some(format!(
                    "Source approved, but conversation attachment failed: {error}"
                )),
            ),
        }
    } else {
        (None, None)
    };
    let catalog = catalog().context("source approved, but its catalog could not be refreshed")?;
    let source = catalog
        .sources
        .iter()
        .find(|row| row.path == request.canonical_path)
        .cloned()
        .unwrap_or_else(|| inspection::ProjectSourceRow {
            path: request.canonical_path.clone(),
            containers: request.containers,
            recipes: request.recipes,
            origin: RootOrigin::Configured,
            inspection: inspection::inspect(std::path::Path::new(&request.canonical_path)),
            last_scanned_at: None,
        });
    Ok(ProjectSourceApproval {
        request,
        source,
        catalog,
        scope,
        attachment_error,
    })
}

async fn approve_core<F>(
    db: &Arc<Database>,
    id: &str,
    picked: &str,
    grant: F,
) -> Result<requests::ProjectSourceRequest>
where
    F: FnOnce(synth_config::ProjectSourceEntry) -> Result<synth_config::ProjectSourceChange> + Send,
{
    let _resolution = requests::RESOLUTION.lock().await;
    let canonical = canonical_project_root(picked)?;
    let id = id.to_owned();
    let request = db.run(move |conn| requests::load(conn, &id)).await?;
    if request.status != "pending" {
        bail!("project source request is no longer pending");
    }
    if canonical.to_string_lossy() != request.canonical_path {
        bail!("selected folder does not match the exact requested folder");
    }
    let inspection = inspection::inspect(&canonical);
    if inspection.status != "valid" {
        bail!(
            "{}",
            inspection
                .message
                .as_deref()
                .unwrap_or("invalid source declaration")
        );
    }
    let change = grant(synth_config::ProjectSourceEntry {
        path: request.canonical_path.clone(),
        containers: request.containers,
        recipes: request.recipes,
    })?;
    let result = db.run_transaction(move |conn| {
        let changed = conn.execute("UPDATE project_source_requests SET status='approved',resolved_at=datetime('now') WHERE id=?1 AND status='pending'", [&request.id])?;
        if changed != 1 { bail!("pending source request could not be settled"); }
        let request = requests::load(conn, &request.id)?;
        append_event(conn, EventAppend::system("project_source.approved", serde_json::json!({
            "requestId": request.id, "path": request.canonical_path, "sessionId": request.session_id,
            "containers": inspection.containers, "recipes": inspection.recipes,
            "grant": { "containers": request.containers, "recipes": request.recipes }, "method": "requested_native_picker"
        })))?;
        Ok(request)
    }).await;
    result.map_err(|error| compensate(change, error))
}

