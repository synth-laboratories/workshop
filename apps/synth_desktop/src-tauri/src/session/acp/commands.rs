//! Thin desktop adapter, also projected into MCP by the command contract.
use super::{config::Backend, Manager, StartRequest};
use crate::contract::specta::OpaqueJson;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
#[specta::specta]
pub fn agent_backends_list(state: State<'_, Arc<Manager>>) -> Result<Vec<Backend>, String> {
    state.backends().map_err(|error| error.to_string())
}
#[tauri::command]
#[specta::specta]
pub async fn agent_session_start(
    state: State<'_, Arc<Manager>>,
    request: StartRequest,
) -> Result<OpaqueJson, String> {
    state
        .inner()
        .start(request)
        .await
        .map(OpaqueJson)
        .map_err(|error| error.to_string())
}
#[tauri::command]
#[specta::specta]
pub async fn agent_session_send(
    state: State<'_, Arc<Manager>>,
    session_id: String,
    text: String,
) -> Result<OpaqueJson, String> {
    state
        .inner()
        .send(session_id, text)
        .await
        .map(OpaqueJson)
        .map_err(|error| error.to_string())
}
#[tauri::command]
#[specta::specta]
pub async fn agent_sessions_list(state: State<'_, Arc<Manager>>) -> Result<OpaqueJson, String> {
    state
        .list()
        .await
        .map(OpaqueJson)
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn agent_session_cancel(
    state: State<'_, Arc<Manager>>,
    session_id: String,
) -> Result<OpaqueJson, String> {
    state
        .inner()
        .cancel(&session_id)
        .await
        .map(|_| {
            OpaqueJson(
                serde_json::json!({"sessionId":session_id,"action":"cancel","accepted":true}),
            )
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn agent_session_close(
    state: State<'_, Arc<Manager>>,
    session_id: String,
) -> Result<OpaqueJson, String> {
    state
        .inner()
        .close(&session_id)
        .await
        .map(|_| {
            OpaqueJson(serde_json::json!({"sessionId":session_id,"action":"close","accepted":true}))
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
#[specta::specta]
pub async fn agent_session_resume(
    state: State<'_, Arc<Manager>>,
    session_id: String,
) -> Result<OpaqueJson, String> {
    state
        .inner()
        .resume(&session_id)
        .await
        .map(OpaqueJson)
        .map_err(|error| error.to_string())
}
