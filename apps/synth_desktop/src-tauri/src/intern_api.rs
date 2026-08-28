use crate::cloud::intern::{
    AsyncCommandKind, AsyncCommandRequest, AsyncEnsureRequest, CommandReceipt, RuntimeBinding,
    RuntimeKind, SyncCommandKind, SyncCommandRequest, SyncCreateRequest,
};
use crate::core_runtime::CoreRuntime;
use crate::domain::{
    CommandReceiptInput, InternMode, RunCreate, RunStatus, RuntimeTarget, SessionCreate,
    SessionKind, SessionStatus,
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

#[derive(Clone, Debug, Deserialize, specta::Type)]
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

#[derive(Clone, Debug, Deserialize, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InternTarget {
    pub kind: String,
    pub mode: String,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub binding: Option<Value>,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InternSessionCreateRequest {
    pub target: InternTarget,
    pub objective: String,
    pub title: Option<String>,
    pub project_id: Option<String>,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InternSessionSendRequest {
    pub session_id: String,
    pub body: String,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InternSessionControlRequest {
    pub session_id: String,
    pub kind: String,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub payload: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InternSessionWire {
    pub id: String,
    pub title: String,
    #[specta(type = specta_typescript::Unknown)]
    pub target: Value,
    pub project_id: Option<String>,
    pub remote_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub status: String,
    #[specta(type = specta_typescript::Number)]
    pub state_generation: Option<i64>,
    #[specta(type = specta_typescript::Number)]
    pub latest_cursor: i64,
    pub active_run_id: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub metadata: Value,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct InternSendResult {
    pub run_id: String,
}

#[derive(Clone, Debug, Serialize, specta::Type)]
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
    let target = RuntimeTarget::InternRuntime {
        mode: InternMode::parse(&request.target.mode).context("invalid Intern mode")?,
        binding: Some(crate::domain::InternBinding {
            factory_id: binding_request.factory_id.clone(),
            project_id: binding_request.project_id.clone(),
            effort_id: binding_request.effort_id.clone(),
            run_id: binding_request.run_id.clone(),
        }),
    };
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
                && session.target.intern_mode() == Some(InternMode::Async)
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
        .target
        .intern_mode()
        .context("Intern session mode is missing")?
        .as_str();
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
            target: value.target.to_json_value(),
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

