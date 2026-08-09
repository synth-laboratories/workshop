mod codex;
mod laguna;
mod runtime;
mod terminal;

use codex::{
    CodexManager, CodexSessionInfo, CodexSessionRecord, CodexSessionRequest,
    CodexSessionStartRequest, CodexTurnStartRequest,
};
use laguna::{LagunaManager, LagunaStatus};
use runtime::{RuntimeManager, RuntimeRequest, RuntimeSubscribeRequest};
use std::sync::Arc;
use tauri::{Emitter, Manager, State};
use tauri_plugin_dialog::DialogExt;
use terminal::{TerminalCreateRequest, TerminalEvent, TerminalInfo, TerminalManager};

#[tauri::command]
async fn runtime_request(
    state: State<'_, Arc<RuntimeManager>>,
    request: RuntimeRequest,
) -> Result<serde_json::Value, String> {
    state
        .request(request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn runtime_subscribe(
    app: tauri::AppHandle,
    state: State<'_, Arc<RuntimeManager>>,
    request: RuntimeSubscribeRequest,
) -> Result<serde_json::Value, String> {
    state
        .subscribe(app, request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn runtime_unsubscribe(
    state: State<'_, Arc<RuntimeManager>>,
    subscription_id: String,
) -> Result<(), String> {
    state.unsubscribe(&subscription_id).await;
    Ok(())
}

#[tauri::command]
async fn laguna_get_status(state: State<'_, Arc<LagunaManager>>) -> Result<LagunaStatus, String> {
    Ok(state.status().await)
}

#[tauri::command]
async fn project_choose_directory(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_title("Choose a project folder")
        .pick_folder(move |path| {
            let _ = sender.send(path.map(|value| value.to_string()));
        });
    receiver.await.map_err(|error| error.to_string())
}

#[tauri::command]
async fn codex_session_start(
    app: tauri::AppHandle,
    state: State<'_, Arc<CodexManager>>,
    laguna: State<'_, Arc<LagunaManager>>,
    mut request: CodexSessionStartRequest,
) -> Result<CodexSessionInfo, String> {
    if request.provider_name.as_deref() == Some("local-laguna") {
        let root = runtime::workshop_root().map_err(|error| error.to_string())?;
        request.base_url = laguna
            .ensure(&root)
            .await
            .map_err(|error| error.to_string())?
            .ok_or_else(|| "Laguna Responses server is unavailable".to_string())?;
        request.api_key = laguna.api_key().unwrap_or_default();
    }
    state
        .start(app, request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn codex_turn_start(
    state: State<'_, Arc<CodexManager>>,
    request: CodexTurnStartRequest,
) -> Result<CodexSessionInfo, String> {
    state
        .start_turn(request)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn codex_turn_interrupt(
    state: State<'_, Arc<CodexManager>>,
    request: CodexSessionRequest,
) -> Result<(), String> {
    state
        .interrupt(&request.session_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn codex_session_close(
    state: State<'_, Arc<CodexManager>>,
    request: CodexSessionRequest,
) -> Result<(), String> {
    state
        .close(&request.session_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn codex_sessions_list(
    state: State<'_, Arc<CodexManager>>,
) -> Result<Vec<CodexSessionRecord>, String> {
    Ok(state.list().await)
}

#[tauri::command]
fn codex_default_workspace() -> Result<String, String> {
    let path = std::env::var_os("SYNTH_DESKTOP_WORKSPACE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_default()
                .join(".synth-desktop/workspaces/default")
        });
    std::fs::create_dir_all(&path)
        .map_err(|error| format!("Cannot create the default workspace: {error}"))?;
    let path = path
        .canonicalize()
        .map_err(|error| format!("Default workspace is unavailable: {error}"))?;
    if !path.is_dir() {
        return Err("Default workspace must be a directory".into());
    }
    Ok(path.to_string_lossy().into_owned())
}

#[tauri::command]
fn terminal_create(
    app: tauri::AppHandle,
    state: State<'_, Arc<TerminalManager>>,
    request: TerminalCreateRequest,
) -> Result<TerminalInfo, String> {
    state
        .create(app, request)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn terminal_list(
    state: State<'_, Arc<TerminalManager>>,
    workspace_id: Option<String>,
) -> Vec<TerminalInfo> {
    state.list(workspace_id.as_deref())
}

#[tauri::command]
fn terminal_snapshot(
    state: State<'_, Arc<TerminalManager>>,
    terminal_id: String,
    after_sequence: Option<u64>,
) -> Result<Vec<TerminalEvent>, String> {
    state
        .snapshot(&terminal_id, after_sequence.unwrap_or(0))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn terminal_write(
    state: State<'_, Arc<TerminalManager>>,
    terminal_id: String,
    data: String,
) -> Result<(), String> {
    state
        .write(&terminal_id, &data)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn terminal_resize(
    state: State<'_, Arc<TerminalManager>>,
    terminal_id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    state
        .resize(&terminal_id, cols, rows)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn terminal_close(
    state: State<'_, Arc<TerminalManager>>,
    terminal_id: String,
) -> Result<(), String> {
    state.close(&terminal_id).map_err(|error| error.to_string())
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let laguna = Arc::new(LagunaManager::new());
            let runtime = Arc::new(RuntimeManager::new(laguna.clone()));
            app.manage(Arc::new(CodexManager::new()));
            app.manage(Arc::new(TerminalManager::new()));
            app.manage(laguna.clone());
            app.manage(runtime.clone());

            let mut status_updates = laguna.subscribe();
            let status_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Ok(status) = status_updates.recv().await {
                    let _ = status_handle.emit("laguna:status", status);
                }
            });

            if let Some(window) = app.get_webview_window("main") {
                window.show()?;
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            runtime_request,
            runtime_subscribe,
            runtime_unsubscribe,
            laguna_get_status,
            project_choose_directory,
            codex_session_start,
            codex_turn_start,
            codex_turn_interrupt,
            codex_session_close,
            codex_sessions_list,
            codex_default_workspace,
            terminal_create,
            terminal_list,
            terminal_snapshot,
            terminal_write,
            terminal_resize,
            terminal_close
        ])
        .run(tauri::generate_context!())
        .expect("error while running Synth Desktop");
}
