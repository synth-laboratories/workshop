use crate::core_runtime::CoreRuntime;
use crate::domain::{
    RunCreate, RunService, RunStatus, SessionCreate, SessionService, SessionStatus,
    SessionTitleOrigin,
};
use crate::storage::{EventAppend, EventSource};
use crate::synth_config::MultiAgentVersion;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    env, fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
    sync::{oneshot, Mutex, RwLock},
};

const EVENT_NAME: &str = "codex:event";

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionStartRequest {
    pub session_id: String,
    pub workspace: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub provider_name: Option<String>,
    pub provider_title: Option<String>,
    pub provider_env_key: Option<String>,
    pub approval_policy: Option<String>,
    pub sandbox: Option<String>,
    pub thread_id: Option<String>,
    pub multi_agent_version: Option<MultiAgentVersion>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexTurnStartRequest {
    pub session_id: String,
    pub prompt: String,
    pub effort: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionRequest {
    pub session_id: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexApprovalDecisionRequest {
    pub session_id: String,
    pub approval_id: String,
    pub decision: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionInfo {
    pub session_id: String,
    pub thread_id: String,
    pub turn_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionRecord {
    pub session_id: String,
    pub thread_id: String,
    pub workspace: String,
    pub model: String,
    pub provider_name: String,
    pub provider_title: String,
    pub base_url: String,
    pub status: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub title_origin: Option<String>,
    #[serde(default = "default_approval_policy")]
    pub approval_policy: String,
    #[serde(default = "default_sandbox")]
    pub sandbox: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CodexEvent {
    session_id: String,
    method: String,
    params: Value,
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

#[derive(Clone, Debug)]
struct PendingApproval {
    rpc_id: Value,
    available_decisions: Vec<String>,
}

type PendingApprovals = Arc<Mutex<HashMap<String, PendingApproval>>>;

struct AppServer {
    child: Mutex<Child>,
    stdin: Arc<Mutex<tokio::process::ChildStdin>>,
    pending: Pending,
    approvals: PendingApprovals,
    next_id: AtomicU64,
}

impl Drop for AppServer {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.try_lock() {
            let _ = child.start_kill();
        }
    }
}

impl AppServer {
    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        if let Err(error) = write_message(
            &self.stdin,
            &json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}),
        )
        .await
        {
            self.pending.lock().await.remove(&id);
            return Err(error);
        }
        match tokio::time::timeout(Duration::from_secs(30), rx).await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(error))) => Err(anyhow!("codex app-server {method} error: {error}")),
            Ok(Err(_)) => Err(anyhow!("codex app-server stopped while handling {method}")),
            Err(_) => {
                self.pending.lock().await.remove(&id);
                Err(anyhow!("codex app-server timed out waiting for {method}"))
            }
        }
    }

    async fn notify(&self, method: &str) -> Result<()> {
        write_message(&self.stdin, &json!({"jsonrpc":"2.0","method":method})).await
    }

    async fn stop(&self) -> Result<()> {
        self.child
            .lock()
            .await
            .kill()
            .await
            .context("stop app-server")
    }

    async fn resolve_approval(&self, approval_id: &str, requested: &str) -> Result<String> {
        let pending = self
            .approvals
            .lock()
            .await
            .get(approval_id)
            .cloned()
            .ok_or_else(|| anyhow!("approval is no longer pending: {approval_id}"))?;
        let decision = select_approval_decision(&pending.available_decisions, requested)?;
        write_message(
            &self.stdin,
            &json!({"jsonrpc":"2.0","id":pending.rpc_id,"result":{"decision":decision}}),
        )
        .await?;
        self.approvals.lock().await.remove(approval_id);
        Ok(decision)
    }
}

fn select_approval_decision(available: &[String], requested: &str) -> Result<String> {
    let candidates: &[&str] = match requested {
        "once" => &["accept", "approve", "allow", "yes"],
        "always" => &["acceptForSession", "allowForSession", "always"],
        "reject" => &["decline", "reject", "deny", "cancel", "no"],
        _ => return Err(anyhow!("unsupported approval decision: {requested}")),
    };
    candidates
        .iter()
        .find(|candidate| available.iter().any(|value| value == **candidate))
        .copied()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("the app-server does not support {requested} for this request"))
}

fn default_approval_policy() -> String {
    "untrusted".into()
}
fn default_sandbox() -> String {
    "workspace-write".into()
}

struct Session {
    attachment_id: uuid::Uuid,
    server: Arc<AppServer>,
    thread_id: String,
    turn_id: RwLock<Option<String>>,
    model: String,
    approval_policy: String,
}

pub struct CodexManager {
    sessions: Arc<RwLock<HashMap<String, Arc<Session>>>>,
    records: Arc<RwLock<HashMap<String, CodexSessionRecord>>>,
    root: PathBuf,
    state_path: PathBuf,
    binary: PathBuf,
    core: Option<Arc<CoreRuntime>>,
}

impl CodexManager {
    pub fn new(core: Option<Arc<CoreRuntime>>) -> Self {
        let root = codex_root();
        let binary = PathBuf::from(env::var("SYNTH_CODEX_BIN").unwrap_or_else(|_| "codex".into()));
        Self::with_paths(core, root, binary)
    }

    fn with_paths(core: Option<Arc<CoreRuntime>>, root: PathBuf, binary: PathBuf) -> Self {
        let state_path = root.join("threads.json");
        let records = fs::read_to_string(&state_path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            records: Arc::new(RwLock::new(records)),
            root,
            state_path,
            binary,
            core,
        }
    }

    pub async fn start<R: tauri::Runtime>(
        &self,
        app: AppHandle<R>,
        request: CodexSessionStartRequest,
    ) -> Result<CodexSessionInfo> {
        validate_start(&request)?;
        if let Some(existing) = self.sessions.read().await.get(&request.session_id) {
            return Ok(session_info(&request.session_id, existing).await);
        }
        let home = self
            .root
            .join("homes")
            .join(safe_component(&request.session_id));
        ensure_home(&home, &request)?;
        let attachment_id = uuid::Uuid::new_v4();
        let server = spawn_server(
            app.clone(),
            &self.binary,
            &request.session_id,
            &home,
            &request,
            self.records.clone(),
            self.state_path.clone(),
            self.core.clone(),
            self.sessions.clone(),
            attachment_id,
        )
        .await?;
        let initialize_params = json!({"clientInfo":{"name":"synth-desktop","title":crate::instance::display_name(),"version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}});
        let mut initialize_attempts = 0;
        loop {
            match server
                .request("initialize", initialize_params.clone())
                .await
            {
                Ok(_) => break,
                Err(error)
                    if error.to_string().contains("database is locked")
                        && initialize_attempts < 5 =>
                {
                    initialize_attempts += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(200 * initialize_attempts))
                        .await;
                }
                Err(error) => return Err(error),
            }
        }
        server.notify("initialized").await?;
        let remembered = self.records.read().await.get(&request.session_id).cloned();
        let requested_thread = request
            .thread_id
            .clone()
            .or_else(|| remembered.as_ref().map(|record| record.thread_id.clone()));
        let method = if requested_thread.is_some() {
            "thread/resume"
        } else {
            "thread/start"
        };
        let mut params = json!({
            "model": request.model,
            "cwd": request.workspace,
            "approvalPolicy": request.approval_policy.as_deref().unwrap_or("untrusted"),
            "sandbox": request.sandbox.as_deref().unwrap_or("workspace-write")
        });
        if let Some(thread_id) = requested_thread {
            params["threadId"] = Value::String(thread_id);
        }
        let mut attempts = 0;
        let result = loop {
            match server.request(method, params.clone()).await {
                Ok(result) => break result,
                Err(error) if error.to_string().contains("database is locked") && attempts < 5 => {
                    attempts += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(200 * attempts)).await;
                }
                Err(error) => return Err(error),
            }
        };
        let thread_id = nested_id(&result, "threadId")
            .ok_or_else(|| anyhow!("Codex {method} response missing thread id: {result}"))?;
        let session = Arc::new(Session {
            attachment_id,
            server,
            thread_id: thread_id.clone(),
            turn_id: RwLock::new(None),
            model: request.model.clone(),
            approval_policy: request
                .approval_policy
                .clone()
                .unwrap_or_else(default_approval_policy),
        });
        self.sessions
            .write()
            .await
            .insert(request.session_id.clone(), session.clone());
        let default_title = if request.provider_name.as_deref() == Some("local-laguna") {
            "Laguna XS".to_owned()
        } else {
            request.model.clone()
        };
        let title = remembered
            .as_ref()
            .and_then(|record| record.title.clone())
            .unwrap_or(default_title);
        let title_origin = remembered
            .as_ref()
            .and_then(|record| record.title_origin.clone())
            .unwrap_or_else(|| "default".into());
        self.records.write().await.insert(
            request.session_id.clone(),
            CodexSessionRecord {
                session_id: request.session_id.clone(),
                thread_id: thread_id.clone(),
                workspace: request.workspace.clone(),
                model: session.model.clone(),
                provider_name: request.provider_name.unwrap_or_else(|| "custom".into()),
                provider_title: request
                    .provider_title
                    .unwrap_or_else(|| "Synth Responses Provider".into()),
                base_url: responses_base_url(&request.base_url),
                status: "ready".into(),
                title: Some(title.clone()),
                title_origin: Some(title_origin.clone()),
                approval_policy: request
                    .approval_policy
                    .clone()
                    .unwrap_or_else(default_approval_policy),
                sandbox: request.sandbox.clone().unwrap_or_else(default_sandbox),
            },
        );
        self.persist_records().await?;
        if let Some(core) = &self.core {
            let service = SessionService::new(core.storage().database().clone());
            let persistence = service.create_or_update(SessionCreate {
                id: request.session_id.clone(),
                title,
                target: json!({
                    "kind": "codex",
                    "model": session.model,
                    "workspace": request.workspace,
                    "threadId": thread_id,
                }),
                project_id: None,
                remote_id: None,
                codex_thread_id: Some(thread_id.clone()),
                status: SessionStatus::Ready,
                state_generation: None,
                metadata: json!({
                    "workspace": request.workspace,
                    "model": session.model,
                    "approvalPolicy": request.approval_policy.clone().unwrap_or_else(default_approval_policy),
                    "sandbox": request.sandbox.clone().unwrap_or_else(default_sandbox),
                    "titleOrigin": title_origin,
                }),
                source: EventSource::Codex,
            });
            if let Ok(Ok(mutation)) =
                tokio::time::timeout(std::time::Duration::from_secs(2), persistence).await
            {
                if let Some(event) = mutation.event {
                    let _ = core.publish_event(&app, event).await;
                }
            }
        }
        Ok(session_info(&request.session_id, &session).await)
    }

    pub async fn start_turn<R: tauri::Runtime>(
        &self,
        app: AppHandle<R>,
        request: CodexTurnStartRequest,
    ) -> Result<CodexSessionInfo> {
        if request.prompt.trim().is_empty() {
            return Err(anyhow!("prompt must not be empty"));
        }
        let effort = request
            .effort
            .as_deref()
            .map(validate_reasoning_effort)
            .transpose()?;
        let session = self.session(&request.session_id).await?;
        if let Some(core) = &self.core {
            let persistence = core.append_and_emit(
                &app,
                EventAppend::codex(
                    request.session_id.clone(),
                    "message.created",
                    json!({
                        "messageId": format!("user-{}", uuid::Uuid::new_v4()),
                        "role": "user",
                        "content": request.prompt.clone(),
                    }),
                ),
            );
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), persistence).await;
        }
        let mut turn_params = json!({
            "threadId": session.thread_id,
            "model": session.model,
            "input":[{"type":"text","text":request.prompt,"textElements":[]}],
            "approvalPolicy": session.approval_policy
        });
        if let Some(effort) = effort {
            turn_params["effort"] = Value::String(effort.to_owned());
        }
        let result = session.server.request("turn/start", turn_params).await?;
        let turn_id = nested_id(&result, "turnId")
            .ok_or_else(|| anyhow!("Codex turn/start response missing turn id: {result}"))?;
        *session.turn_id.write().await = Some(turn_id.clone());
        self.set_automatic_title(&app, &request.session_id, &request.prompt, &session)
            .await;
        if let Some(core) = &self.core {
            let service = RunService::new(core.storage().database().clone());
            let persistence = service.start(RunCreate {
                id: turn_id,
                session_id: request.session_id.clone(),
                mode: "codex_turn".into(),
                model: Some(session.model.clone()),
                adapter: None,
                metadata: json!({"threadId": session.thread_id, "effort": effort}),
                source: EventSource::Codex,
            });
            if let Ok(Ok(mutation)) =
                tokio::time::timeout(std::time::Duration::from_secs(2), persistence).await
            {
                if let Some(event) = mutation.event {
                    let _ = core.publish_event(&app, event).await;
                }
            }
        }
        self.set_status(&request.session_id, "running").await?;
        Ok(session_info(&request.session_id, &session).await)
    }

    pub async fn interrupt(&self, session_id: &str) -> Result<()> {
        let Some(session) = self.sessions.read().await.get(session_id).cloned() else {
            // Stop is idempotent. A persisted running record can outlive its
            // in-memory app-server after a desktop restart or process crash.
            self.mark_detached_turn_interrupted(session_id).await?;
            return Ok(());
        };
        let Some(turn_id) = session.turn_id.read().await.clone() else {
            self.mark_detached_turn_interrupted(session_id).await?;
            return Ok(());
        };
        session
            .server
            .request(
                "turn/interrupt",
                json!({"threadId":session.thread_id,"turnId":turn_id}),
            )
            .await?;
        Ok(())
    }

    pub async fn close(&self, session_id: &str) -> Result<()> {
        let session = self.sessions.write().await.remove(session_id);
        if let Some(session) = session {
            session.server.stop().await?;
        }
        self.set_status(session_id, "closed").await?;
        Ok(())
    }

    pub async fn resolve_approval<R: tauri::Runtime>(
        &self,
        app: AppHandle<R>,
        request: CodexApprovalDecisionRequest,
    ) -> Result<()> {
        let session = self.session(&request.session_id).await?;
        let decision = session
            .server
            .resolve_approval(&request.approval_id, &request.decision)
            .await?;
        let kind = if request.decision == "reject" {
            "approval.rejected"
        } else {
            "approval.granted"
        };
        let payload = json!({
            "approvalId": request.approval_id,
            "decision": request.decision,
            "appServerDecision": decision,
        });
        let _ = app.emit(
            EVENT_NAME,
            CodexEvent {
                session_id: request.session_id.clone(),
                method: kind.into(),
                params: payload.clone(),
            },
        );
        if let Some(core) = &self.core {
            let _ = core
                .append_and_emit(&app, EventAppend::codex(request.session_id, kind, payload))
                .await;
        }
        Ok(())
    }

    pub async fn list(&self) -> Vec<CodexSessionRecord> {
        let attached = self.sessions.read().await;
        let mut changed = false;
        let mut interrupted = Vec::new();
        {
            let mut records = self.records.write().await;
            for record in records.values_mut() {
                let reconciled = reconcile_detached_status(
                    &mut record.status,
                    attached.contains_key(&record.session_id),
                );
                changed |= reconciled;
                if reconciled {
                    interrupted.push(record.session_id.clone());
                }
            }
        }
        drop(attached);
        if changed {
            let _ = self.persist_records().await;
            if let Some(core) = &self.core {
                for session_id in interrupted {
                    let _ = interrupt_active_core_run(core, &session_id, "desktop_restarted").await;
                }
            }
        }
        let mut records: Vec<_> = self.records.read().await.values().cloned().collect();
        records.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        records
    }

    async fn session(&self, id: &str) -> Result<Arc<Session>> {
        self.sessions
            .read()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow!("Codex session not started: {id}"))
    }

    async fn set_status(&self, session_id: &str, status: &str) -> Result<()> {
        if let Some(record) = self.records.write().await.get_mut(session_id) {
            record.status = status.into();
        }
        self.persist_records().await
    }

    async fn mark_detached_turn_interrupted(&self, session_id: &str) -> Result<()> {
        let should_change = self
            .records
            .read()
            .await
            .get(session_id)
            .is_some_and(|record| record.status == "running");
        if should_change {
            self.set_status(session_id, "interrupted").await?;
            if let Some(core) = &self.core {
                let _ = interrupt_active_core_run(core, session_id, "runtime_detached").await;
            }
        }
        Ok(())
    }

    async fn set_automatic_title<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        session_id: &str,
        prompt: &str,
        session: &Session,
    ) {
        let should_set = self
            .records
            .read()
            .await
            .get(session_id)
            .and_then(|record| record.title_origin.as_deref())
            == Some("default");
        if !should_set {
            return;
        }
        let Some(title) = automatic_thread_title(prompt) else {
            return;
        };
        let previous_title = {
            let mut records = self.records.write().await;
            let Some(record) = records.get_mut(session_id) else {
                return;
            };
            let previous = record.title.clone();
            record.title = Some(title.clone());
            record.title_origin = Some("automatic".into());
            previous
        };
        let _ = self.persist_records().await;
        if session
            .server
            .request(
                "thread/name/set",
                json!({"threadId": session.thread_id, "name": title}),
            )
            .await
            .is_err()
        {
            if let Some(record) = self.records.write().await.get_mut(session_id) {
                record.title = previous_title;
                record.title_origin = Some("default".into());
            }
            let _ = self.persist_records().await;
            return;
        }
        if let Some(core) = &self.core {
            let service = SessionService::new(core.storage().database().clone());
            if let Ok(mutation) = service
                .set_title(
                    session_id.to_owned(),
                    title.clone(),
                    SessionTitleOrigin::Automatic,
                )
                .await
            {
                if let Some(event) = mutation.event {
                    let _ = core.publish_event(app, event).await;
                }
            }
        }
    }

    async fn persist_records(&self) -> Result<()> {
        persist_records(&self.records, &self.state_path).await
    }
}

fn reconcile_detached_status(status: &mut String, attached: bool) -> bool {
    if status == "running" && !attached {
        *status = "interrupted".into();
        true
    } else {
        false
    }
}

async fn interrupt_active_core_run(
    core: &CoreRuntime,
    session_id: &str,
    reason: &str,
) -> Result<Option<crate::storage::AppEvent>> {
    let sessions = SessionService::new(core.storage().database().clone());
    let Some(session) = sessions.get(session_id.to_owned()).await? else {
        return Ok(None);
    };
    let Some(run_id) = session.active_run_id else {
        return Ok(None);
    };
    let runs = RunService::new(core.storage().database().clone());
    let mutation = runs
        .transition(
            run_id,
            RunStatus::Interrupted,
            Some(json!({ "reason": reason })),
            EventSource::Codex,
        )
        .await?;
    Ok(mutation.event)
}

async fn spawn_server<R: tauri::Runtime>(
    app: AppHandle<R>,
    binary: &Path,
    session_id: &str,
    home: &Path,
    request: &CodexSessionStartRequest,
    records: Arc<RwLock<HashMap<String, CodexSessionRecord>>>,
    state_path: PathBuf,
    core: Option<Arc<CoreRuntime>>,
    sessions: Arc<RwLock<HashMap<String, Arc<Session>>>>,
    attachment_id: uuid::Uuid,
) -> Result<Arc<AppServer>> {
    let env_key = request
        .provider_env_key
        .as_deref()
        .unwrap_or("SYNTH_LAGUNA_API_KEY");
    let mut command = Command::new(binary);
    command
        .args(["app-server", "--listen", "stdio://"])
        .current_dir(&request.workspace)
        .env("CODEX_HOME", home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if !request.api_key.is_empty() {
        command.env(env_key, &request.api_key);
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
    tauri::async_runtime::spawn(read_stdout(
        app.clone(),
        sid.clone(),
        stdout,
        stdin,
        pending,
        approvals,
        request
            .approval_policy
            .clone()
            .unwrap_or_else(default_approval_policy),
        PersistenceContext {
            records,
            state_path,
            core,
            sessions,
            attachment_id,
        },
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

struct PersistenceContext {
    records: Arc<RwLock<HashMap<String, CodexSessionRecord>>>,
    state_path: PathBuf,
    core: Option<Arc<CoreRuntime>>,
    sessions: Arc<RwLock<HashMap<String, Arc<Session>>>>,
    attachment_id: uuid::Uuid,
}

async fn read_stdout<R: tauri::Runtime>(
    app: AppHandle<R>,
    session_id: String,
    stdout: tokio::process::ChildStdout,
    stdin: Arc<Mutex<tokio::process::ChildStdin>>,
    pending: Pending,
    approvals: PendingApprovals,
    approval_policy: String,
    persistence: PersistenceContext,
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
        let method = message["method"].as_str().unwrap_or_default().to_owned();
        let params = message.get("params").cloned().unwrap_or(Value::Null);
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
                    let response = rejection_response(&available_decisions, rpc_id);
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
                if let Some(core) = &persistence.core {
                    let _ = core
                        .append_and_emit(
                            &app,
                            EventAppend::codex(session_id.clone(), "approval.requested", safe),
                        )
                        .await;
                }
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
        if let Some(core) = &persistence.core {
            let _ = core
                .append_and_emit(
                    &app,
                    EventAppend::codex(session_id.clone(), method.clone(), params.clone()),
                )
                .await;
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
                    if let Some(core) = &persistence.core {
                        let sessions = SessionService::new(core.storage().database().clone());
                        if let Ok(mutation) = sessions
                            .set_title(
                                session_id.clone(),
                                title.to_owned(),
                                SessionTitleOrigin::Manual,
                            )
                            .await
                        {
                            if let Some(event) = mutation.event {
                                let _ = core.publish_event(&app, event).await;
                            }
                        }
                    }
                }
            }
        }
        if matches!(
            method.as_str(),
            "turn/completed" | "turn/failed" | "turn/interrupted"
        ) {
            let status = match method.as_str() {
                "turn/completed" => "ready",
                "turn/failed" => "failed",
                _ => "interrupted",
            };
            if let Some(record) = persistence.records.write().await.get_mut(&session_id) {
                record.status = status.into();
            }
            let _ = persist_records(&persistence.records, &persistence.state_path).await;
            if let Some(core) = &persistence.core {
                let runs = RunService::new(core.storage().database().clone());
                let sessions = SessionService::new(core.storage().database().clone());
                if let Ok(Some(session)) = sessions.get(session_id.clone()).await {
                    if let Some(run_id) = session.active_run_id {
                        let run_status = match method.as_str() {
                            "turn/completed" => RunStatus::Completed,
                            "turn/failed" => RunStatus::Failed,
                            _ => RunStatus::Interrupted,
                        };
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
                                let _ = core.publish_event(&app, event).await;
                            }
                        }
                    }
                }
            }
        }
    }
    let mut pending = pending.lock().await;
    for (_, sender) in pending.drain() {
        let _ = sender.send(Err("codex app-server stdout closed".into()));
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
            if record.status != "running" {
                return false;
            }
            record.status = "interrupted".into();
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
        if let Some(core) = &persistence.core {
            if let Ok(Some(event)) =
                interrupt_active_core_run(core, &session_id, "app_server_exited").await
            {
                let _ = core.publish_event(&app, event).await;
            }
        }
    }
}

fn rejection_response(available: &[String], id: Value) -> Value {
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

fn safe_approval_payload(
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

fn is_approval_method(method: &str) -> bool {
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

async fn write_message(
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

fn ensure_home(home: &Path, request: &CodexSessionStartRequest) -> Result<()> {
    fs::create_dir_all(home.join("sessions"))?;
    let container_skill = home.join("skills/use-synth-containers");
    fs::create_dir_all(&container_skill)?;
    fs::write(
        container_skill.join("SKILL.md"),
        include_str!("../../skills/use-synth-containers/SKILL.md"),
    )?;
    let visuals_skill = home.join("skills/use-synth-visuals");
    fs::create_dir_all(visuals_skill.join("references"))?;
    fs::write(
        visuals_skill.join("SKILL.md"),
        include_str!("../../skills/use-synth-visuals/SKILL.md"),
    )?;
    fs::write(
        visuals_skill.join("references/visual-recipes.md"),
        include_str!("../../skills/use-synth-visuals/references/visual-recipes.md"),
    )?;
    let provider = request.provider_name.as_deref().unwrap_or("custom");
    let title = request
        .provider_title
        .as_deref()
        .unwrap_or("Synth Responses Provider");
    let env_key = request
        .provider_env_key
        .as_deref()
        .unwrap_or("SYNTH_LAGUNA_API_KEY");
    let multi_agent_version = request
        .multi_agent_version
        .unwrap_or(MultiAgentVersion::None);
    let (agents_enabled, multi_agent_v1, multi_agent_v2) = multi_agent_flags(multi_agent_version);
    let allowed_workspace_roots = crate::synth_config::allowed_workspace_roots()?;
    let workspace_write_config = workspace_write_config(&allowed_workspace_roots);
    let config = format!(
        "model = \"{}\"\nmodel_provider = \"{}\"\napproval_policy = \"{}\"\nsandbox_mode = \"{}\"\nservice_tier = \"default\"\n\n{}[model_providers.{}]\nname = \"{}\"\nbase_url = \"{}\"\nenv_key = \"{}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\n\n[agents]\nenabled = {}\n\n[features]\nmulti_agent = {}\nmulti_agent_v2 = {}\ntool_call_mcp_elicitation = false\nshell_tool = true\nunified_exec = true\n",
        toml_string(&request.model), toml_string(provider), toml_string(request.approval_policy.as_deref().unwrap_or("untrusted")), toml_string(request.sandbox.as_deref().unwrap_or("workspace-write")), workspace_write_config, toml_key(provider), toml_string(title), toml_string(&responses_base_url(&request.base_url)), toml_string(env_key), agents_enabled, multi_agent_v1, multi_agent_v2
    );
    fs::write(home.join("config.toml"), config)?;
    let auth = home.join("auth.json");
    if !auth.exists() {
        fs::write(
            auth,
            "{\n  \"OPENAI_API_KEY\": \"synth-desktop-provider\"\n}\n",
        )?;
    }
    // Point Codex at the Rust container and visuals MCP adapters (both forward to CoreRuntime IPC).
    if let Ok(exe) = env::current_exe() {
        let ipc = crate::storage::app_data_root().join("visuals-ipc.json");
        let mut existing = fs::read_to_string(home.join("config.toml")).unwrap_or_default();
        for (server, binary) in [
            ("synth_containers", "synth-containers-mcp"),
            ("synth_visuals", "synth-visuals-mcp"),
        ] {
            let bin = exe
                .parent()
                .map(|dir| dir.join(binary))
                .filter(|path| path.exists())
                .or_else(|| {
                    let candidate = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                        .join(format!("target/debug/{binary}"));
                    candidate.exists().then_some(candidate)
                });
            let Some(bin) = bin else { continue };
            let heading = format!("[mcp_servers.{server}]");
            if existing.contains(&heading) {
                continue;
            }
            existing.push_str(&format!(
                "\n{heading}\ncommand = \"{}\"\nargs = []\n{}default_tools_approval_mode = \"approve\"\nenv = {{ SYNTH_DESKTOP_IPC_FILE = \"{}\", SYNTH_SESSION_ID = \"{}\" }}\n",
                toml_string(&bin.display().to_string()), mcp_enabled_tools(server), toml_string(&ipc.display().to_string()), toml_string(&request.session_id),
            ));
        }
        fs::write(home.join("config.toml"), existing)?;
    }
    Ok(())
}

fn multi_agent_flags(version: MultiAgentVersion) -> (bool, bool, bool) {
    match version {
        MultiAgentVersion::None => (false, false, false),
        MultiAgentVersion::V1 => (true, true, false),
        MultiAgentVersion::V2 => (true, true, true),
    }
}

fn validate_start(request: &CodexSessionStartRequest) -> Result<()> {
    if request.session_id.trim().is_empty() || request.model.trim().is_empty() {
        return Err(anyhow!("sessionId and model are required"));
    }
    if !Path::new(&request.workspace).is_dir() {
        return Err(anyhow!("workspace must be an existing directory"));
    }
    if !(request.base_url.starts_with("http://127.0.0.1:")
        || request.base_url.starts_with("http://localhost:")
        || request.base_url.starts_with("https://"))
    {
        return Err(anyhow!("baseUrl must be local HTTP or HTTPS"));
    }
    Ok(())
}

fn validate_reasoning_effort(value: &str) -> Result<&str> {
    match value {
        "none" | "low" | "medium" | "high" | "xhigh" | "max" => Ok(value),
        _ => Err(anyhow!("unsupported reasoning effort: {value}")),
    }
}

fn nested_id(payload: &Value, key: &str) -> Option<String> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| {
            payload
                .get(key.trim_end_matches("Id"))
                .and_then(|nested| nested.get("id"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
}

async fn session_info(id: &str, session: &Session) -> CodexSessionInfo {
    CodexSessionInfo {
        session_id: id.into(),
        thread_id: session.thread_id.clone(),
        turn_id: session.turn_id.read().await.clone(),
    }
}

fn codex_root() -> PathBuf {
    env::var_os("SYNTH_CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::instance::state_root().join("codex"))
}

async fn persist_records(
    records: &Arc<RwLock<HashMap<String, CodexSessionRecord>>>,
    state_path: &Path,
) -> Result<()> {
    if let Some(parent) = state_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let data = serde_json::to_vec_pretty(&*records.read().await)?;
    let temporary = state_path.with_extension("json.tmp");
    tokio::fs::write(&temporary, data).await?;
    tokio::fs::rename(temporary, state_path).await?;
    Ok(())
}
fn safe_component(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect()
}
fn toml_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}
fn toml_key(value: &str) -> String {
    format!("\"{}\"", toml_string(value))
}

fn workspace_write_config(allowed_roots: &[String]) -> String {
    if allowed_roots.is_empty() {
        return String::new();
    }
    let roots = allowed_roots
        .iter()
        .map(|root| format!("\"{}\"", toml_string(root)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[sandbox_workspace_write]\nwritable_roots = [{roots}]\n\n")
}

fn mcp_enabled_tools(server: &str) -> &'static str {
    match server {
        // Codex sees one compact namespace member. The adapter keeps legacy
        // tools callable for other MCP clients, while visual_manage routes the
        // same operations after the visual skill is loaded.
        "synth_visuals" => "enabled_tools = [\"visual_manage\"]\n",
        _ => "",
    }
}

/// Codex appends `/responses` to the provider base URL. Laguna and standard
/// OpenAI-compatible providers expose that endpoint below `/v1`.
fn responses_base_url(value: &str) -> String {
    let trimmed = value.trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        trimmed.to_owned()
    } else {
        format!("{trimmed}/v1")
    }
}

fn automatic_thread_title(prompt: &str) -> Option<String> {
    let mut value = prompt
        .lines()
        .find(|line| !line.trim().is_empty())?
        .trim()
        .trim_start_matches(|c: char| matches!(c, '-' | '*' | '#' | '>' | ' '))
        .to_owned();
    for prefix in [
        "please ",
        "can you ",
        "could you ",
        "would you ",
        "i want you to ",
        "i need you to ",
    ] {
        if value.to_ascii_lowercase().starts_with(prefix) {
            value = value[prefix.len()..].trim_start().to_owned();
            break;
        }
    }
    let words = value.split_whitespace().collect::<Vec<_>>();
    let skip_skill_preamble = words
        .first()
        .is_some_and(|word| word.eq_ignore_ascii_case("use"))
        && words.get(1).is_some_and(|word| word.starts_with('$'));
    value = words
        .into_iter()
        .enumerate()
        .filter(|(index, word)| {
            !word.starts_with('$')
                && !(skip_skill_preamble && *index == 0)
                && !(skip_skill_preamble && *index == 2 && word.eq_ignore_ascii_case("to"))
        })
        .map(|(_, word)| word)
        .collect::<Vec<_>>()
        .join(" ");
    if let Some(index) = value.find(['\n', '.', '?', '!']) {
        value.truncate(index);
    }
    value = value
        .trim_matches(|c: char| c.is_whitespace() || matches!(c, ',' | ':' | ';' | '-' | '—'))
        .to_owned();
    if value.is_empty() {
        return None;
    }
    const MAX_CHARS: usize = 56;
    if value.chars().count() > MAX_CHARS {
        let mut shortened = String::new();
        for word in value.split_whitespace() {
            let next_len = shortened.chars().count()
                + usize::from(!shortened.is_empty())
                + word.chars().count();
            if next_len > MAX_CHARS {
                break;
            }
            if !shortened.is_empty() {
                shortened.push(' ');
            }
            shortened.push_str(word);
        }
        value = shortened;
    }
    let mut chars = value.chars();
    let first = chars.next()?;
    Some(first.to_uppercase().collect::<String>() + chars.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{RunCreate, RunService, SessionCreate, SessionService};
    use std::path::Path;
    use tempfile::tempdir;

    fn fixture_binary() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fake_codex_app_server.py")
    }

    fn test_request(workspace: &Path, session_id: &str) -> CodexSessionStartRequest {
        CodexSessionStartRequest {
            session_id: session_id.into(),
            workspace: workspace.display().to_string(),
            base_url: "http://127.0.0.1:7333".into(),
            api_key: String::new(),
            model: "poolside/Laguna-XS-2.1-NVFP4-mlx".into(),
            provider_name: Some("local-laguna".into()),
            provider_title: Some("Laguna fixture".into()),
            provider_env_key: Some("SYNTH_LAGUNA_API_KEY".into()),
            approval_policy: Some("never".into()),
            sandbox: Some("workspace-write".into()),
            thread_id: None,
            multi_agent_version: Some(MultiAgentVersion::None),
        }
    }

    async fn wait_for_record_status(manager: &CodexManager, session_id: &str, expected: &str) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let actual = manager
                    .records
                    .read()
                    .await
                    .get(session_id)
                    .map(|record| record.status.clone());
                if actual.as_deref() == Some(expected) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("session {session_id} did not become {expected}"));
    }

    async fn wait_for_run_status(core: &CoreRuntime, run_id: &str, expected: &str) {
        let runs = RunService::new(core.storage().database().clone());
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let actual = runs
                    .get(run_id.to_owned())
                    .await
                    .unwrap()
                    .map(|run| run.status);
                if actual.as_deref() == Some(expected) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("run {run_id} did not become {expected}"));
    }

    fn fixture_requests(root: &Path, session_id: &str) -> Vec<Value> {
        let path = root
            .join("homes")
            .join(safe_component(session_id))
            .join("fake-app-server-requests.jsonl");
        fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[tokio::test]
    async fn killed_app_server_interrupts_sqlite_and_resumes_the_same_thread() {
        let temp = tempdir().unwrap();
        let codex_root = temp.path().join("codex");
        let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
        let manager =
            CodexManager::with_paths(Some(core.clone()), codex_root.clone(), fixture_binary());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let request = test_request(temp.path(), "crash-resume");

        let started = manager
            .start(app_handle.clone(), request.clone())
            .await
            .unwrap();
        assert_eq!(started.thread_id, "thread-fixture");
        let first_turn = manager
            .start_turn(
                app_handle.clone(),
                CodexTurnStartRequest {
                    session_id: request.session_id.clone(),
                    prompt: "keep working until the process is killed".into(),
                    effort: Some("none".into()),
                },
            )
            .await
            .unwrap()
            .turn_id
            .unwrap();
        let first_attachment = manager
            .sessions
            .read()
            .await
            .get(&request.session_id)
            .unwrap()
            .clone();

        first_attachment.server.stop().await.unwrap();
        wait_for_record_status(&manager, &request.session_id, "interrupted").await;
        wait_for_run_status(&core, &first_turn, "interrupted").await;
        assert!(!manager
            .sessions
            .read()
            .await
            .contains_key(&request.session_id));
        let persisted_run = RunService::new(core.storage().database().clone())
            .get(first_turn.clone())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(persisted_run.status, "interrupted");
        assert_eq!(
            persisted_run.outcome.unwrap()["reason"],
            "app_server_exited"
        );

        // Stop remains safe after the process and active attachment are gone.
        manager.interrupt(&request.session_id).await.unwrap();

        let resumed = manager
            .start(app_handle.clone(), request.clone())
            .await
            .unwrap();
        assert_eq!(resumed.thread_id, started.thread_id);
        let second_turn = manager
            .start_turn(
                app_handle,
                CodexTurnStartRequest {
                    session_id: request.session_id.clone(),
                    prompt: "continue after reconnect".into(),
                    effort: Some("none".into()),
                },
            )
            .await
            .unwrap()
            .turn_id
            .unwrap();
        assert_ne!(first_turn, second_turn);
        let requests = fixture_requests(&codex_root, &request.session_id);
        assert!(requests.iter().any(|message| {
            message["method"] == "thread/resume"
                && message["params"]["threadId"] == "thread-fixture"
        }));
        manager.close(&request.session_id).await.unwrap();
    }

    #[tokio::test]
    async fn startup_reconciles_an_orphaned_running_turn_in_sqlite() {
        let temp = tempdir().unwrap();
        let codex_root = temp.path().join("codex");
        fs::create_dir_all(&codex_root).unwrap();
        let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
        let sessions = SessionService::new(core.storage().database().clone());
        sessions
            .create_or_update(SessionCreate {
                id: "orphan".into(),
                title: "Orphaned local turn".into(),
                target: json!({"kind":"codex"}),
                project_id: None,
                remote_id: None,
                codex_thread_id: Some("thread-orphan".into()),
                status: SessionStatus::Ready,
                state_generation: None,
                metadata: json!({}),
                source: EventSource::Codex,
            })
            .await
            .unwrap();
        RunService::new(core.storage().database().clone())
            .start(RunCreate {
                id: "turn-orphan".into(),
                session_id: "orphan".into(),
                mode: "codex_turn".into(),
                model: Some("laguna".into()),
                adapter: None,
                metadata: json!({}),
                source: EventSource::Codex,
            })
            .await
            .unwrap();
        let record = CodexSessionRecord {
            session_id: "orphan".into(),
            thread_id: "thread-orphan".into(),
            workspace: temp.path().display().to_string(),
            model: "laguna".into(),
            provider_name: "local-laguna".into(),
            provider_title: "Laguna fixture".into(),
            base_url: "http://127.0.0.1:7333/v1".into(),
            status: "running".into(),
            title: Some("Orphaned local turn".into()),
            title_origin: Some("automatic".into()),
            approval_policy: "never".into(),
            sandbox: "workspace-write".into(),
        };
        fs::write(
            codex_root.join("threads.json"),
            serde_json::to_vec_pretty(&HashMap::from([("orphan".to_owned(), record)])).unwrap(),
        )
        .unwrap();

        let restarted = CodexManager::with_paths(Some(core.clone()), codex_root, fixture_binary());
        assert_eq!(restarted.list().await[0].status, "interrupted");
        let run = RunService::new(core.storage().database().clone())
            .get("turn-orphan".into())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(run.status, "interrupted");
        assert_eq!(run.outcome.unwrap()["reason"], "desktop_restarted");
        assert_eq!(
            sessions.get("orphan".into()).await.unwrap().unwrap().status,
            "interrupted"
        );
    }

    #[tokio::test]
    async fn stale_attachment_exit_cannot_detach_its_replacement() {
        let temp = tempdir().unwrap();
        let codex_root = temp.path().join("codex");
        let manager = CodexManager::with_paths(None, codex_root, fixture_binary());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let request = test_request(temp.path(), "generation-fence");
        manager
            .start(app_handle.clone(), request.clone())
            .await
            .unwrap();
        let stale = manager
            .sessions
            .write()
            .await
            .remove(&request.session_id)
            .unwrap();
        manager.start(app_handle, request.clone()).await.unwrap();
        let replacement_id = manager
            .sessions
            .read()
            .await
            .get(&request.session_id)
            .unwrap()
            .attachment_id;
        assert_ne!(stale.attachment_id, replacement_id);

        stale.server.stop().await.unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        let current = manager
            .sessions
            .read()
            .await
            .get(&request.session_id)
            .unwrap()
            .attachment_id;
        assert_eq!(current, replacement_id);
        assert_eq!(
            manager.records.read().await[&request.session_id].status,
            "ready"
        );
        manager.close(&request.session_id).await.unwrap();
    }

    #[test]
    fn extracts_flat_and_nested_ids() {
        assert_eq!(
            nested_id(&json!({"threadId":"a"}), "threadId").as_deref(),
            Some("a")
        );
        assert_eq!(
            nested_id(&json!({"thread":{"id":"b"}}), "threadId").as_deref(),
            Some("b")
        );
    }
    #[test]
    fn approvals_always_fail_closed() {
        let denied = rejection_response(&["decline".into(), "acceptForSession".into()], json!(7));
        assert_eq!(
            denied.pointer("/result/decision").and_then(Value::as_str),
            Some("decline")
        );
        let rejected = rejection_response(&["accept".into(), "acceptForSession".into()], json!(8));
        assert_eq!(
            rejected.pointer("/error/code").and_then(Value::as_i64),
            Some(-32602)
        );
    }
    #[test]
    fn approval_decisions_only_use_server_supported_values() {
        let available = vec!["decline".into(), "accept".into(), "acceptForSession".into()];
        assert_eq!(
            select_approval_decision(&available, "once").unwrap(),
            "accept"
        );
        assert_eq!(
            select_approval_decision(&available, "always").unwrap(),
            "acceptForSession"
        );
        assert_eq!(
            select_approval_decision(&available, "reject").unwrap(),
            "decline"
        );
        assert!(select_approval_decision(&available, "unknown").is_err());
        assert!(select_approval_decision(&["accept".into()], "reject").is_err());
    }
    #[test]
    fn approval_payload_does_not_expose_command_or_arbitrary_reason() {
        let payload = safe_approval_payload(
            "approval-1",
            "item/commandExecution/requestApproval",
            &json!({
                "command":"OPENROUTER_API_KEY=secret curl example.test",
                "cwd":"/workspace",
                "reason":"raw model-supplied detail"
            }),
            &["decline".into(), "accept".into(), "acceptForSession".into()],
        );
        assert_eq!(payload["detail"], "Run a shell command in /workspace");
        assert_eq!(payload["alwaysSupported"], true);
        let encoded = payload.to_string();
        assert!(!encoded.contains("secret"));
        assert!(!encoded.contains("raw model"));
    }
    #[test]
    fn sanitizes_session_home_component() {
        assert_eq!(safe_component("a/b c"), "a_b_c");
    }
    #[test]
    fn escapes_toml_values() {
        assert_eq!(toml_string("a\"b\\c\n"), "a\\\"b\\\\c\\n");
    }
    #[test]
    fn renders_additional_workspace_roots_for_codex() {
        assert_eq!(workspace_write_config(&[]), "");
        let config =
            workspace_write_config(&["/Users/example/Documents/GitHub".into(), "/tmp/a\"b".into()]);
        assert!(config.contains("[sandbox_workspace_write]"));
        assert!(config
            .contains("writable_roots = [\"/Users/example/Documents/GitHub\", \"/tmp/a\\\"b\"]"));
    }
    #[test]
    fn advertises_only_the_compact_visual_tool_to_codex() {
        assert_eq!(
            mcp_enabled_tools("synth_visuals"),
            "enabled_tools = [\"visual_manage\"]\n"
        );
        assert_eq!(mcp_enabled_tools("synth_containers"), "");
    }
    #[test]
    fn normalizes_responses_provider_base_url() {
        assert_eq!(
            responses_base_url("http://127.0.0.1:7333"),
            "http://127.0.0.1:7333/v1"
        );
        assert_eq!(
            responses_base_url("https://provider.test/v1/"),
            "https://provider.test/v1"
        );
    }

    #[test]
    fn maps_model_capability_to_app_server_feature_flags() {
        assert_eq!(
            multi_agent_flags(MultiAgentVersion::None),
            (false, false, false)
        );
        assert_eq!(
            multi_agent_flags(MultiAgentVersion::V1),
            (true, true, false)
        );
        assert_eq!(multi_agent_flags(MultiAgentVersion::V2), (true, true, true));
    }

    #[test]
    fn validates_reasoning_effort_values() {
        for value in ["none", "low", "medium", "high", "xhigh", "max"] {
            assert_eq!(validate_reasoning_effort(value).unwrap(), value);
        }
        assert!(validate_reasoning_effort("ultra").is_err());
    }

    #[test]
    fn derives_a_short_title_from_the_first_prompt() {
        assert_eq!(
            automatic_thread_title(
                "please add session descriptions to the Rust core. Then test it"
            ),
            Some("Add session descriptions to the Rust core".into())
        );
        assert_eq!(
            automatic_thread_title(
                "Use $use-synth-containers to inspect Craftax and locate its real policy harness"
            ),
            Some("Inspect Craftax and locate its real policy harness".into())
        );
    }

    #[test]
    fn detached_running_sessions_are_reconciled_as_interrupted() {
        let mut running = "running".to_owned();
        assert!(reconcile_detached_status(&mut running, false));
        assert_eq!(running, "interrupted");

        let mut attached = "running".to_owned();
        assert!(!reconcile_detached_status(&mut attached, true));
        assert_eq!(attached, "running");

        let mut ready = "ready".to_owned();
        assert!(!reconcile_detached_status(&mut ready, false));
        assert_eq!(ready, "ready");
    }
}
