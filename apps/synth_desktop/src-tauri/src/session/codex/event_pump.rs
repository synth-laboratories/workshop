//! Stdio JSON-RPC event pump for the Codex app-server child.
use anyhow::{anyhow, Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{atomic::AtomicU64, Arc},
    time::Duration,
};

fn codex_child_path(binary: &Path, inherited: Option<&OsStr>) -> Result<Option<OsString>> {
    let Some(parent) = binary
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(inherited.map(OsStr::to_os_string));
    };
    let mut entries = vec![parent.to_path_buf()];
    if let Some(inherited) = inherited {
        entries.extend(std::env::split_paths(inherited));
    }
    Ok(Some(
        std::env::join_paths(entries).context("compose Codex child PATH")?,
    ))
}
use tauri::AppHandle;
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader},
    net::UnixStream,
    process::Command,
    sync::{Mutex, RwLock},
};
use tokio_tungstenite::{client_async, tungstenite::Message};

type CodexWebSocket = tokio_tungstenite::WebSocketStream<UnixStream>;

async fn connect_websocket(path: &Path) -> Result<CodexWebSocket> {
    let stream = UnixStream::connect(path).await?;
    let (socket, _) = client_async("ws://localhost/", stream).await?;
    Ok(socket)
}

use crate::domain::{RunStatus, SessionStatus, SessionTitleOrigin};
use crate::session::approval::{ApprovalDecision, ApprovalKind, ApprovalOrigin, ApprovalScope};
use crate::session::SessionPersistence;
use crate::storage::EventSource;

use super::generation_speed::{monotonic_us, MEASUREMENT_EVENT};
use super::home::persist_records;
use super::proto::{
    default_approval_policy, select_approval_decision, AppServer, CodexResolver,
    CodexSessionRecord, CodexSessionStartRequest, CompactWaiters, Pending, ProviderTransport,
    RpcWriter, Session, STDOUT_CLOSED,
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
pub(crate) const CREDENTIAL_ENV_NAMES: &[&str] =
    &["SYNTH_API_KEY", "OPENROUTER_API_KEY", "OPENAI_API_KEY"];

/// The single provider variable the Codex child is allowed to receive, if any.
///
/// Refusing a real credential name is an error, not an omission: launching the
/// child without the variable it was configured to read would surface later as
/// an unauthenticated provider, and the reason would be nowhere in the logs.
pub(crate) fn provider_child_env(
    request: &CodexSessionStartRequest,
) -> Result<Option<(String, String)>> {
    if super::home::provider_class(request.provider_name.as_deref())
        == super::home::ProviderClass::OpenaiCodexOauth
    {
        return Ok(None);
    }
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
    pub persistent: bool,
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
    /// Typed terminal notifications that can race ahead of `turn/start`'s
    /// response. The manager consumes one when it creates that run.
    pub early_terminal_turns: Arc<Mutex<HashMap<String, Option<(String, Value)>>>>,
    pub performance_trackers: PerformanceTrackers,
    pub receipts: Arc<crate::credential_broker::ReceiptStore>,
    pub approvals: Arc<crate::session::approval::ApprovalBroker>,
    pub attachment_id: uuid::Uuid,
    pub codex_oauth: bool,
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
        persistent,
    } = spawn;
    if persistent {
        return spawn_persistent_server(app, binary, session_id, home, request, pump).await;
    }
    let mut command = Command::new(binary);
    command
        .args(["app-server", "--listen", "stdio://"])
        .current_dir(&request.workspace)
        .env("CODEX_HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // A configured Codex executable is commonly a `#!/usr/bin/env node`
    // launcher. Finder launches do not inherit the operator's shell PATH, so
    // resolving the launcher without also exposing its sibling `node` still
    // fails at runtime. Prepend the resolved launcher's directory without
    // discarding the packaged app's existing PATH.
    if let Some(path) = codex_child_path(binary, std::env::var_os("PATH").as_deref())? {
        command.env("PATH", path);
    }
    if let Some((name, value)) = provider_child_env(request)? {
        command.env(name, value);
    }
    // A Stop must contain every process the app-server owns, not merely the
    // JSON-RPC parent. Shell tools commonly spawn grandchildren, and killing
    // only the parent leaves a command (or its container client) running after
    // the composer has been told the turn is over. Give each attachment its
    // own process group so the transport can terminate that exact tree.
    isolate_process_group(&mut command);
    let mut child = command.spawn().context("spawn codex app-server")?;
    let stdin: RpcWriter = Arc::new(Mutex::new(Box::new(
        child.stdin.take().context("capture app-server stdin")?,
    )));
    let stdout = child.stdout.take().context("capture app-server stdout")?;
    let stderr = child.stderr.take().context("capture app-server stderr")?;
    let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
    let server = Arc::new(AppServer {
        child: Mutex::new(Some(child)),
        stdin: stdin.clone(),
        pending: pending.clone(),
        next_id: AtomicU64::new(1),
        persistent: false,
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
        approval_policy,
        pump.clone(),
    ));
    let stderr_persistence = pump.persistence.clone();
    tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let line = crate::secrets::redact_live(&crate::codex_oauth::redact_text(&line));
            stderr_persistence
                .notify_codex_event(&app, sid.clone(), "app-server/stderr", json!({"line":line}))
                .await;
        }
    });
    Ok(server)
}

/// Connect to (or launch) the instance's durable ChatGPT app-server.
///
/// The Unix listener is owned by Codex itself. Workshop can disappear and a
/// later process can reconnect to the same live thread with `thread/resume`.
/// Only ChatGPT OAuth sessions use this path: proxy-backed providers depend on
/// the in-process credential broker and therefore cannot safely outlive it.
async fn spawn_persistent_server<R: tauri::Runtime>(
    app: AppHandle<R>,
    binary: &Path,
    session_id: &str,
    home: &Path,
    request: &CodexSessionStartRequest,
    pump: EventPumpState,
) -> Result<Arc<AppServer>> {
    let mut digest = Sha256::new();
    digest.update(crate::instance::instance_id().as_bytes());
    digest.update(b"\0");
    digest.update(session_id.as_bytes());
    let suffix = format!("{:x}", digest.finalize());
    let socket = std::env::temp_dir().join(format!(
        "synth-codex-{}-{}.sock",
        unsafe { libc::geteuid() },
        &suffix[..20]
    ));

    let ws = match connect_websocket(&socket).await {
        Ok(socket) => socket,
        Err(_) => {
            if socket.exists() {
                // A Unix socket with no listener is inert filesystem state.
                // Remove this exact deterministic socket only after connect
                // failed; never unlink a socket another server accepted.
                std::fs::remove_file(&socket).with_context(|| {
                    format!("remove stale app-server socket {}", socket.display())
                })?;
            }
            let mut command = Command::new(binary);
            command
                .arg("app-server")
                .arg("--listen")
                .arg(format!("unix://{}", socket.display()))
                .current_dir(&request.workspace)
                .env("CODEX_HOME", home)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(false);
            if let Some(path) = codex_child_path(binary, std::env::var_os("PATH").as_deref())? {
                command.env("PATH", path);
            }
            isolate_process_group(&mut command);
            // `kill_on_drop(false)` deliberately transfers the listener's
            // lifetime to Codex. Workshop owns only a WebSocket attachment.
            let listener = command
                .spawn()
                .context("spawn persistent codex app-server")?;
            let mut connected = None;
            for _ in 0..100 {
                match connect_websocket(&socket).await {
                    Ok(stream) => {
                        connected = Some(stream);
                        break;
                    }
                    Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
                }
            }
            let connected = connected.ok_or_else(|| {
                anyhow!(
                    "persistent codex app-server did not open {}",
                    socket.display()
                )
            })?;
            drop(listener);
            connected
        }
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("secure app-server socket {}", socket.display()))?;
    }
    // Adapt WebSocket text messages to the existing newline JSON-RPC pump.
    // The two bounded duplex channels provide backpressure in both directions.
    let (mut websocket_sink, mut websocket_stream) = ws.split();
    let (stdin_writer, stdin_reader) = tokio::io::duplex(1024 * 1024);
    let (stdout_writer, stdout_reader) = tokio::io::duplex(1024 * 1024);
    let stdin: RpcWriter = Arc::new(Mutex::new(Box::new(stdin_writer)));
    tauri::async_runtime::spawn(async move {
        let mut lines = BufReader::new(stdin_reader).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if websocket_sink
                .send(Message::Text(line.into()))
                .await
                .is_err()
            {
                break;
            }
        }
        let _ = websocket_sink.close().await;
    });
    tauri::async_runtime::spawn(async move {
        let mut output = stdout_writer;
        while let Some(message) = websocket_stream.next().await {
            match message {
                Ok(Message::Text(text)) => {
                    if output.write_all(text.as_bytes()).await.is_err()
                        || output.write_all(b"\n").await.is_err()
                    {
                        break;
                    }
                }
                Ok(Message::Close(_)) | Err(_) => break,
                _ => {}
            }
        }
    });
    let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
    let server = Arc::new(AppServer {
        child: Mutex::new(None),
        stdin: stdin.clone(),
        pending: pending.clone(),
        next_id: AtomicU64::new(1),
        persistent: true,
    });
    let approval_policy = request
        .approval_policy
        .clone()
        .unwrap_or_else(default_approval_policy);
    tauri::async_runtime::spawn(read_stdout(
        app,
        session_id.to_owned(),
        stdout_reader,
        stdin,
        pending,
        approval_policy,
        pump,
    ));
    Ok(server)
}

#[cfg(unix)]
fn isolate_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.as_std_mut().process_group(0);
}

#[cfg(not(unix))]
fn isolate_process_group(_command: &mut Command) {}

#[cfg(test)]
mod persistent_transport_tests {
    use super::*;
    use serde_json::json;

    async fn response(socket: &mut CodexWebSocket, id: u64) -> Value {
        loop {
            let message = socket
                .next()
                .await
                .expect("websocket closed")
                .expect("websocket error");
            if let Message::Text(text) = message {
                let value: Value = serde_json::from_str(&text).expect("valid app-server JSON");
                if value.get("id").and_then(Value::as_u64) == Some(id) {
                    return value;
                }
            }
        }
    }

    async fn send(socket: &mut CodexWebSocket, value: Value) {
        socket
            .send(Message::Text(value.to_string().into()))
            .await
            .expect("send app-server request");
    }

    /// Requires the locally installed Codex binary. This is an explicit
    /// transport acceptance test, not part of hermetic CI.
    #[tokio::test]
    #[ignore = "requires a local codex binary"]
    async fn reconnects_over_unix_websocket_and_resumes_the_same_thread() {
        let root = tempfile::Builder::new()
            .prefix("synth-codex-")
            .tempdir_in("/tmp")
            .expect("short temporary path");
        let home = root.path().join("home");
        std::fs::create_dir_all(&home).expect("create isolated CODEX_HOME");
        let socket_path = root.path().join("server.sock");
        let mut server = Command::new("codex");
        server
            .args(["app-server", "--listen"])
            .arg(format!("unix://{}", socket_path.display()))
            .env("CODEX_HOME", &home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut server = server.spawn().expect("launch codex app-server");

        let mut first = None;
        for _ in 0..100 {
            match connect_websocket(&socket_path).await {
                Ok(socket) => {
                    first = Some(socket);
                    break;
                }
                Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
        let mut first = first.expect("first websocket connection");
        send(&mut first, json!({
            "method": "initialize", "id": 1,
            "params": {"clientInfo": {"name": "workshop-proof", "title": "Workshop proof", "version": "0.8"}, "capabilities": {"experimentalApi": true}}
        })).await;
        assert!(response(&mut first, 1).await.get("result").is_some());
        send(&mut first, json!({
            "method": "thread/start", "id": 2,
            "params": {"cwd": root.path(), "approvalPolicy": "never", "sandbox": "read-only", "persistExtendedHistory": true}
        })).await;
        let started = response(&mut first, 2).await;
        let thread_id = started
            .pointer("/result/thread/id")
            .and_then(Value::as_str)
            .expect("thread id")
            .to_owned();
        // A started thread is not resumable until Codex has created its first
        // rollout. The turn need not succeed (this isolated home has no auth);
        // its durable identity is what this transport test exercises.
        send(
            &mut first,
            json!({
                "method": "turn/start", "id": 5,
                "params": {"threadId": thread_id, "model": "gpt-5.1", "input": [{"type": "text", "text": "transport proof", "textElements": []}], "approvalPolicy": "never"}
            }),
        )
        .await;
        let turn_started = response(&mut first, 5).await;
        assert!(
            turn_started.pointer("/result/turn/id").is_some(),
            "unexpected turn/start response: {turn_started}"
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
        first.close(None).await.expect("close first client");

        let mut second = connect_websocket(&socket_path)
            .await
            .expect("second websocket connection");
        send(&mut second, json!({
            "method": "initialize", "id": 3,
            "params": {"clientInfo": {"name": "workshop-proof", "title": "Workshop proof", "version": "0.8"}, "capabilities": {"experimentalApi": true}}
        })).await;
        assert!(response(&mut second, 3).await.get("result").is_some());
        send(
            &mut second,
            json!({
                "method": "thread/resume", "id": 4,
                "params": {"threadId": thread_id, "persistExtendedHistory": true}
            }),
        )
        .await;
        let resumed = response(&mut second, 4).await;
        assert_eq!(
            resumed.pointer("/result/thread/id").and_then(Value::as_str),
            Some(thread_id.as_str()),
            "unexpected thread/resume response: {resumed}"
        );
        server.kill().await.expect("stop proof server");
    }
}

async fn read_stdout<R: tauri::Runtime, T: AsyncRead + Unpin>(
    app: AppHandle<R>,
    session_id: String,
    stdout: T,
    stdin: RpcWriter,
    pending: Pending,
    approval_policy: String,
    persistence: EventPumpState,
) {
    let mut lines = BufReader::new(stdout).lines();
    let mut heartbeat = tokio::time::interval(Duration::from_secs(5));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut settlement = TurnSettlement::default();
    loop {
        // An open provider stdout channel is positive liveness evidence even
        // while an XHigh turn or proposer is legitimately silent. Previously
        // ownership was refreshed only after a decoded frame, so any >20s
        // think was reconciled as a crash and interrupted while its optimizer
        // continued running. EOF still exits this loop and follows the normal
        // terminal/unhealthy settlement path.
        let next_line = tokio::select! {
            _ = heartbeat.tick() => {
                persistence.persistence.heartbeat_turn(&session_id).await;
                continue;
            }
            next_line = lines.next_line() => next_line,
        };
        let Ok(Some(line)) = next_line else {
            break;
        };
        // Stamped here, on the decoded frame, before parsing, redaction,
        // persistence, IPC, or any renderer work. Generation speed is a claim
        // about delivery, and every millisecond spent below this line would
        // otherwise be charged to the model.
        let received_at_us = monotonic_us();
        let line = crate::secrets::redact_live(&line);
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
        crate::codex_oauth::redact_event_value(&mut params);
        // Some Responses-compatible servers report a terminal envelope as
        // `turn/completed` even when the enclosed turn has failed. Preserve
        // the actual outcome: otherwise the transcript says “Worked” while
        // there is no answer to show.
        let method = normalized_turn_method(raw_method, &params).to_owned();
        if persistence.codex_oauth && method == "turn/failed" {
            normalize_oauth_failure(&mut params);
        }
        // Some app-server versions close the stream immediately after a
        // typed final agent message instead of sending `turn/completed`.
        // `phase: final_answer` is a protocol lifecycle signal, not an
        // inference from response text, so EOF must not overwrite it as an
        // unhealthy local agent.
        //
        // Ordinary commentary between tool batches is not terminal. Track
        // settlement so a later stdout EOF can complete a turn that already
        // used tools and emitted an assistant item; do not finish while the
        // process is still producing events.
        settlement.observe(&method, &params);
        let final_answer = is_final_agent_message(&method, &params);
        if final_answer {
            crate::recovery::crash_checkpoint(crate::recovery::checkpoints::BEFORE_FINAL_MESSAGE);
        }
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
        if is_approval_method(&method) {
            let available_decisions = approval_decisions(&params);
            let Some(rpc_id) = message.get("id").cloned() else {
                // An approval without a JSON-RPC request id cannot be answered.
                // Do not leave the turn indefinitely in Working: surface a safe,
                // actionable terminal event instead of treating it as activity.
                fail_malformed_approval(
                    &app,
                    &session_id,
                    &persistence,
                    "The provider requested approval in an unsupported format. Stop and retry this turn.",
                )
                .await;
                continue;
            };
            if available_decisions.is_empty() {
                let _ = write_message(
                    &stdin,
                    &json!({
                        "jsonrpc":"2.0",
                        "id":rpc_id,
                        "error":{"code":-32602,"message":"Approval request did not advertise any supported decisions"}
                    }),
                )
                .await;
                fail_malformed_approval(
                    &app,
                    &session_id,
                    &persistence,
                    "The provider requested approval without any available decision. Stop and retry this turn.",
                )
                .await;
                continue;
            }
            if approval_policy == "never" {
                // “Allow all” is an explicit session policy. A provider
                // may still ask despite it, so resolve the request rather
                // than surfacing a contradictory approval card. The decision
                // still gets a durable `approval.granted` receipt — permissive
                // policy must never mean unaudited work — but the provider is
                // answered even if persisting the receipt fails, because an
                // unanswered approval wedges the turn.
                if let Some(decision) = automatic_approval_decision(&available_decisions) {
                    let kind = shell_approval_kind(&method, &params, &available_decisions);
                    let _ = persistence
                        .approvals
                        .record_auto(&app, &session_id, &kind, &decision, &approval_policy)
                        .await;
                }
                let response = automatic_approval_response(&available_decisions, rpc_id);
                let _ = write_message(&stdin, &response).await;
                continue;
            }
            let kind = shell_approval_kind(&method, &params, &available_decisions);
            let resolver = Arc::new(CodexResolver {
                stdin: stdin.clone(),
                rpc_id,
                available_decisions,
            });
            let origin = ApprovalOrigin {
                session_id: session_id.clone(),
                instance_id: persistence.attachment_id.to_string(),
            };
            let _ = persistence
                .approvals
                .request(&app, origin, kind, resolver)
                .await;
            continue;
        }
        if let Some(rpc_id) = message.get("id").cloned() {
            // Unknown server requests are never approved implicitly.
            let _ = write_message(&stdin, &json!({
                "jsonrpc":"2.0","id":rpc_id,
                "error":{"code":-32601,"message":format!("Unsupported server request: {method}")}
            })).await;
            continue;
        }
        if matches!(
            method.as_str(),
            "turn/completed" | "turn/failed" | "turn/interrupted"
        ) {
            let _ = persistence
                .approvals
                .expire_session(&app, &session_id, "origin_turn_ended")
                .await;
        }
        persistence
            .persistence
            .notify_codex_event(&app, session_id.clone(), method.clone(), params.clone())
            .await;
        // Live provider traffic is the proof that this process still owns the
        // turn. Refreshing here — rather than on a timer the renderer holds —
        // is what keeps a long XHigh turn from being reconciled away while it
        // is genuinely thinking, and what makes a dead one expire on its own.
        persistence.persistence.heartbeat_turn(&session_id).await;
        crate::recovery::crash_checkpoint(crate::recovery::checkpoints::AFTER_FIRST_ACTIVITY);
        let measurements = track_performance_event(
            &persistence.persistence,
            &persistence.performance_trackers,
            &persistence.receipts,
            &session_id,
            &method,
            &params,
            received_at_us,
        )
        .await;
        // A finished segment goes onto the journal as its own event, so the
        // transcript renders an authoritative backend measurement instead of
        // recomputing a rate from deltas it saw after IPC and batching.
        for measurement in &measurements {
            let Ok(payload) = serde_json::to_value(measurement) else {
                continue;
            };
            persistence
                .persistence
                .notify_codex_event(&app, session_id.clone(), MEASUREMENT_EVENT, payload)
                .await;
        }
        let terminal_method = if final_answer {
            "turn/completed"
        } else {
            method.as_str()
        };
        if matches!(
            terminal_method,
            "turn/completed" | "thread/compact/completed"
        ) {
            if let Some(waiter) = persistence.compact_waiters.lock().await.remove(&session_id) {
                let _ = waiter.send(Ok(()));
            }
        } else if matches!(terminal_method, "turn/failed" | "turn/interrupted") {
            if let Some(waiter) = persistence.compact_waiters.lock().await.remove(&session_id) {
                let _ = waiter.send(Err(format!("context compaction ended with {method}")));
            }
        }
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
            terminal_method,
            "turn/completed" | "turn/failed" | "turn/interrupted"
        ) {
            apply_codex_terminal(
                &app,
                &session_id,
                &persistence,
                terminal_method,
                params.clone(),
            )
            .await;
        }
    }
    if settlement.ready_for_eof_completion() {
        // The child closed stdout after tools settled and an assistant item
        // completed, without `turn/completed` or `phase: final_answer`. That
        // is process-exit evidence, not a mid-turn commentary gap.
        let params = json!({
            "reason": "app_server_exited_after_settled_turn",
            "message": "The local agent process exited after tools settled and an assistant item completed."
        });
        persistence
            .persistence
            .notify_codex_event(&app, session_id.clone(), "turn/completed", params.clone())
            .await;
        apply_codex_terminal(&app, &session_id, &persistence, "turn/completed", params).await;
    }
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
    if owned_attachment {
        // The app-server is gone, so this process can no longer advance the
        // turn whatever its records say. Release first: the durable
        // interruption below is best-effort, and an unreleased claim would
        // outlive it.
        persistence.persistence.release_turn(&session_id).await;
        let terminal_pending = persistence
            .early_terminal_turns
            .lock()
            .await
            .get(&session_id)
            .is_some_and(Option::is_some);
        let was_running = {
            let mut records = persistence.records.write().await;
            records.get_mut(&session_id).is_some_and(|record| {
                if terminal_pending || !SessionStatus::Running.equals_str(&record.status) {
                    return false;
                }
                record.status = SessionStatus::Interrupted.as_str().into();
                true
            })
        };
        if was_running {
            let _ = persist_records(&persistence.records, &persistence.state_path).await;
            persistence
                .persistence
                .notify_codex_event(
                    &app,
                    session_id.clone(),
                    "session/unhealthy",
                    json!({
                        "reason": "app_server_exited",
                        "message": "The local agent process exited before the turn completed."
                    }),
                )
                .await;
            // A segment cut off by the child's death is still evidence; publish
            // it so the transcript can label it partial rather than silently
            // showing nothing for an answer that was measurably arriving.
            let measurements = finalize_performance_tracker(
                &persistence.persistence,
                &persistence.performance_trackers,
                &persistence.receipts,
                &session_id,
                RunStatus::Interrupted.as_str(),
                None,
            )
            .await;
            for measurement in &measurements {
                let Ok(payload) = serde_json::to_value(measurement) else {
                    continue;
                };
                persistence
                    .persistence
                    .notify_codex_event(&app, session_id.clone(), MEASUREMENT_EVENT, payload)
                    .await;
            }
            if let Ok(Some(event)) = persistence
                .persistence
                .interrupt_active_run(&session_id, "app_server_exited")
                .await
            {
                let _ = persistence.persistence.publish_event(&app, event).await;
            }
        }
    }
    let _ = persistence
        .approvals
        .expire_session(&app, &session_id, "origin_process_exited")
        .await;
    // Release failed requests only after the attachment owner has finalized
    // its durable run. Command callers therefore observe authoritative state
    // and do not need a second reconciliation path.
    let mut pending = pending.lock().await;
    for (_, sender) in pending.drain() {
        let _ = sender.send(Err(STDOUT_CLOSED.into()));
    }
}

fn is_final_agent_message(method: &str, params: &Value) -> bool {
    if !matches!(method, "item/completed" | "item/agentMessage") {
        return false;
    }
    let Some(item) = params.get("item").and_then(Value::as_object) else {
        return false;
    };
    let item_type = item
        .get("type")
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase);
    let phase = item
        .get("phase")
        .or_else(|| params.get("phase"))
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase);
    matches!(item_type.as_deref(), Some("agentmessage"))
        && matches!(phase.as_deref(), Some("final_answer"))
}

fn is_tool_item(item_type: Option<&str>) -> bool {
    matches!(
        item_type.unwrap_or_default(),
        "functioncall"
            | "function_call"
            | "customtool"
            | "custom_tool"
            | "customtoolcall"
            | "mcptoolcall"
            | "mcp_tool_call"
            | "commandexecution"
            | "command_execution"
            | "tool"
            | "toolcall"
            | "tool_call"
    )
}

fn item_type_and_id(params: &Value) -> (Option<String>, String) {
    let item = params.get("item").and_then(Value::as_object);
    let item_type = item
        .and_then(|item| item.get("type"))
        .and_then(Value::as_str)
        .map(str::to_ascii_lowercase);
    let item_id = item
        .and_then(|item| item.get("id"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("item")
        .to_string();
    (item_type, item_id)
}

fn is_completed_agent_message(method: &str, params: &Value) -> bool {
    if !matches!(method, "item/completed" | "item/agentMessage") {
        return false;
    }
    let (item_type, _) = item_type_and_id(params);
    matches!(item_type.as_deref(), Some("agentmessage"))
}

/// Tracks whether stdout EOF may complete a turn. Ordinary commentary between
/// tool batches is never terminal while the child is still producing events.
/// Authoritative completion remains `turn/completed`, `phase: final_answer`,
/// crash-recovery lease expiry, or process exit after tools and an assistant
/// item have already landed.
#[derive(Default)]
struct TurnSettlement {
    in_flight: HashSet<String>,
    tools_seen: bool,
    assistant_completed: bool,
    protocol_terminal: bool,
}

impl TurnSettlement {
    fn observe(&mut self, method: &str, params: &Value) {
        if matches!(method, "turn/started" | "turn/start") {
            *self = Self::default();
            return;
        }
        if matches!(
            method,
            "turn/completed" | "turn/failed" | "turn/interrupted"
        ) || is_final_agent_message(method, params)
        {
            self.protocol_terminal = true;
            if is_completed_agent_message(method, params) {
                self.assistant_completed = true;
            }
            return;
        }
        let (item_type, item_id) = item_type_and_id(params);
        if is_tool_item(item_type.as_deref()) {
            self.tools_seen = true;
            if method == "item/started" {
                self.in_flight.insert(item_id);
            } else if matches!(method, "item/completed" | "item/failed") {
                self.in_flight.remove(&item_id);
            }
            return;
        }
        if is_completed_agent_message(method, params) {
            self.assistant_completed = true;
        }
    }

    fn ready_for_eof_completion(&self) -> bool {
        !self.protocol_terminal
            && self.tools_seen
            && self.in_flight.is_empty()
            && self.assistant_completed
    }
}

async fn apply_codex_terminal<R: tauri::Runtime>(
    app: &AppHandle<R>,
    session_id: &str,
    persistence: &EventPumpState,
    terminal_method: &str,
    params: Value,
) {
    // A pending entry means `turn/start` has not finished registering
    // its durable run. Update that mailbox while holding its one lock;
    // otherwise the manager could remove the marker in between the
    // ownership check and this terminal write.
    let manager_owns_terminal = {
        let mut terminals = persistence.early_terminal_turns.lock().await;
        if terminals.contains_key(session_id) {
            terminals.insert(
                session_id.to_owned(),
                Some((terminal_method.to_owned(), params.clone())),
            );
            true
        } else {
            false
        }
    };
    persistence
        .pending_compact_sources
        .lock()
        .await
        .remove(session_id);
    let status = match terminal_method {
        "turn/completed" => SessionStatus::Ready,
        "turn/failed" => SessionStatus::Failed,
        _ => SessionStatus::Interrupted,
    };
    if let Some(record) = persistence.records.write().await.get_mut(session_id) {
        record.status = status.as_str().into();
    }
    let _ = persist_records(&persistence.records, &persistence.state_path).await;
    persistence.persistence.release_turn(session_id).await;
    if manager_owns_terminal {
        return;
    }
    if let Ok(Some(session)) = persistence
        .persistence
        .get_session(session_id.to_owned())
        .await
    {
        if let Some(run_id) = session.active_run_id {
            let run_status = match terminal_method {
                "turn/completed" => RunStatus::Completed,
                "turn/failed" => RunStatus::Failed,
                _ => RunStatus::Interrupted,
            };
            if let Some(runs) = persistence.persistence.runs() {
                if let Ok(mutation) = runs
                    .transition(run_id, run_status, Some(params), EventSource::Codex)
                    .await
                {
                    if let Some(event) = mutation.event {
                        let _ = persistence.persistence.publish_event(app, event).await;
                    }
                }
            }
        } else if let Ok(Some(mutation)) = persistence
            .persistence
            .transition_session(session_id.to_owned(), status, EventSource::Codex, params)
            .await
        {
            if let Some(event) = mutation.event {
                let _ = persistence.persistence.publish_event(app, event).await;
            }
        }
    }
}

async fn fail_malformed_approval<R: tauri::Runtime>(
    app: &AppHandle<R>,
    session_id: &str,
    persistence: &EventPumpState,
    message: &str,
) {
    let params = json!({
        "code": "approval_request_malformed",
        "message": message,
    });
    persistence
        .persistence
        .notify_codex_event(app, session_id.to_owned(), "turn/failed", params.clone())
        .await;
    if let Some(record) = persistence.records.write().await.get_mut(session_id) {
        record.status = SessionStatus::Failed.as_str().into();
    }
    let _ = persist_records(&persistence.records, &persistence.state_path).await;
    if let Ok(Some(session)) = persistence
        .persistence
        .get_session(session_id.to_owned())
        .await
    {
        if let Some(run_id) = session.active_run_id {
            if let Some(runs) = persistence.persistence.runs() {
                if let Ok(mutation) = runs
                    .transition(run_id, RunStatus::Failed, Some(params), EventSource::Codex)
                    .await
                {
                    if let Some(event) = mutation.event {
                        let _ = persistence.persistence.publish_event(app, event).await;
                    }
                }
            }
        } else if let Ok(Some(mutation)) = persistence
            .persistence
            .transition_session(
                session_id.to_owned(),
                SessionStatus::Failed,
                EventSource::Codex,
                params,
            )
            .await
        {
            if let Some(event) = mutation.event {
                let _ = persistence.persistence.publish_event(app, event).await;
            }
        }
    }
    let server = persistence
        .sessions
        .read()
        .await
        .get(session_id)
        .cloned()
        .map(|session| session.server.clone());
    if let Some(server) = server {
        let _ = server.stop().await;
    }
}

fn normalize_oauth_failure(params: &mut Value) {
    let lower = params.to_string().to_ascii_lowercase();
    let (code, message) =
        if lower.contains("usage_limit") || lower.contains("quota") || lower.contains("rate limit")
        {
            (
                "codex_oauth_usage_limit",
                "Your ChatGPT Codex plan allowance is currently unavailable.",
            )
        } else if lower.contains("401")
            || lower.contains("unauthorized")
            || lower.contains("revoked")
            || lower.contains("authentication")
        {
            (
                "codex_oauth_reauth_required",
                "Reconnect ChatGPT subscription in Settings → Models.",
            )
        } else {
            return;
        };
    *params = json!({"code": code, "message": message});
}

#[cfg(test)]
mod oauth_failure_tests {
    use super::*;

    #[test]
    fn maps_auth_and_quota_failures_to_stable_codes() {
        let mut auth = json!({"error":{"message":"401 Unauthorized"}});
        normalize_oauth_failure(&mut auth);
        assert_eq!(auth["code"], "codex_oauth_reauth_required");
        assert!(!auth.to_string().contains("401"));

        let mut quota = json!({"error":{"type":"usage_limit"}});
        normalize_oauth_failure(&mut quota);
        assert_eq!(quota["code"], "codex_oauth_usage_limit");
    }
}

#[cfg(test)]
mod terminal_message_tests {
    use super::*;

    #[test]
    fn only_completed_final_agent_messages_are_terminal() {
        let final_answer = json!({
            "item": {
                "type": "agentMessage",
                "phase": "final_answer",
                "text": "done"
            }
        });
        assert!(!is_final_agent_message("item/started", &final_answer));
        assert!(!is_final_agent_message(
            "item/agentMessage/delta",
            &final_answer
        ));
        assert!(is_final_agent_message("item/completed", &final_answer));
        assert!(is_final_agent_message("item/agentMessage", &final_answer));

        let commentary = json!({
            "item": {
                "type": "agentMessage",
                "phase": "commentary",
                "text": "working"
            }
        });
        assert!(!is_final_agent_message("item/completed", &commentary));
    }

    #[test]
    fn a_final_answer_after_settled_tools_is_terminal() {
        let after_tools = json!({
            "item": {
                "id": "msg_final",
                "type": "agentMessage",
                "phase": "final_answer",
                "text": "Banking77 is blocked on stale capabilities; HealthBench needs GEPA v2."
            }
        });
        assert!(is_final_agent_message("item/completed", &after_tools));
        assert!(is_final_agent_message("item/agentMessage", &after_tools));
    }

    #[test]
    fn commentary_after_tools_is_not_terminal_while_the_process_is_alive() {
        let mut settlement = TurnSettlement::default();
        let tool_start = json!({
            "item": { "id": "call_bind", "type": "mcpToolCall", "name": "visual_create" }
        });
        let tool_done = json!({
            "item": { "id": "call_bind", "type": "mcpToolCall", "status": "failed" }
        });
        let diagnostic = json!({
            "item": {
                "id": "msg_diag",
                "type": "agentMessage",
                "phase": "commentary",
                "text": "I found X; checking Y next."
            }
        });
        settlement.observe("item/started", &tool_start);
        settlement.observe("item/completed", &tool_done);
        settlement.observe("item/completed", &diagnostic);
        assert!(!is_final_agent_message("item/completed", &diagnostic));
        assert!(settlement.ready_for_eof_completion());
    }

    #[test]
    fn commentary_between_tool_batches_does_not_finish_the_turn() {
        let mut settlement = TurnSettlement::default();
        let first = json!({"item": {"id": "t1", "type": "functionCall"}});
        let commentary = json!({
            "item": {
                "id": "msg_note",
                "type": "agentMessage",
                "phase": "commentary",
                "text": "I found X; checking Y next."
            }
        });
        let second = json!({"item": {"id": "t2", "type": "mcpToolCall"}});
        settlement.observe("item/started", &first);
        settlement.observe("item/completed", &first);
        settlement.observe("item/completed", &commentary);
        assert!(settlement.ready_for_eof_completion());
        settlement.observe("item/started", &second);
        assert!(!settlement.ready_for_eof_completion());
        settlement.observe("item/completed", &second);
        assert!(settlement.ready_for_eof_completion());
        assert!(!is_final_agent_message("item/completed", &commentary));
    }

    #[test]
    fn commentary_without_tools_does_not_finish_the_turn() {
        let mut settlement = TurnSettlement::default();
        let commentary = json!({
            "item": {
                "id": "msg_note",
                "type": "agentMessage",
                "phase": "commentary",
                "text": "looking at the visual"
            }
        });
        settlement.observe("item/completed", &commentary);
        assert!(!settlement.ready_for_eof_completion());
        assert!(!is_final_agent_message("item/completed", &commentary));
    }

    #[test]
    fn in_flight_tools_block_eof_completion() {
        let mut settlement = TurnSettlement::default();
        let first = json!({"item": {"id": "t1", "type": "functionCall"}});
        let second = json!({"item": {"id": "t2", "type": "mcpToolCall"}});
        let assistant = json!({
            "item": {"id": "msg", "type": "agentMessage", "phase": "commentary", "text": "still working"}
        });
        settlement.observe("item/started", &first);
        settlement.observe("item/completed", &first);
        settlement.observe("item/started", &second);
        settlement.observe("item/completed", &assistant);
        assert!(!settlement.ready_for_eof_completion());
        settlement.observe("item/completed", &second);
        assert!(settlement.ready_for_eof_completion());
    }

    #[test]
    fn protocol_final_answer_is_terminal_and_not_an_eof_fallback() {
        let mut settlement = TurnSettlement::default();
        let tool = json!({"item": {"id": "t1", "type": "mcpToolCall"}});
        let final_answer = json!({
            "item": {
                "id": "msg_final",
                "type": "agentMessage",
                "phase": "final_answer",
                "text": "done"
            }
        });
        settlement.observe("item/started", &tool);
        settlement.observe("item/completed", &tool);
        settlement.observe("item/completed", &final_answer);
        assert!(is_final_agent_message("item/completed", &final_answer));
        assert!(!settlement.ready_for_eof_completion());
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

/// The typed decision `automatic_approval_response` will deliver, or `None`
/// when no automatic answer exists (the response is then a JSON-RPC error and
/// there is nothing to receipt).
pub(crate) fn automatic_approval_decision(available: &[String]) -> Option<ApprovalDecision> {
    if select_approval_decision(available, "always").is_ok() {
        Some(ApprovalDecision::Approve {
            scope: ApprovalScope::Session,
        })
    } else if select_approval_decision(available, "once").is_ok() {
        Some(ApprovalDecision::Approve {
            scope: ApprovalScope::Once,
        })
    } else {
        None
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
    shell_approval_kind(method, params, available).safe_payload(approval_id)
}

fn shell_approval_kind(method: &str, params: &Value, available: &[String]) -> ApprovalKind {
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
    let kind = if method.to_ascii_lowercase().contains("file")
        || method.to_ascii_lowercase().contains("patch")
    {
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
    ApprovalKind::ShellCommand {
        request_method: method.to_owned(),
        detail,
        scope: cwd.or(path).map(str::to_owned),
        always_supported,
    }
}

pub(crate) fn is_approval_method(method: &str) -> bool {
    matches!(
        method,
        "permissions/request" | "execCommandApproval" | "applyPatchApproval"
    ) || method.ends_with("/requestApproval")
        || method.ends_with("/request_approval")
}

pub(crate) fn approval_decisions(params: &Value) -> Vec<String> {
    let values = params
        .get("availableDecisions")
        .or_else(|| params.get("available_decisions"))
        .or_else(|| params.pointer("/item/availableDecisions"))
        .or_else(|| params.pointer("/item/available_decisions"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    values
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

pub(crate) async fn write_message(stdin: &RpcWriter, value: &Value) -> Result<()> {
    let mut encoded = serde_json::to_vec(value)?;
    encoded.push(b'\n');
    let mut stdin = stdin.lock().await;
    stdin.write_all(&encoded).await?;
    stdin.flush().await?;
    Ok(())
}

#[cfg(test)]
mod child_path_tests {
    use super::codex_child_path;
    use std::{ffi::OsStr, path::Path};

    #[test]
    fn configured_launcher_directory_precedes_finder_path() {
        let path = codex_child_path(
            Path::new("/opt/synth/node/bin/codex"),
            Some(OsStr::new("/usr/bin:/bin")),
        )
        .unwrap()
        .unwrap();
        let entries = std::env::split_paths(&path).collect::<Vec<_>>();
        assert_eq!(entries[0], Path::new("/opt/synth/node/bin"));
        assert_eq!(entries[1], Path::new("/usr/bin"));
        assert_eq!(entries[2], Path::new("/bin"));
    }
}
