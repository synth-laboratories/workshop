//! Tauri command edge for workspace documents.
//!
//! Three commands and no fourth. There is deliberately no write, no delete, and
//! no "read arbitrary path": the pane's whole job is to display bytes the
//! conversation is already allowed to see, and a viewer that could also write
//! would need the approval machinery the agent path already owns.
//!
//! Every command takes `session_id` because the scope that authorizes the read
//! belongs to the conversation, not to the window. Passing the window's current
//! session implicitly would make the grant ambient, which is the property
//! `workspace_scope` exists to remove.
//!
//! # Registration
//!
//! These are not reachable until they are added to `collect_commands!` in
//! `contract/specta.rs` and named in `contract/commands.rs`. See the handoff
//! note beside this module for the exact lines.

use std::sync::Arc;
use tauri::State;

use super::{DocumentShown, WorkspaceDirectory, WorkspaceDocument};
use crate::core_runtime::CoreRuntime;
use crate::error::AppError;

async fn blocking<T, F>(work: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce() -> anyhow::Result<T> + Send + 'static,
{
    // Reading up to 2 MiB and stat-ing a thousand directory entries is real
    // blocking I/O; it does not belong on a runtime worker that a live stream
    // is also using.
    match tokio::task::spawn_blocking(work).await {
        Ok(result) => result.map_err(AppError::from),
        Err(error) => Err(AppError::internal(error)),
    }
}

/// Read one workspace document for display.
///
/// Refuses with `document_outside_workspace` for a path outside every session
/// root, and with `document_unavailable` plus the named reason for a path that
/// is in scope but cannot be typeset. Neither is an empty string.
#[tauri::command]
#[specta::specta]
pub async fn workspace_read_file(
    core: State<'_, Arc<CoreRuntime>>,
    session_id: String,
    path: String,
) -> Result<WorkspaceDocument, AppError> {
    let core = core.inner().clone();
    blocking(move || super::read(core.storage().database(), &session_id, &path)).await
}

/// List one workspace directory — the breadcrumb's and the file picker's data.
///
/// Rows that cannot be opened are listed with the reason rather than filtered
/// out, so a folder of binaries reads as a folder of binaries and not as empty.
#[tauri::command]
#[specta::specta]
pub async fn workspace_list_dir(
    core: State<'_, Arc<CoreRuntime>>,
    session_id: String,
    path: String,
) -> Result<WorkspaceDirectory, AppError> {
    let core = core.inner().clone();
    blocking(move || super::list_dir(core.storage().database(), &session_id, &path)).await
}

/// Open one workspace document in the right panel.
///
/// The same rail a visual takes: resolve or create the deterministic pane
/// record, emit the durable `visual.show` event, and let the panel's existing
/// listener open it. The renderer does not open the pane itself, so a document
/// the agent shows and a document the reader clicks arrive by one path.
#[tauri::command]
#[specta::specta]
pub async fn document_show(
    core: State<'_, Arc<CoreRuntime>>,
    session_id: String,
    path: String,
) -> Result<DocumentShown, AppError> {
    let core = core.inner().clone();
    super::show(&core, &session_id, &path)
        .await
        .map_err(AppError::from)
}
