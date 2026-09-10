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

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::Path};

    fn recipe(root: &Path) {
        fs::write(root.join("workshop.recipe.toml"), "id='eval.source.v1'\nalgorithm='eval'\ncontainer='fixture'\nprovider='openrouter'\nmodel='openai/gpt-4.1-nano'\nlocality='container'\ntrain_seeds=[0]\n[bounds]\nmax_cost_usd=0.5\nmax_total_rollouts=10\n").unwrap();
    }
    async fn pending(db: &Arc<Database>, root: &Path) -> requests::ProjectSourceRequest {
        requests::request(
            db,
            requests::ProjectSourceRequestInput {
                session_id: None,
                path: root.display().to_string(),
                reason: "Use this declared recipe".into(),
                containers: false,
                recipes: true,
                attach_to_conversation: false,
            },
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn approval_requires_exact_picker_and_pending_request() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("source");
        fs::create_dir(&root).unwrap();
        recipe(&root);
        let config = dir.path().join("config.toml");
        let db = Arc::new(Database::open(dir.path().join("state.sqlite3")).unwrap());
        let request = pending(&db, &root).await;
        let grant = |entry| synth_config::begin_project_source_grant_at(&config, entry);
        assert!(
            approve_core(&db, &request.id, &dir.path().display().to_string(), grant)
                .await
                .is_err()
        );
        assert!(!config.exists());
        let approved = approve_core(&db, &request.id, &root.display().to_string(), grant)
            .await
            .unwrap();
        assert_eq!(approved.status, "approved");
        assert!(approved.resolved_at.is_some());
        let before = fs::read_to_string(&config).unwrap();
        assert!(before.contains("recipes = true"));
        assert!(
            approve_core(&db, &request.id, &root.display().to_string(), grant)
                .await
                .is_err()
        );
        assert_eq!(fs::read_to_string(&config).unwrap(), before);
    }

    #[tokio::test]
    async fn approval_reinspects_declarations_before_granting() {
        let dir = tempfile::tempdir().unwrap();
        recipe(dir.path());
        let db = Arc::new(Database::open(dir.path().join("state.sqlite3")).unwrap());
        let request = pending(&db, dir.path()).await;
        fs::write(
            dir.path().join("workshop.recipe.toml"),
            "invalid changed declaration [",
        )
        .unwrap();
        assert!(approve_core(
            &db,
            &request.id,
            &dir.path().display().to_string(),
            |_| panic!("invalid declaration must never grant access")
        )
        .await
        .is_err());
        assert_eq!(
            requests::list(&db, None).await.unwrap()[0].status,
            "pending"
        );
    }

    #[tokio::test]
    async fn failed_settlement_restores_previous_capabilities_and_preserves_other_roots() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("source");
        fs::create_dir(&root).unwrap();
        recipe(&root);
        let root = root.canonicalize().unwrap();
        let config = dir.path().join("config.toml");
        synth_config::begin_project_source_grant_at(
            &config,
            synth_config::ProjectSourceEntry {
                path: root.display().to_string(),
                containers: true,
                recipes: false,
            },
        )
        .unwrap();
        let db = Arc::new(Database::open(dir.path().join("state.sqlite3")).unwrap());
        let request = pending(&db, &root).await;
        db.with_conn(|conn| { conn.execute_batch("CREATE TRIGGER fail_source_audit BEFORE INSERT ON events BEGIN SELECT RAISE(ABORT,'fixture audit error'); END;")?; Ok(()) }).unwrap();
        let result = approve_core(&db, &request.id, &root.display().to_string(), |entry| {
            let change = synth_config::begin_project_source_grant_at(&config, entry)?;
            synth_config::begin_project_source_grant_at(
                &config,
                synth_config::ProjectSourceEntry {
                    path: "/other/source".into(),
                    containers: false,
                    recipes: true,
                },
            )?;
            Ok(change)
        })
        .await;
        assert!(result.is_err());
        assert_eq!(
            requests::list(&db, None).await.unwrap()[0].status,
            "pending"
        );
        let value: toml::Value = fs::read_to_string(&config).unwrap().parse().unwrap();
        let entries = value["desktop"]["project_sources"]["entries"]
            .as_array()
            .unwrap();
        let original = entries
            .iter()
            .find(|entry| entry["path"].as_str() == root.to_str())
            .unwrap();
        assert_eq!(original["containers"].as_bool(), Some(true));
        assert_eq!(original["recipes"].as_bool(), Some(false));
        assert!(entries
            .iter()
            .any(|entry| entry["path"].as_str() == Some("/other/source")));
    }
}
