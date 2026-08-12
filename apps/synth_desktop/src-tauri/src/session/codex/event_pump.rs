//! Stdio JSON-RPC event pump for the Codex app-server child.
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::AtomicU64,
        Arc,
    },
};
use tauri::{AppHandle, Emitter};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::Command,
    sync::{Mutex, RwLock},
};

use crate::domain::{RunStatus, SessionStatus, SessionTitleOrigin};
use crate::session::SessionPersistence;
use crate::storage::{EventAppend, EventSource};

use super::home::persist_records;
use super::proto::{
    default_approval_policy, select_approval_decision, AppServer, CodexEvent, CodexSessionRecord,
    CodexSessionStartRequest, CompactWaiters, Pending, PendingApproval, PendingApprovals, Session,
    EVENT_NAME, STDOUT_CLOSED,
};
use super::telemetry::{
    finalize_performance_tracker, is_context_compaction_notification, track_performance_event,
    PerformanceTrackers,
};

/// Environment variables reaching a Codex child are not private: Codex writes
/// its inherited environment to `CODEX_HOME/shell_snapshots` as plain
/// `export NAME=value`. These names are the ones a real provider credential
/// lives under, so a value under any of them is refused at the spawn boundary —
/// a `credential_broker` lease is the only thing that may cross it.
pub(crate) const CREDENTIAL_ENV_NAMES: &[&str] = &["SYNTH_API_KEY", "OPENROUTER_API_KEY", "OPENAI_API_KEY"];

/// The single provider variable the Codex child is allowed to receive, if any.
///
/// Refusing a real credential name is an error, not an omission: launching the
/// child without the variable it was configured to read would surface later as
/// an unauthenticated provider, and the reason would be nowhere in the logs.
pub(crate) fn provider_child_env(
    request: &CodexSessionStartRequest,
) -> Result<Option<(String, String)>> {
    if request.api_key.is_empty() {
        return Ok(None);
    }
    let env_key = request
        .provider_env_key
        .as_deref()
        .unwrap_or("SYNTH_LAGUNA_API_KEY");
    if CREDENTIAL_ENV_NAMES.contains(&env_key) {
        return Err(anyhow!(
            "{env_key} would be written to this session's Codex shell snapshot. \
             Route the provider through the credential broker instead of exporting \
             its key (see credential_broker::apply_brokered_credential)."
        ));
    }
    Ok(Some((env_key.to_owned(), request.api_key.clone())))
}

/// Inputs required to launch the app-server child (stdio transport).
pub(crate) struct SpawnServerRequest<'a> {
    pub binary: &'a Path,
    pub session_id: &'a str,
    pub home: &'a Path,
    pub request: &'a CodexSessionStartRequest,
}

/// Shared pump state cloned into the stdout reader task.
#[derive(Clone)]
pub(crate) struct EventPumpState {
    pub records: Arc<RwLock<HashMap<String, CodexSessionRecord>>>,
    pub state_path: PathBuf,
    pub persistence: SessionPersistence,
    pub sessions: Arc<RwLock<HashMap<String, Arc<Session>>>>,
    pub compact_waiters: CompactWaiters,
    pub pending_compact_sources: Arc<Mutex<HashMap<String, String>>>,
    pub performance_trackers: PerformanceTrackers,
    pub attachment_id: uuid::Uuid,
}

/// Spawn the Codex app-server and attach the stdout event pump.
///
/// Signature kept to five parameters: app handle, spawn request, pump state,
/// plus the two derived from `SpawnServerRequest` are folded into that struct.
pub(crate) async fn spawn_server<R: tauri::Runtime>(
    app: AppHandle<R>,
    spawn: SpawnServerRequest<'_>,
    pump: EventPumpState,
) -> Result<Arc<AppServer>> {
    let SpawnServerRequest {
        binary,
        session_id,
        home,
        request,
    } = spawn;
    let mut command = Command::new(binary);
    command
        .args(["app-server", "--listen", "stdio://"])
        .current_dir(&request.workspace)
        .env("CODEX_HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some((name, value)) = provider_child_env(request)? {
        command.env(name, value);
    }
    let mut child = command.spawn().context("spawn codex app-server")?;
    let stdin = Arc::new(Mutex::new(
        child.stdin.take().context("capture app-server stdin")?,
    ));
    let stdout = child.stdout.take().context("capture app-server stdout")?;
    let stderr = child.stderr.take().context("capture app-server stderr")?;
    let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
    let approvals: PendingApprovals = Arc::new(Mutex::new(HashMap::new()));
    let server = Arc::new(AppServer {
        child: Mutex::new(child),
        stdin: stdin.clone(),
        pending: pending.clone(),
        approvals: approvals.clone(),
        next_id: AtomicU64::new(1),
    });
    let sid = session_id.to_owned();
    let approval_policy = request
        .approval_policy
        .clone()
        .unwrap_or_else(default_approval_policy);
    tauri::async_runtime::spawn(read_stdout(
        app.clone(),
        sid.clone(),
        stdout,
        stdin,
        pending,
        approvals,
        approval_policy,
        pump,
    ));
    tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = app.emit(
                EVENT_NAME,
                CodexEvent {
                    session_id: sid.clone(),
                    method: "app-server/stderr".into(),
                    params: json!({"line":line}),
                },
            );
        }
    });
    Ok(server)
}

async fn read_stdout<R: tauri::Runtime>(
    app: AppHandle<R>,
    session_id: String,
    stdout: tokio::process::ChildStdout,
    stdin: Arc<Mutex<tokio::process::ChildStdin>>,
    pending: Pending,
    approvals: PendingApprovals,
    approval_policy: String,
    persistence: EventPumpState,
) {
    let mut lines = BufReader::new(stdout).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if message.get("method").is_none() {
            if let Some(id) = message.get("id").and_then(Value::as_u64) {
                if let Some(sender) = pending.lock().await.remove(&id) {
                    let response = match message.get("error") {
                        Some(error) => Err(error.to_string()),
                        None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                    };
                    let _ = sender.send(response);
                }
            }
            continue;
        }
        let raw_method = message["method"].as_str().unwrap_or_default();
        let mut params = message.get("params").cloned().unwrap_or(Value::Null);
        // Some Responses-compatible servers report a terminal envelope as
        // `turn/completed` even when the enclosed turn has failed. Preserve
        // the actual outcome: otherwise the transcript says “Worked” while
        // there is no answer to show.
        let method = normalized_turn_method(raw_method, &params).to_owned();
        if is_context_compaction_notification(&method, &params) {
            if let Some(source) = persistence
                .pending_compact_sources
                .lock()
                .await
                .remove(&session_id)
            {
                if let Some(value) = params.as_object_mut() {
                    value.insert("source".into(), Value::String(source));
                }
            }
        }
        if let Some(rpc_id) = message.get("id").cloned() {
            if is_approval_method(&method) {
                let available_decisions = params
                    .get("availableDecisions")
                    .and_then(Value::as_array)
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                if approval_policy == "never" {
                    // “Allow all” is an explicit session policy. A provider
                    // may still ask despite it, so resolve the request rather
                    // than surfacing a contradictory approval card.
                    let response = automatic_approval_response(&available_decisions, rpc_id);
                    let _ = write_message(&stdin, &response).await;
                    continue;
                }
                let approval_id = format!("approval-{}", uuid::Uuid::new_v4().simple());
                approvals.lock().await.insert(
                    approval_id.clone(),
                    PendingApproval {
                        rpc_id,
                        available_decisions: available_decisions.clone(),
                    },
                );
                let safe =
                    safe_approval_payload(&approval_id, &method, &params, &available_decisions);
                let _ = app.emit(
                    EVENT_NAME,
                    CodexEvent {
                        session_id: session_id.clone(),
                        method: "approval.requested".into(),
                        params: safe.clone(),
                    },
                );
                let _ = persistence
                    .persistence
                    .append_and_emit(
                        &app,
                        EventAppend::codex(session_id.clone(), "approval.requested", safe),
                    )
                    .await;
                continue;
            }
            // Unknown server requests are never approved implicitly.
            let _ = write_message(&stdin, &json!({
                "jsonrpc":"2.0","id":rpc_id,
                "error":{"code":-32601,"message":format!("Unsupported server request: {method}")}
            })).await;
            continue;
        }
        let _ = app.emit(
            EVENT_NAME,
            CodexEvent {
                session_id: session_id.clone(),
                method: method.clone(),
                params: params.clone(),
            },
        );
        track_performance_event(
            &persistence.persistence,
            &persistence.performance_trackers,
            &session_id,
            &method,
            &params,
        )
        .await;
        if matches!(method.as_str(), "turn/completed" | "thread/compact/completed") {
            if let Some(waiter) = persistence.compact_waiters.lock().await.remove(&session_id) {
                let _ = waiter.send(Ok(()));
            }
        } else if matches!(method.as_str(), "turn/failed" | "turn/interrupted") {
            if let Some(waiter) = persistence.compact_waiters.lock().await.remove(&session_id) {
                let _ = waiter.send(Err(format!("context compaction ended with {method}")));
            }
        }
        let _ = persistence
            .persistence
            .append_and_emit(
                &app,
                EventAppend::codex(session_id.clone(), method.clone(), params.clone()),
            )
            .await;
        if method == "thread/name/updated" {
            if let Some(title) = params
                .get("threadName")
                .or_else(|| params.get("name"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|title| !title.is_empty())
            {
                let changed_elsewhere = {
                    let mut records = persistence.records.write().await;
                    if let Some(record) = records.get_mut(&session_id) {
                        if record.title.as_deref() == Some(title) {
                            false
                        } else {
                            record.title = Some(title.to_owned());
                            record.title_origin = Some("manual".into());
                            true
                        }
                    } else {
                        false
                    }
                };
                if changed_elsewhere {
                    let _ = persist_records(&persistence.records, &persistence.state_path).await;
                    if let Ok(Some(mutation)) = persistence
                        .persistence
                        .set_title(
                            session_id.clone(),
                            title.to_owned(),
                            SessionTitleOrigin::Manual,
                        )
                        .await
                    {
                        if let Some(event) = mutation.event {
                            let _ = persistence.persistence.publish_event(&app, event).await;
                        }
                    }
                }
            }
        }
        if matches!(
            method.as_str(),
            "turn/completed" | "turn/failed" | "turn/interrupted"
        ) {
            persistence
                .pending_compact_sources
                .lock()
                .await
                .remove(&session_id);
            let status = match method.as_str() {
                "turn/completed" => SessionStatus::Ready,
                "turn/failed" => SessionStatus::Failed,
                _ => SessionStatus::Interrupted,
            };
            if let Some(record) = persistence.records.write().await.get_mut(&session_id) {
                record.status = status.as_str().into();
            }
            let _ = persist_records(&persistence.records, &persistence.state_path).await;
            if let Ok(Some(session)) = persistence
                .persistence
                .get_session(session_id.clone())
                .await
            {
                if let Some(run_id) = session.active_run_id {
                    let run_status = match method.as_str() {
                        "turn/completed" => RunStatus::Completed,
                        "turn/failed" => RunStatus::Failed,
                        _ => RunStatus::Interrupted,
                    };
                    if let Some(runs) = persistence.persistence.runs() {
                        if let Ok(mutation) = runs
                            .transition(
                                run_id,
                                run_status,
                                Some(params.clone()),
                                EventSource::Codex,
                            )
                            .await
                        {
                            if let Some(event) = mutation.event {
                                let _ = persistence.persistence.publish_event(&app, event).await;
                            }
                        }
                    }
                } else if let Ok(Some(mutation)) = persistence
                    .persistence
                    .transition_session(
                        session_id.clone(),
                        status,
                        EventSource::Codex,
                        params.clone(),
                    )
                    .await
                {
                    // No active run: still advance Session through the machine.
                    if let Some(event) = mutation.event {
                        let _ = persistence.persistence.publish_event(&app, event).await;
                    }
                }
            }
        }
    }
    let mut pending = pending.lock().await;
    for (_, sender) in pending.drain() {
        let _ = sender.send(Err(STDOUT_CLOSED.into()));
    }
    drop(pending);
    let owned_attachment = {
        let mut sessions = persistence.sessions.write().await;
        let owns_current = sessions
            .get(&session_id)
            .is_some_and(|session| session.attachment_id == persistence.attachment_id);
        if owns_current {
            sessions.remove(&session_id);
        }
        owns_current
    };
    if !owned_attachment {
        return;
    }
    let was_running = {
        let mut records = persistence.records.write().await;
        records.get_mut(&session_id).is_some_and(|record| {
            if !SessionStatus::Running.equals_str(&record.status) {
                return false;
            }
            record.status = SessionStatus::Interrupted.as_str().into();
            true
        })
    };
    if was_running {
        let _ = persist_records(&persistence.records, &persistence.state_path).await;
        let _ = app.emit(
            EVENT_NAME,
            CodexEvent {
                session_id: session_id.clone(),
                method: "session/unhealthy".into(),
                params: json!({
                    "reason": "app_server_exited",
                    "message": "The local agent process exited before the turn completed."
                }),
            },
        );
        finalize_performance_tracker(
            &persistence.persistence,
            &persistence.performance_trackers,
            &session_id,
            RunStatus::Interrupted.as_str(),
            None,
        )
        .await;
        if let Ok(Some(event)) = persistence
            .persistence
            .interrupt_active_run(&session_id, "app_server_exited")
            .await
        {
            let _ = persistence.persistence.publish_event(&app, event).await;
        }
    }
}

pub(crate) fn normalized_turn_method<'a>(method: &'a str, params: &Value) -> &'a str {
    if method != "turn/completed" {
        return method;
    }
    let turn = params.get("turn").unwrap_or(params);
    let status_is_failure = turn
        .get("status")
        .and_then(Value::as_str)
        .is_some_and(|status| matches!(status.to_ascii_lowercase().as_str(), "failed" | "error"));
    let has_error = turn.get("error").is_some_and(|error| !error.is_null());
    if status_is_failure || has_error {
        "turn/failed"
    } else {
        method
    }
}

pub(crate) fn rejection_response(available: &[String], id: Value) -> Value {
    let decision = ["decline", "reject", "deny", "cancel", "no"]
        .iter()
        .find(|candidate| available.iter().any(|value| value == **candidate));
    match decision {
        Some(decision) => json!({"jsonrpc":"2.0","id":id,"result":{"decision":decision}}),
        None => {
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":"No supported rejection decision"}})
        }
    }
}

pub(crate) fn automatic_approval_response(available: &[String], id: Value) -> Value {
    // Prefer the durable session decision, then fall back to one permitted
    // action for providers that do not expose a session-scoped variant.
    match select_approval_decision(available, "always")
        .or_else(|_| select_approval_decision(available, "once"))
    {
        Ok(decision) => json!({"jsonrpc":"2.0","id":id,"result":{"decision":decision}}),
        Err(error) => {
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":format!("Cannot automatically approve this request: {error}")}})
        }
    }
}

pub(crate) fn safe_approval_payload(
    approval_id: &str,
    method: &str,
    params: &Value,
    available: &[String],
) -> Value {
    let command = params
        .get("command")
        .and_then(Value::as_str)
        .or_else(|| params.pointer("/item/command").and_then(Value::as_str));
    let cwd = params
        .get("cwd")
        .and_then(Value::as_str)
        .or_else(|| params.pointer("/item/cwd").and_then(Value::as_str));
    let path = params
        .get("path")
        .and_then(Value::as_str)
        .or_else(|| params.pointer("/item/path").and_then(Value::as_str));
    let kind = if method.to_ascii_lowercase().contains("file") {
        "file_change"
    } else if method.to_ascii_lowercase().contains("command") || command.is_some() {
        "shell_command"
    } else {
        "permission"
    };
    let detail = match (kind, cwd, path) {
        ("shell_command", Some(cwd), _) => format!("Run a shell command in {cwd}"),
        ("shell_command", None, _) => "Run a shell command".into(),
        ("file_change", _, Some(path)) => format!("Modify {path}"),
        ("file_change", _, None) => "Modify workspace files".into(),
        _ => "Use a protected capability".into(),
    };
    let always_supported = ["acceptForSession", "allowForSession", "always"]
        .iter()
        .any(|candidate| available.iter().any(|value| value == candidate));
    json!({
        "approvalId": approval_id,
        "requestMethod": method,
        "kind": kind,
        "detail": detail,
        "scope": cwd.or(path),
        "alwaysSupported": always_supported,
    })
}

pub(crate) fn is_approval_method(method: &str) -> bool {
    matches!(
        method,
        "item/commandExecution/requestApproval"
            | "item/fileChange/requestApproval"
            | "commandExecution/requestApproval"
            | "applyPatch/requestApproval"
            | "fileChange/requestApproval"
            | "permissions/request"
            | "execCommandApproval"
    )
}

pub(crate) async fn write_message(
    stdin: &Arc<Mutex<tokio::process::ChildStdin>>,
    value: &Value,
) -> Result<()> {
    let mut encoded = serde_json::to_vec(value)?;
    encoded.push(b'\n');
    let mut stdin = stdin.lock().await;
    stdin.write_all(&encoded).await?;
    stdin.flush().await?;
    Ok(())
}
