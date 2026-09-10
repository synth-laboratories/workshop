//! Human admission is performed by a native folder picker, not an agent path.
use super::{admit_picked_root, catalog, remove_root, ProjectSourceCatalog};
use crate::error::AppError;
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
    admit_picked_root(&path, containers, recipes)
        .map(Some)
        .map_err(AppError::from)
}

#[tauri::command]
#[specta::specta]
pub fn project_source_remove(path: String) -> Result<ProjectSourceCatalog, AppError> {
    remove_root(&path).map_err(AppError::from)
}
