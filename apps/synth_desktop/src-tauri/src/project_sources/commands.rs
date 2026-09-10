//! Human admission is performed by a native folder picker, not an agent path.
use super::requests::{self, ProjectSourceRequest, ProjectSourceRequestInput};
use super::{
    admit_picked_root, approve, catalog, remove_root, ProjectSourceApproval, ProjectSourceCatalog,
};
use crate::core_runtime::CoreRuntime;
use crate::error::AppError;
use crate::session::codex::CodexManager;
use std::sync::Arc;
use tauri::State;
use tauri_plugin_dialog::DialogExt;

#[tauri::command]
#[specta::specta]
pub fn project_sources_get() -> Result<ProjectSourceCatalog, AppError> {
    catalog().map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub fn project_sources_refresh() -> Result<ProjectSourceCatalog, AppError> {
    // Declarations and grants are read on every discovery; there is no stale
    // secondary catalog to populate or mistake for current execution authority.
    catalog().map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn project_source_add(
    app: tauri::AppHandle,
    core: State<'_, Arc<CoreRuntime>>,
    containers: bool,
    recipes: bool,
) -> Result<Option<ProjectSourceCatalog>, AppError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Choose an executable project source")
        .pick_folder(move |path| {
            let _ = sender.send(path.map(|value| value.to_string()));
        });
    let Some(path) = receiver.await.map_err(AppError::from)? else {
        return Ok(None);
    };
    admit_picked_root(core.storage().database(), &path, containers, recipes)
        .await
        .map(Some)
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn project_source_remove(
    core: State<'_, Arc<CoreRuntime>>,
    path: String,
) -> Result<ProjectSourceCatalog, AppError> {
    remove_root(core.storage().database(), &path)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn project_source_request(
    core: State<'_, Arc<CoreRuntime>>,
    request: ProjectSourceRequestInput,
) -> Result<ProjectSourceRequest, AppError> {
    requests::request(core.storage().database(), request)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn project_source_requests_list(
    core: State<'_, Arc<CoreRuntime>>,
    session_id: Option<String>,
) -> Result<Vec<ProjectSourceRequest>, AppError> {
    requests::list(core.storage().database(), session_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn project_source_deny(
    core: State<'_, Arc<CoreRuntime>>,
    request_id: String,
) -> Result<ProjectSourceRequest, AppError> {
    requests::deny(core.storage().database(), &request_id)
        .await
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub async fn project_source_approve(
    app: tauri::AppHandle,
    core: State<'_, Arc<CoreRuntime>>,
    codex: State<'_, Arc<CodexManager>>,
    request_id: String,
) -> Result<Option<ProjectSourceApproval>, AppError> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Confirm the exact requested project folder")
        .pick_folder(move |path| {
            let _ = sender.send(path.map(|value| value.to_string()));
        });
    let Some(path) = receiver.await.map_err(AppError::from)? else {
        return Ok(None);
    };
    let mut approval = approve(core.storage().database(), &request_id, &path)
        .await
        .map_err(AppError::from)?;
    if let Some(scope) = &approval.scope {
        if let Err(error) = codex.fence_attachment(&scope.session_id).await {
            approval.attachment_error = Some(format!("Source approved and folder attached, but the active agent could not be fenced: {error}"));
        }
    }
    Ok(Some(approval))
}
