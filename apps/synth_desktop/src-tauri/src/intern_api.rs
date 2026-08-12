use crate::cloud::intern::{
    AsyncCommandKind, AsyncCommandRequest, AsyncEnsureRequest, CommandReceipt, RuntimeBinding,
    RuntimeKind, SyncCommandKind, SyncCommandRequest, SyncCreateRequest,
};
use crate::core_runtime::CoreRuntime;
use crate::domain::{
    CommandReceiptInput, RunCreate, RunStatus, SessionCreate, SessionKind, SessionStatus,
};
use crate::storage::{EventSource, SessionRecord};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use uuid::Uuid;

// The Async Intern is an organization singleton. Serialize local binding
// creation so concurrent UI requests cannot both observe an empty local store
// and create two sessions for the same remote runtime.
static ASYNC_BINDING_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InternBindingRequest {
    pub factory_id: Option<String>,
    pub project_id: Option<String>,
    pub effort_id: Option<String>,
    pub run_id: Option<String>,
}

impl From<Option<InternBindingRequest>> for RuntimeBinding {
    fn from(value: Option<InternBindingRequest>) -> Self {
        let value = value.unwrap_or(InternBindingRequest {
            factory_id: None,
            project_id: None,
            effort_id: None,
            run_id: None,
        });
        Self {
            factory_id: value.factory_id,
            project_id: value.project_id,
            effort_id: value.effort_id,
            run_id: value.run_id,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InternTarget {
    pub kind: String,
    pub mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<Value>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InternSessionCreateRequest {
    pub target: InternTarget,
    pub objective: String,
    pub title: Option<String>,
    pub project_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InternSessionSendRequest {
    pub session_id: String,
    pub body: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InternSessionControlRequest {
    pub session_id: String,
    pub kind: String,
    #[serde(default)]
    pub payload: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InternSessionWire {
    pub id: String,
    pub title: String,
    pub target: Value,
    pub project_id: Option<String>,
    pub remote_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub status: String,
    pub state_generation: Option<i64>,
    pub latest_cursor: i64,
    pub active_run_id: Option<String>,
    pub metadata: Value,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InternSendResult {
    pub run_id: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InternControlResult {
    pub accepted: bool,
    pub receipt: CommandReceipt,
}

pub async fn list(core: &CoreRuntime) -> Result<Vec<InternSessionWire>> {
    Ok(core
        .sessions()
        .list(2_000)
        .await?
        .into_iter()
        .filter(|session| session.kind == SessionKind::Intern.as_str())
        .map(Into::into)
        .collect())
}

pub async fn create(
    core: &CoreRuntime,
    request: InternSessionCreateRequest,
) -> Result<InternSessionWire> {
    if request.target.kind != "intern" || !matches!(request.target.mode.as_str(), "sync" | "async")
    {
        bail!("Intern target must use sync or async mode");
    }
    let objective = request.objective.trim().to_owned();
    if objective.is_empty() {
        bail!("Intern objective is required");
    }
    // Validate the complete remote request before creating durable local
    // state. A malformed binding must not leave a phantom session behind.
    let binding_value = request.target.binding.clone().unwrap_or_else(|| json!({}));
    let binding_request: InternBindingRequest =
        serde_json::from_value(binding_value).context("invalid Intern target binding")?;
    let _async_binding_guard = if request.target.mode == "async" {
        Some(ASYNC_BINDING_LOCK.lock().await)
    } else {
        None
    };
    if request.target.mode == "async" {
        if let Some(existing) = existing_async_binding(core).await? {
            return Ok(existing.into());
        }
    }
    let client = core.intern().client().await?;
    let session_id = format!("ses_{}", Uuid::new_v4().simple());
    let title = request.title.unwrap_or_else(|| {
        if request.target.mode == "sync" {
            "Live Intern".into()
        } else {
            "Background Intern".into()
        }
    });
    let target = serde_json::to_value(&request.target)?;
    let created = core
        .sessions()
        .create_or_update(SessionCreate {
            id: session_id.clone(),
            title: title.clone(),
            kind: SessionKind::Intern,
            target: target.clone(),
            project_id: request.project_id,
            remote_id: None,
            codex_thread_id: None,
            status: SessionStatus::Created,
            state_generation: None,
            metadata: json!({"runtime": "rust-intern", "objective": objective}),
            source: EventSource::Intern,
        })
        .await?;
    core.broadcast_committed(created.event);

    let idempotency = format!("desktop-create-{session_id}");
    let remote = if request.target.mode == "sync" {
        client
            .create_sync(&SyncCreateRequest::desktop(
                objective.clone(),
                idempotency,
                Some(binding_request).into(),
            ))
            .await
    } else {
        client
            .ensure_async(&AsyncEnsureRequest::desktop(
                objective,
                idempotency,
                Some(binding_request).into(),
            ))
            .await
    };
    let projection = match remote {
        Ok(projection) => projection,
        Err(error) => {
            let failed = core
                .sessions()
                .transition(
                    session_id,
                    SessionStatus::Failed,
                    EventSource::Intern,
                    json!({"error": error.to_string()}),
                )
                .await?;
            core.broadcast_committed(failed.event);
            return Err(error.into());
        }
    };
    let runtime_id = match projection.runtime_id() {
        Some(runtime_id) => runtime_id.to_owned(),
        None => {
            fail_session(
                core,
                session_id,
                "Intern response omitted runtime identity".into(),
            )
            .await?;
            bail!("Intern response omitted runtime identity");
        }
    };
    let ready = core
        .sessions()
        .create_or_update(SessionCreate {
            id: created.value.id,
            title: title.clone(),
            kind: SessionKind::Intern,
            target,
            project_id: created.value.project_id,
            remote_id: Some(runtime_id.clone()),
            codex_thread_id: None,
            status: SessionStatus::Ready,
            state_generation: Some(i64::try_from(projection.state_generation)?),
            metadata: json!({"runtime": "rust-intern", "objective": request.objective.trim(), "projection": projection}),
            source: EventSource::Intern,
        })
        .await?;
    core.broadcast_committed(ready.event);
    if let Err(error) = core
        .start_intern_provider(
            ready.value.id.clone(),
            runtime_id,
            if request.target.mode == "sync" {
                RuntimeKind::Sync
            } else {
                RuntimeKind::Async
            },
            Some(title),
        )
        .await
    {
        fail_session(core, ready.value.id, error.to_string()).await?;
        return Err(error);
    }
    Ok(ready.value.into())
}

async fn existing_async_binding(core: &CoreRuntime) -> Result<Option<SessionRecord>> {
    Ok(core
        .sessions()
        .list(2_000)
        .await?
        .into_iter()
        .find(|session| {
            session.remote_id.is_some()
                && session.metadata.get("runtime").and_then(Value::as_str) == Some("rust-intern")
                && session
                    .metadata
                    .get("internTransport")
                    .and_then(Value::as_str)
                    != Some("demo")
                && session.kind == SessionKind::Intern.as_str()
                && (session.target_json.get("mode").and_then(Value::as_str) == Some("async")
                    || session
                        .target_json
                        .pointer("/intern/runtimeKind")
                        .and_then(Value::as_str)
                        == Some("async"))
        }))
}

pub async fn send(
    core: &CoreRuntime,
    request: InternSessionSendRequest,
) -> Result<InternSendResult> {
    if request.body.trim().is_empty() {
        bail!("message body is required");
    }
    let session = core
        .sessions()
        .get(request.session_id.clone())
        .await?
        .context("Intern session not found")?;
    let (mode, runtime_id) = intern_identity(&session)?;
    // Configuration is checked before accepting durable work. Network/HTTP
    // failures happen after acceptance and therefore receive failure receipts.
    let client = core.intern().client().await?;
    let command_id = format!("cmd_{}", Uuid::new_v4().simple());
    let run_id = format!("run_{}", Uuid::new_v4().simple());
    let run = core
        .runs()
        .start(RunCreate {
            id: run_id.clone(),
            session_id: session.id.clone(),
            mode: mode.into(),
            model: None,
            adapter: None,
            metadata: json!({"commandId": command_id}),
            source: EventSource::Intern,
        })
        .await?;
    core.broadcast_committed(run.event);
    let accepted = core
        .runs()
        .accept_command(CommandReceiptInput {
            command_id: command_id.clone(),
            session_id: session.id.clone(),
            run_id: Some(run_id.clone()),
            source: EventSource::Intern,
            kind: "message".into(),
            request: json!({"body": request.body}),
        })
        .await?;
    core.broadcast_committed(accepted.event);
    let expected = u64::try_from(session.state_generation.unwrap_or(0)).unwrap_or(0);
    let remote = if mode == "sync" {
        client
            .command_sync(
                &runtime_id,
                &SyncCommandRequest::operator_message(
                    command_id.clone(),
                    command_id.clone(),
                    expected,
                    request.body,
                ),
            )
            .await
    } else {
        client
            .send_async(&AsyncCommandRequest::message(
                command_id.clone(),
                command_id.clone(),
                expected,
                request.body,
                Map::new(),
            ))
            .await
    };
    finish_command(core, command_id, run_id.clone(), remote).await?;
    Ok(InternSendResult { run_id })
}

pub async fn control(
    core: &CoreRuntime,
    request: InternSessionControlRequest,
) -> Result<InternControlResult> {
    let session = core
        .sessions()
        .get(request.session_id.clone())
        .await?
        .context("Intern session not found")?;
    let (mode, runtime_id) = intern_identity(&session)?;
    let client = core.intern().client().await?;
    let supported = if mode == "sync" {
        matches!(
            request.kind.as_str(),
            "pause" | "resume" | "close" | "cancel"
        )
    } else {
        matches!(
            request.kind.as_str(),
            "pause" | "resume" | "close" | "cancel" | "request_checkpoint"
        )
    };
    if !supported {
        bail!("{} is not supported for {mode} Intern", request.kind);
    }
    let command_id = format!("cmd_{}", Uuid::new_v4().simple());
    let accepted = core
        .runs()
        .accept_command(CommandReceiptInput {
            command_id: command_id.clone(),
            session_id: session.id.clone(),
            run_id: session.active_run_id.clone(),
            source: EventSource::Intern,
            kind: request.kind.clone(),
            request: Value::Object(request.payload.clone()),
        })
        .await?;
    core.broadcast_committed(accepted.event);
    let expected = u64::try_from(session.state_generation.unwrap_or(0)).unwrap_or(0);
    let remote = if mode == "sync" {
        let kind = match request.kind.as_str() {
            "pause" => SyncCommandKind::Pause,
            "resume" => SyncCommandKind::Resume,
            "close" | "cancel" => SyncCommandKind::Close,
            _ => unreachable!("control kind validated above"),
        };
        let mut payload = request.payload;
        if kind == SyncCommandKind::Pause {
            payload
                .entry("reason")
                .or_insert_with(|| json!("operator pause"));
        }
        if kind == SyncCommandKind::Close {
            payload.entry("outcome").or_insert_with(|| json!("closed"));
            payload
                .entry("reason")
                .or_insert_with(|| json!("operator close"));
        }
        client
            .command_sync(
                &runtime_id,
                &SyncCommandRequest {
                    command_id: command_id.clone(),
                    idempotency_key: command_id.clone(),
                    expected_generation: expected,
                    command_kind: kind,
                    payload,
                    execution_mode: Default::default(),
                    mode: "sync".into(),
                    evidence_refs: vec![],
                },
            )
            .await
    } else {
        let kind = match request.kind.as_str() {
            "pause" => AsyncCommandKind::Pause,
            "resume" => AsyncCommandKind::Resume,
            "cancel" | "close" => AsyncCommandKind::Cancel,
            "request_checkpoint" => AsyncCommandKind::RequestCheckpoint,
            _ => unreachable!("control kind validated above"),
        };
        let mut payload = request.payload;
        if matches!(kind, AsyncCommandKind::Pause | AsyncCommandKind::Cancel) {
            payload
                .entry("reason")
                .or_insert_with(|| json!("operator request"));
        }
        client
            .command_async(&AsyncCommandRequest {
                command_id: command_id.clone(),
                idempotency_key: command_id.clone(),
                expected_generation: expected,
                command_kind: kind,
                payload,
            })
            .await
    };
    let receipt = match remote {
        Ok(receipt) => receipt,
        Err(error) => {
            let resolved = core
                .runs()
                .resolve_command(
                    command_id,
                    "failed".into(),
                    json!({"error": error.to_string()}),
                    None,
                )
                .await?;
            core.broadcast_committed(resolved.event);
            return Err(error.into());
        }
    };
    let resolved = core
        .runs()
        .resolve_command(
            command_id,
            receipt.local_terminal_status()?.into(),
            serde_json::to_value(&receipt)?,
            Some(i64::try_from(receipt.state_generation)?),
        )
        .await?;
    core.broadcast_committed(resolved.event);
    let accepted = resolved.value.status == "completed";
    Ok(InternControlResult { accepted, receipt })
}

async fn fail_session(core: &CoreRuntime, session_id: String, error: String) -> Result<()> {
    let failed = core
        .sessions()
        .transition(
            session_id,
            SessionStatus::Failed,
            EventSource::Intern,
            json!({"error": error}),
        )
        .await?;
    core.broadcast_committed(failed.event);
    Ok(())
}

async fn finish_command(
    core: &CoreRuntime,
    command_id: String,
    run_id: String,
    remote: std::result::Result<CommandReceipt, crate::cloud::intern::InternClientError>,
) -> Result<()> {
    match remote {
        Ok(receipt) => {
            let terminal_status = receipt.local_terminal_status()?;
            let response = serde_json::to_value(&receipt)?;
            let resolved = core
                .runs()
                .resolve_command(
                    command_id,
                    terminal_status.into(),
                    response.clone(),
                    Some(i64::try_from(receipt.state_generation)?),
                )
                .await?;
            core.broadcast_committed(resolved.event);
            let (run_status, outcome) = if terminal_status == "completed" {
                (RunStatus::Completed, None)
            } else {
                (
                    RunStatus::Failed,
                    Some(json!({"reason": "remote_command_rejected", "receipt": response})),
                )
            };
            let run = core
                .runs()
                .transition(run_id, run_status, outcome, EventSource::Intern)
                .await?;
            core.broadcast_committed(run.event);
            if terminal_status == "completed" {
                Ok(())
            } else {
                bail!(
                    "Intern command was {} ({})",
                    receipt.status,
                    receipt.decision_code
                )
            }
        }
        Err(error) => {
            let resolved = core
                .runs()
                .resolve_command(
                    command_id,
                    "failed".into(),
                    json!({"error": error.to_string()}),
                    None,
                )
                .await?;
            core.broadcast_committed(resolved.event);
            let run = core
                .runs()
                .transition(
                    run_id,
                    RunStatus::Failed,
                    Some(json!({"error": error.to_string()})),
                    EventSource::Intern,
                )
                .await?;
            core.broadcast_committed(run.event);
            Err(error.into())
        }
    }
}

fn intern_identity(session: &SessionRecord) -> Result<(&str, String)> {
    let mode = session
        .target_json
        .get("mode")
        .and_then(Value::as_str)
        .context("Intern session mode is missing")?;
    if !matches!(mode, "sync" | "async") {
        bail!("invalid Intern session mode");
    }
    let remote = session
        .remote_id
        .clone()
        .context("Intern remote identity is missing")?;
    Ok((mode, remote))
}

impl From<SessionRecord> for InternSessionWire {
    fn from(value: SessionRecord) -> Self {
        let status = match SessionStatus::parse(&value.status).unwrap_or(SessionStatus::Ready) {
            SessionStatus::Interrupted => "paused".to_owned(),
            SessionStatus::Closed => "completed".to_owned(),
            other => other.as_str().to_owned(),
        };
        Self {
            id: value.id,
            title: value.title,
            target: value.target_json,
            project_id: value.project_id,
            remote_id: value.remote_id,
            created_at: value.created_at,
            updated_at: value.updated_at,
            status,
            state_generation: value.state_generation,
            latest_cursor: value.latest_cursor,
            active_run_id: value.active_run_id,
            metadata: value.metadata,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::intern::InternRuntime;
    use std::{
        sync::{
            atomic::{AtomicBool, AtomicU8, Ordering},
            Arc,
        },
        time::Duration,
    };
    use tempfile::tempdir;
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        task::JoinHandle,
    };

    struct MockIntern {
        url: String,
        fail_commands: Arc<AtomicBool>,
        command_outcome: Arc<AtomicU8>,
        task: JoinHandle<()>,
    }

    impl MockIntern {
        async fn start() -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let fail_commands = Arc::new(AtomicBool::new(false));
            let failure = fail_commands.clone();
            let command_outcome = Arc::new(AtomicU8::new(0));
            let outcome = command_outcome.clone();
            let task = tokio::spawn(async move {
                while let Ok((stream, _)) = listener.accept().await {
                    let failure = failure.clone();
                    let outcome = outcome.clone();
                    tokio::spawn(async move {
                        let _ = respond(
                            stream,
                            failure.load(Ordering::SeqCst),
                            outcome.load(Ordering::SeqCst),
                        )
                        .await;
                    });
                }
            });
            Self {
                url,
                fail_commands,
                command_outcome,
                task,
            }
        }

        fn runtime(&self) -> InternRuntime {
            InternRuntime::configured(&self.url, "test-key", Duration::from_secs(2)).unwrap()
        }
    }

    impl Drop for MockIntern {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn respond(
        mut stream: TcpStream,
        fail_commands: bool,
        command_outcome: u8,
    ) -> std::io::Result<()> {
        let mut bytes = Vec::new();
        let mut chunk = [0_u8; 4096];
        let (header_end, content_length) = loop {
            let count = stream.read(&mut chunk).await?;
            if count == 0 {
                return Ok(());
            }
            bytes.extend_from_slice(&chunk[..count]);
            if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                let end = end + 4;
                let headers = String::from_utf8_lossy(&bytes[..end]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.split_once(':').and_then(|(name, value)| {
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                    })
                    .unwrap_or(0);
                break (end, length);
            }
        };
        while bytes.len() < header_end + content_length {
            let count = stream.read(&mut chunk).await?;
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..count]);
        }
        let head = String::from_utf8_lossy(&bytes[..header_end]);
        let request_line = head.lines().next().unwrap_or_default();
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or_default();
        let path = parts.next().unwrap_or_default();
        let request_body =
            serde_json::from_slice::<Value>(&bytes[header_end..]).unwrap_or_else(|_| json!({}));

        let (status, body) = if method == "POST"
            && path.ends_with("/sync-sessions")
            && request_body
                .get("objective")
                .and_then(Value::as_str)
                .is_some_and(|value| !value.trim().is_empty())
        {
            (
                "200 OK",
                json!({
                    "sync_session_id": "remote-sync-1",
                    "status": "ready",
                    "state_generation": 1,
                    "last_event_sequence": 0
                }),
            )
        } else if method == "POST" && path.ends_with("/async/ensure") {
            (
                "200 OK",
                json!({
                    "async_runtime_id": "org-async-singleton",
                    "async_assignment_id": "org-async-singleton",
                    "status": "ready",
                    "state_generation": 1,
                    "last_event_sequence": 0
                }),
            )
        } else if method == "POST" && path.ends_with("/commands") && fail_commands {
            ("503 Service Unavailable", json!({"detail":"mock outage"}))
        } else if method == "POST" && path.ends_with("/commands") {
            let command_id = request_body
                .get("command_id")
                .and_then(Value::as_str)
                .unwrap_or("missing");
            let (receipt_status, decision_code) = match command_outcome {
                1 => ("refused", "policy_refused"),
                2 => ("conflict", "generation_conflict"),
                _ => ("applied", "applied"),
            };
            (
                "200 OK",
                json!({
                    "schema_version": "smr.intern-runtime-command-receipt.v1",
                    "command_id": command_id,
                    "runtime_kind": "sync",
                    "runtime_id": "remote-sync-1",
                    "status": receipt_status,
                    "previous_generation": 1,
                    "state_generation": 2,
                    "decision_code": decision_code,
                    "created_at": "2026-08-09T00:00:00Z",
                    "duplicate": false
                }),
            )
        } else if method == "GET" && path.contains("/events") {
            ("200 OK", json!([]))
        } else if method == "GET" && path.contains("/sync-sessions/") {
            (
                "200 OK",
                json!({
                    "sync_session_id": "remote-sync-1",
                    "status": "ready",
                    "state_generation": 1,
                    "last_event_sequence": 0
                }),
            )
        } else {
            ("404 Not Found", json!({"detail":"unexpected mock route"}))
        };
        let encoded = serde_json::to_vec(&body).unwrap();
        let response = format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            encoded.len()
        );
        stream.write_all(response.as_bytes()).await?;
        stream.write_all(&encoded).await?;
        stream.shutdown().await
    }

    fn sync_create() -> InternSessionCreateRequest {
        InternSessionCreateRequest {
            target: InternTarget {
                kind: "intern".into(),
                mode: "sync".into(),
                binding: Some(json!({"projectId":"project-1"})),
            },
            objective: "Inspect the rollout and report the safest next step".into(),
            title: Some("Mock Intern".into()),
            project_id: None,
        }
    }

    fn async_create() -> InternSessionCreateRequest {
        InternSessionCreateRequest {
            target: InternTarget {
                kind: "intern".into(),
                mode: "async".into(),
                binding: Some(json!({"projectId":"project-1"})),
            },
            objective: "Continuously inspect evaluation regressions".into(),
            title: Some("Async singleton".into()),
            project_id: None,
        }
    }

    #[tokio::test]
    async fn create_send_control_persist_and_broadcast() {
        let mock = MockIntern::start().await;
        let dir = tempdir().unwrap();
        let core = CoreRuntime::open_with_intern(dir.path(), mock.runtime()).unwrap();
        let mut live = core.subscribe();

        let session = create(&core, sync_create()).await.unwrap();
        assert_eq!(session.remote_id.as_deref(), Some("remote-sync-1"));
        assert_eq!(session.status, "ready");
        let send_result = send(
            &core,
            InternSessionSendRequest {
                session_id: session.id.clone(),
                body: "inspect the rollout".into(),
            },
        )
        .await
        .unwrap();
        assert!(send_result.run_id.starts_with("run_"));
        let control_result = control(
            &core,
            InternSessionControlRequest {
                session_id: session.id.clone(),
                kind: "pause".into(),
                payload: Map::new(),
            },
        )
        .await
        .unwrap();
        assert!(control_result.accepted);
        assert_eq!(control_result.receipt.status, "applied");

        let persisted = list(&core).await.unwrap();
        assert_eq!(persisted.len(), 1);
        assert!(core
            .runs()
            .get(send_result.run_id)
            .await
            .unwrap()
            .is_some_and(|run| run.status == "completed"));
        let mut kinds = Vec::new();
        while let Ok(event) = live.try_recv() {
            kinds.push(event.kind);
        }
        assert!(kinds.contains(&"session.created".to_owned()));
        assert!(kinds.contains(&"run.started".to_owned()));
        assert!(kinds.contains(&"command.resolved".to_owned()));
    }

    #[tokio::test]
    async fn repeated_async_create_reuses_the_local_singleton_binding() {
        let mock = MockIntern::start().await;
        let dir = tempdir().unwrap();
        let core = CoreRuntime::open_with_intern(dir.path(), mock.runtime()).unwrap();
        let first = create(&core, async_create()).await.unwrap();
        let second = create(&core, async_create()).await.unwrap();
        assert_eq!(first.id, second.id);
        assert_eq!(first.remote_id.as_deref(), Some("org-async-singleton"));
        assert_eq!(list(&core).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn migrated_demo_async_session_does_not_mask_the_live_singleton() {
        let mock = MockIntern::start().await;
        let dir = tempdir().unwrap();
        let core = CoreRuntime::open_with_intern(dir.path(), mock.runtime()).unwrap();
        core.sessions()
            .create_or_update(SessionCreate {
                id: "legacy-demo-async".into(),
                title: "Async Intern".into(),
                kind: SessionKind::Intern,
                target: json!({"kind":"intern","mode":"async"}),
                project_id: None,
                remote_id: Some("demo-async-org-singleton".into()),
                codex_thread_id: None,
                status: SessionStatus::Ready,
                state_generation: None,
                metadata: json!({"internTransport":"demo"}),
                source: EventSource::Intern,
            })
            .await
            .unwrap();

        let live = create(&core, async_create()).await.unwrap();
        assert_ne!(live.id, "legacy-demo-async");
        assert_eq!(live.remote_id.as_deref(), Some("org-async-singleton"));
        assert_eq!(list(&core).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn http_failure_resolves_receipt_and_run() {
        let mock = MockIntern::start().await;
        let dir = tempdir().unwrap();
        let core = CoreRuntime::open_with_intern(dir.path(), mock.runtime()).unwrap();
        let session = create(&core, sync_create()).await.unwrap();
        mock.fail_commands.store(true, Ordering::SeqCst);

        assert!(send(
            &core,
            InternSessionSendRequest {
                session_id: session.id.clone(),
                body: "fail this".into(),
            }
        )
        .await
        .is_err());
        assert!(control(
            &core,
            InternSessionControlRequest {
                session_id: session.id.clone(),
                kind: "close".into(),
                payload: Map::new(),
            }
        )
        .await
        .is_err());
        let persisted = core.sessions().get(session.id).await.unwrap().unwrap();
        assert!(persisted.active_run_id.is_none());
        assert_eq!(persisted.status, "failed");
        let (failed_receipts, failed_runs): (i64, i64) = core
            .storage()
            .database()
            .with_conn(|conn| {
                Ok((
                    conn.query_row(
                        "SELECT COUNT(*) FROM command_receipts WHERE status = 'failed'",
                        [],
                        |row| row.get(0),
                    )?,
                    conn.query_row(
                        "SELECT COUNT(*) FROM runs WHERE status = 'failed'",
                        [],
                        |row| row.get(0),
                    )?,
                ))
            })
            .unwrap();
        assert_eq!((failed_receipts, failed_runs), (2, 1));
    }

    #[tokio::test]
    async fn semantic_rejections_reject_receipts_and_fail_message_runs() {
        let mock = MockIntern::start().await;
        let dir = tempdir().unwrap();
        let core = CoreRuntime::open_with_intern(dir.path(), mock.runtime()).unwrap();
        let session = create(&core, sync_create()).await.unwrap();

        mock.command_outcome.store(1, Ordering::SeqCst);
        let control_result = control(
            &core,
            InternSessionControlRequest {
                session_id: session.id.clone(),
                kind: "pause".into(),
                payload: Map::new(),
            },
        )
        .await
        .unwrap();
        assert!(!control_result.accepted);
        assert_eq!(control_result.receipt.status, "refused");

        mock.command_outcome.store(2, Ordering::SeqCst);
        let error = send(
            &core,
            InternSessionSendRequest {
                session_id: session.id.clone(),
                body: "conflict this message".into(),
            },
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("conflict"));

        let persisted = core.sessions().get(session.id).await.unwrap().unwrap();
        assert_eq!(persisted.status, "failed");
        assert!(persisted.active_run_id.is_none());
        let (rejected_receipts, failed_runs, semantic_responses): (i64, i64, i64) = core
            .storage()
            .database()
            .with_conn(|conn| {
                Ok((
                    conn.query_row(
                        "SELECT COUNT(*) FROM command_receipts WHERE status = 'rejected'",
                        [],
                        |row| row.get(0),
                    )?,
                    conn.query_row(
                        "SELECT COUNT(*) FROM runs WHERE status = 'failed'",
                        [],
                        |row| row.get(0),
                    )?,
                    conn.query_row(
                        "SELECT COUNT(*) FROM command_receipts
                         WHERE json_extract(response_json, '$.status') IN ('refused', 'conflict')",
                        [],
                        |row| row.get(0),
                    )?,
                ))
            })
            .unwrap();
        assert_eq!(
            (rejected_receipts, failed_runs, semantic_responses),
            (2, 1, 2)
        );
    }

    #[tokio::test]
    async fn restart_lists_and_reattaches_durable_session() {
        let mock = MockIntern::start().await;
        let dir = tempdir().unwrap();
        let first = CoreRuntime::open_with_intern(dir.path(), mock.runtime()).unwrap();
        let created = create(&first, sync_create()).await.unwrap();
        // Simulate process shutdown before constructing the deliberately
        // in-flight durable command state used by the restart assertion.
        first.stop_intern_providers_for_test().await.unwrap();
        let command_id = "command-before-restart".to_owned();
        let run_id = "run-before-restart".to_owned();
        let run = first
            .runs()
            .start(RunCreate {
                id: run_id.clone(),
                session_id: created.id.clone(),
                mode: "sync".into(),
                model: None,
                adapter: None,
                metadata: json!({"commandId": command_id}),
                source: EventSource::Intern,
            })
            .await
            .unwrap();
        first.broadcast_committed(run.event);
        let receipt = first
            .runs()
            .accept_command(CommandReceiptInput {
                command_id: command_id.clone(),
                session_id: created.id.clone(),
                run_id: Some(run_id.clone()),
                source: EventSource::Intern,
                kind: "message".into(),
                request: json!({"body":"in flight"}),
            })
            .await
            .unwrap();
        first.broadcast_committed(receipt.event);

        let restarted = CoreRuntime::open_with_intern(dir.path(), mock.runtime()).unwrap();
        let restored = list(&restarted).await.unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].id, created.id);
        assert_eq!(restarted.resume_intern_providers().await.unwrap(), 1);
        assert_eq!(
            restarted.runs().get(run_id).await.unwrap().unwrap().status,
            "interrupted"
        );
        assert_eq!(
            restarted
                .runs()
                .command_receipt(command_id)
                .await
                .unwrap()
                .unwrap()
                .status,
            "failed"
        );
        assert!(restarted
            .sessions()
            .get(created.id)
            .await
            .unwrap()
            .unwrap()
            .active_run_id
            .is_none());
    }

    #[tokio::test]
    async fn malformed_binding_does_not_create_local_session() {
        let mock = MockIntern::start().await;
        let dir = tempdir().unwrap();
        let core = CoreRuntime::open_with_intern(dir.path(), mock.runtime()).unwrap();
        let mut request = sync_create();
        request.target.binding = Some(json!("not-an-object"));
        assert!(create(&core, request).await.is_err());
        assert!(list(&core).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn empty_objective_does_not_create_local_or_remote_session() {
        let mock = MockIntern::start().await;
        let dir = tempdir().unwrap();
        let core = CoreRuntime::open_with_intern(dir.path(), mock.runtime()).unwrap();
        let mut request = sync_create();
        request.objective = "   ".into();
        assert!(create(&core, request).await.is_err());
        assert!(list(&core).await.unwrap().is_empty());
    }
}
