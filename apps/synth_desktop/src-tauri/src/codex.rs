use crate::core_runtime::CoreRuntime;
use crate::credential_broker::{self, CredentialBroker};
use crate::domain::{
    RunCreate, RunService, RunStatus, SessionCreate, SessionService, SessionStatus,
    SessionTitleOrigin,
};
use crate::storage::{
    CostSource, EventAppend, EventSource, MeasurementKind, UsageRecord, UsageRecordsRepository,
};
use crate::synth_config::MultiAgentVersion;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    env, fmt, fs,
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
const MIN_AUTO_COMPACT_TOKEN_LIMIT: u64 = 16_000;
const COMPACT_PROMPT: &str = "You are performing a CONTEXT CHECKPOINT COMPACTION for a coding agent.\nWrite a handoff for another LLM that will continue the same workspace task.\nInclude:\n- Goal and acceptance criteria\n- Files read/changed (paths + one-line why)\n- Commands/tests run and outcomes\n- Decisions and constraints\n- Open bugs / next concrete steps\n- Any secrets-safe identifiers (branch names, ticket ids) needed to continue\nOmit raw file dumps, full command logs, and superseded plans.\nBe concise and structured (bullets).";

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSessionStartRequest {
    pub session_id: String,
	pub workspace: String,
	pub base_url: String,
	#[serde(default)]
	pub api_key: String,
    pub model: String,
    pub provider_name: Option<String>,
    pub provider_title: Option<String>,
    pub provider_env_key: Option<String>,
    pub approval_policy: Option<String>,
    pub sandbox: Option<String>,
    pub thread_id: Option<String>,
    pub multi_agent_version: Option<MultiAgentVersion>,
    #[serde(default)]
    pub auto_compact_token_limit: Option<u64>,
    /// Rust-populated exact roots for this conversation. Renderer input is
    /// discarded by `prepare_codex_start` before launch.
    #[serde(default)]
    pub writable_roots: Vec<String>,
    /// Rust-set marker that `api_key` holds a real user credential which must
    /// move into native custody before any child process observes it. Staged
    /// by `prepare_codex_start`, consumed by `CodexManager::start` at spawn
    /// time. Serde skips it, so the renderer can never set it.
    #[serde(skip)]
    pub broker_credential: bool,
}

impl fmt::Debug for CodexSessionStartRequest {
    /// Between preparation and spawn, `api_key` may hold a real user
    /// credential rather than a lease token; never let `{:?}` reproduce it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodexSessionStartRequest")
            .field("session_id", &self.session_id)
            .field("workspace", &self.workspace)
            .field("base_url", &self.base_url)
            .field("api_key", &"<redacted>")
            .field("model", &self.model)
            .field("provider_name", &self.provider_name)
            .field("provider_title", &self.provider_title)
            .field("provider_env_key", &self.provider_env_key)
            .field("approval_policy", &self.approval_policy)
            .field("sandbox", &self.sandbox)
            .field("thread_id", &self.thread_id)
            .field("multi_agent_version", &self.multi_agent_version)
            .field("auto_compact_token_limit", &self.auto_compact_token_limit)
            .field("writable_roots", &self.writable_roots)
            .field("broker_credential", &self.broker_credential)
            .finish()
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexTurnStartRequest {
    pub session_id: String,
    pub prompt: String,
    pub effort: Option<String>,
}

/// One atomic renderer intent: make sure the app-server is attached for this
/// session and start the turn. The renderer never observes the intermediate
/// state where an attachment exists but the turn has not started.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexTurnSendRequest {
    pub start: CodexSessionStartRequest,
    pub prompt: String,
    pub effort: Option<String>,
    /// When the destination model differs from the live attachment, compact the
    /// thread on the *source* model before rebind. Renderer sets this from the
    /// send-time state machine (`modelSwitchPlan`): true only when the thread
    /// has history; empty threads skip compact.
    #[serde(default)]
    pub compact_before_model_switch: bool,
}

/// Typed failure so the renderer can react to a lost app-server without
/// parsing free-form text or leaking raw ids into a toast.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexTurnFailure {
    pub code: String,
    pub message: String,
    pub session_id: String,
    /// Developer-facing text. The renderer keeps this out of user surfaces.
    pub detail: String,
}

pub const CODEX_SESSION_DETACHED: &str = "codex_session_detached";
pub const CODEX_TURN_START_FAILED: &str = "codex_turn_start_failed";
const DETACHED_MESSAGE: &str =
    "The local agent process disconnected before the turn started. Retry to reconnect.";
const STDOUT_CLOSED: &str = "codex app-server stdout closed";

impl CodexTurnFailure {
    fn detached(session_id: &str, detail: String) -> Self {
        Self {
            code: CODEX_SESSION_DETACHED.into(),
            message: DETACHED_MESSAGE.into(),
            session_id: session_id.to_owned(),
            detail,
        }
    }

    fn rejected(session_id: &str, error: &anyhow::Error) -> Self {
        Self {
            code: CODEX_TURN_START_FAILED.into(),
            message: error.to_string(),
            session_id: session_id.to_owned(),
            detail: format!("{error:?}"),
        }
    }
}

/// Marker cause for "the app-server owning this session is gone". It travels
/// inside the anyhow chain so callers never string-match transport text.
#[derive(Debug)]
struct SessionDetached;

impl std::fmt::Display for SessionDetached {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("the local codex app-server is not attached")
    }
}

impl std::error::Error for SessionDetached {}

fn is_detached_failure(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| cause.is::<SessionDetached>())
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

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexSteerRequest {
    pub session_id: String,
    pub text: String,
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
/// Waiters for `thread/compacted`, keyed by Desktop session id.
/// Model-switch compaction waiters. A `thread/compacted` notification means
/// the summary exists, but the source app-server still owns an active
/// compaction turn until its terminal turn event arrives. Rebinding before
/// that terminal event can consume the user's destination prompt as the
/// compaction turn and leave no answer.
type CompactWaiters = Arc<Mutex<HashMap<String, oneshot::Sender<Result<(), String>>>>>;

#[derive(Clone, Debug)]
struct PendingApproval {
    rpc_id: Value,
    available_decisions: Vec<String>,
}

type PendingApprovals = Arc<Mutex<HashMap<String, PendingApproval>>>;

#[derive(Clone, Debug, Default)]
struct TurnTokenUsage {
    input_tokens: Option<i64>,
    cached_input_tokens: Option<i64>,
    cache_write_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
    output_tokens: Option<i64>,
}

#[derive(Clone, Debug)]
struct TurnPerformanceTracker {
    provider: String,
    model_id: String,
    turn_id: String,
    started_at_ms: i64,
    first_output_at_ms: Option<i64>,
    last_output_at_ms: Option<i64>,
    usage: TurnTokenUsage,
}

type PerformanceTrackers = Arc<Mutex<HashMap<String, TurnPerformanceTracker>>>;

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
            // A write failure on the child's stdin means the process is gone.
            return Err(error.context(SessionDetached));
        }
        match tokio::time::timeout(Duration::from_secs(30), rx).await {
            Ok(Ok(Ok(value))) => Ok(value),
            Ok(Ok(Err(error))) if error.contains(STDOUT_CLOSED) => Err(anyhow!(SessionDetached)
                .context(format!("codex app-server {method} lost its process"))),
            Ok(Ok(Err(error))) => Err(anyhow!("codex app-server {method} error: {error}")),
            Ok(Err(_)) => Err(anyhow!(SessionDetached)
                .context(format!("codex app-server stopped while handling {method}"))),
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
    sandbox: String,
    workspace: String,
    /// The provider binding the child was spawned against, as staged by
    /// `prepare_codex_start` — the upstream endpoint and credential *before*
    /// brokering. Held in memory only, never serialized, and compared on
    /// reuse so a rotated credential or endpoint respawns the child instead
    /// of leaving it bound to the old provider.
    upstream_endpoint: String,
    upstream_credential: String,
    /// The provider identity the child was spawned under. This is the sole
    /// input to `provider_class`, which gates the settled-receipt drain at
    /// finalize — so a name change must respawn (and thereby revoke, which
    /// discards queued receipts) even if endpoint, credential and model all
    /// coincide across the two providers.
    provider_name: String,
}

pub struct CodexManager {
    sessions: Arc<RwLock<HashMap<String, Arc<Session>>>>,
    records: Arc<RwLock<HashMap<String, CodexSessionRecord>>>,
    /// Serializes attach + turn/start per session so no caller can observe the
    /// window between "the attachment exists" and "the turn is running".
    turn_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    compact_waiters: CompactWaiters,
    /// Session ids awaiting a `thread/compacted` notification, mapped to the
    /// UI source label (`manual` or `model_switch`). Auto token-threshold
    /// compaction leaves this empty and renders as "automatically compacted".
    pending_compact_sources: Arc<Mutex<HashMap<String, String>>>,
    performance_trackers: PerformanceTrackers,
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
            turn_locks: Mutex::new(HashMap::new()),
            compact_waiters: Arc::new(Mutex::new(HashMap::new())),
            pending_compact_sources: Arc::new(Mutex::new(HashMap::new())),
            performance_trackers: Arc::new(Mutex::new(HashMap::new())),
            root,
            state_path,
            binary,
            core,
        }
    }

    pub async fn start<R: tauri::Runtime>(
        &self,
        app: AppHandle<R>,
        mut request: CodexSessionStartRequest,
    ) -> Result<CodexSessionInfo> {
        validate_start(&request)?;
        let requested_approval = request
            .approval_policy
            .clone()
            .unwrap_or_else(default_approval_policy);
        let requested_sandbox = request.sandbox.clone().unwrap_or_else(default_sandbox);
        let requested_provider = request
            .provider_name
            .clone()
            .unwrap_or_else(|| "custom".into());
        let existing = self.sessions.read().await.get(&request.session_id).cloned();
        if let Some(existing) = existing {
            if existing.model == request.model
                && existing.approval_policy == requested_approval
                && existing.sandbox == requested_sandbox
                && existing.workspace == request.workspace
                && existing.upstream_endpoint == request.base_url
                && existing.upstream_credential == request.api_key
                && existing.provider_name == requested_provider
            {
                return Ok(session_info(&request.session_id, &existing).await);
            }
            // Approval, sandbox and workspace are attachment-time properties in
            // Codex app-server. Reusing an attachment after the composer mode
            // changes would make the UI lie (for example, showing Allow all
            // while the live process still asks for shell approval). The
            // provider binding is compared the same way: a child spawned
            // against a rotated-away credential or endpoint must be replaced,
            // not reused.
            self.close(&request.session_id).await?;
        }
        // Custody is taken here — at spawn, after the reuse decision — and
        // nowhere earlier. Leasing during request preparation invalidated the
        // token a live, reused child was still presenting (a mid-conversation
        // 401), and on rebind the `close()` above revokes the previous lease,
        // so this is the one point where a fresh token's lifetime matches the
        // child's.
        let upstream_endpoint = request.base_url.clone();
        let upstream_credential = request.api_key.clone();
        if request.broker_credential {
            let broker = credential_broker::shared()
                .map_err(|error| anyhow!("credential broker unavailable: {error}"))?;
            apply_brokered_credential(&mut request, &broker).map_err(|message| anyhow!(message))?;
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
            self.compact_waiters.clone(),
            self.pending_compact_sources.clone(),
            self.performance_trackers.clone(),
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
            sandbox: request.sandbox.clone().unwrap_or_else(default_sandbox),
            workspace: request.workspace.clone(),
            upstream_endpoint,
            upstream_credential,
            provider_name: requested_provider.clone(),
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
            if crate::workspace_scope::get(core.storage().database(), &request.session_id)
                .await?
                .is_none()
            {
                crate::workspace_scope::provision(
                    core.storage().database(),
                    &request.session_id,
                    &request.workspace,
                )
                .await?;
            }
            crate::workspace_scope::mark_bound(core.storage().database(), &request.session_id)
                .await?;
        }
        Ok(session_info(&request.session_id, &session).await)
    }

    pub async fn start_turn<R: tauri::Runtime>(
        &self,
        app: AppHandle<R>,
        request: CodexTurnStartRequest,
    ) -> Result<CodexSessionInfo> {
        self.start_turn_inner(app, request, true).await
    }

    /// Atomic attach-or-resume plus turn start.
    ///
    /// A single per-session lock covers both halves, and one silent retry
    /// covers the remaining live race: the app-server can exit after `start`
    /// observes the attachment and before `turn/start` reaches it. When the
    /// process is really gone the durable JSON record and the SQLite run are
    /// reconciled *before* the typed error reaches the renderer, so the UI can
    /// never be left showing `Working` with `Stop`.
    pub async fn send_turn<R: tauri::Runtime>(
        &self,
        app: AppHandle<R>,
        request: CodexTurnSendRequest,
    ) -> std::result::Result<CodexSessionInfo, CodexTurnFailure> {
        let session_id = request.start.session_id.clone();
        if request.prompt.trim().is_empty() {
            return Err(CodexTurnFailure::rejected(
                &session_id,
                &anyhow!("prompt must not be empty"),
            ));
        }
        if let Some(effort) = request.effort.as_deref() {
            if let Err(error) = validate_reasoning_effort(effort) {
                return Err(CodexTurnFailure::rejected(&session_id, &error));
            }
        }
        let lock = self.turn_lock(&session_id).await;
        let _guard = lock.lock().await;

        // The user prompt is journalled once, up front, so a rejected turn
        // still preserves the text the operator typed.
        self.record_user_prompt(&app, &session_id, &request.prompt)
            .await;

        // Compact-on-send model switch: while the live attachment is still the
        // source model, summarize before start() closes it and resumes as B.
        if let Err(error) = self.maybe_compact_before_model_switch(&request).await {
            let _ = self
                .reconcile_failed_turn_start(&session_id, "model_switch_compact_failed")
                .await;
            return Err(CodexTurnFailure::rejected(&session_id, &error));
        }

        let mut failure: Option<anyhow::Error> = None;
        for attempt in 0..2u8 {
            let attachment = match self.start(app.clone(), request.start.clone()).await {
                Ok(_) => self
                    .sessions
                    .read()
                    .await
                    .get(&session_id)
                    .map(|session| session.attachment_id),
                Err(error) => {
                    let detached = is_detached_failure(&error);
                    failure = Some(error);
                    if detached && attempt == 0 {
                        self.discard_attachment(&session_id, None).await;
                        continue;
                    }
                    break;
                }
            };
            let turn = self
                .start_turn_inner(
                    app.clone(),
                    CodexTurnStartRequest {
                        session_id: session_id.clone(),
                        prompt: request.prompt.clone(),
                        effort: request.effort.clone(),
                    },
                    false,
                )
                .await;
            match turn {
                Ok(info) => return Ok(info),
                Err(error) => {
                    let detached = is_detached_failure(&error);
                    failure = Some(error);
                    if !detached {
                        break;
                    }
                    // Drop only the attachment we used. A replacement created
                    // by another caller keeps its generation fence.
                    self.discard_attachment(&session_id, attachment).await;
                    if attempt == 0 {
                        continue;
                    }
                    break;
                }
            }
        }

        let error = failure.unwrap_or_else(|| anyhow!("the turn could not be started"));
        if !is_detached_failure(&error) {
            let _ = self
                .reconcile_failed_turn_start(&session_id, "turn_start_failed")
                .await;
            return Err(CodexTurnFailure::rejected(&session_id, &error));
        }
        let _ = self
            .reconcile_failed_turn_start(&session_id, "turn_start_detached")
            .await;
        let _ = app.emit(
            EVENT_NAME,
            CodexEvent {
                session_id: session_id.clone(),
                method: "session/unhealthy".into(),
                params: json!({
                    "reason": "turn_start_detached",
                    "message": DETACHED_MESSAGE
                }),
            },
        );
        Err(CodexTurnFailure::detached(
            &session_id,
            format!("{error:?}"),
        ))
    }

    async fn start_turn_inner<R: tauri::Runtime>(
        &self,
        app: AppHandle<R>,
        request: CodexTurnStartRequest,
        record_prompt: bool,
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
        if record_prompt {
            self.record_user_prompt(&app, &request.session_id, &request.prompt)
                .await;
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
        let started_at_ms = chrono::Utc::now().timestamp_millis();
        let provider = self
            .records
            .read()
            .await
            .get(&request.session_id)
            .map(|record| record.provider_name.clone())
            .unwrap_or_else(|| "custom".into());
        let pending_turn_id = format!("pending-{}", uuid::Uuid::new_v4().simple());
        self.performance_trackers.lock().await.insert(
            request.session_id.clone(),
            TurnPerformanceTracker {
                provider,
                model_id: session.model.clone(),
                turn_id: pending_turn_id.clone(),
                started_at_ms,
                first_output_at_ms: None,
                last_output_at_ms: None,
                usage: TurnTokenUsage::default(),
            },
        );
        let result = match session.server.request("turn/start", turn_params).await {
            Ok(result) => result,
            Err(error) => {
                let mut trackers = self.performance_trackers.lock().await;
                if trackers
                    .get(&request.session_id)
                    .is_some_and(|tracker| tracker.turn_id == pending_turn_id)
                {
                    trackers.remove(&request.session_id);
                }
                return Err(error);
            }
        };
        let Some(turn_id) = nested_id(&result, "turnId") else {
            self.performance_trackers.lock().await.remove(&request.session_id);
            return Err(anyhow!("Codex turn/start response missing turn id: {result}"));
        };
        if let Some(tracker) = self.performance_trackers.lock().await.get_mut(&request.session_id) {
            if tracker.turn_id == pending_turn_id {
                tracker.turn_id = turn_id.clone();
            }
        }
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

    /// Send-time compact before a model rebind.
    ///
    /// Only runs when the renderer asked for compact *and* a live attachment
    /// is still bound to a different model than `request.start`. Chip fiddling
    /// never reaches here; empty threads pass `compact_before_model_switch=false`.
    async fn maybe_compact_before_model_switch(
        &self,
        request: &CodexTurnSendRequest,
    ) -> Result<()> {
        if !request.compact_before_model_switch {
            return Ok(());
        }
        let session_id = &request.start.session_id;
        let Some(session) = self.sessions.read().await.get(session_id).cloned() else {
            // No live source attachment (cold resume). Rebind without compact
            // rather than inventing a source provider/home from the destination
            // request — that would be dishonest about which model summarized.
            return Ok(());
        };
        if session.model == request.start.model {
            return Ok(());
        }
        self.compact_thread(session_id, &session).await
    }

    async fn compact_thread(&self, session_id: &str, session: &Session) -> Result<()> {
        self.pending_compact_sources
            .lock()
            .await
            .insert(session_id.to_owned(), "model_switch".into());
        let (tx, rx) = oneshot::channel();
        {
            let mut waiters = self.compact_waiters.lock().await;
            if let Some(previous) = waiters.insert(session_id.to_owned(), tx) {
                let _ = previous.send(Err("context compaction was superseded".into()));
            }
        }
        let params = json!({ "threadId": session.thread_id });
        if let Err(error) = session.server.request("thread/compact/start", params).await {
            self.compact_waiters.lock().await.remove(session_id);
            self.pending_compact_sources.lock().await.remove(session_id);
            return Err(error.context("thread/compact/start failed; staying on the current model"));
        }
        match tokio::time::timeout(Duration::from_secs(120), rx).await {
            Ok(Ok(Ok(()))) => Ok(()),
            Ok(Ok(Err(message))) => {
                self.pending_compact_sources.lock().await.remove(session_id);
                Err(anyhow!(message))
            }
            Ok(Err(_)) => {
                self.pending_compact_sources.lock().await.remove(session_id);
                Err(anyhow!(
                    "compaction waiter dropped before thread/compacted; staying on the current model"
                ))
            }
            Err(_) => {
                self.compact_waiters.lock().await.remove(session_id);
                self.pending_compact_sources.lock().await.remove(session_id);
                Err(anyhow!(
                    "timed out waiting for thread/compacted; staying on the current model"
                ))
            }
        }
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

    pub async fn compact<R: tauri::Runtime>(
        &self,
        app: AppHandle<R>,
        request: CodexSessionStartRequest,
    ) -> Result<()> {
        let session_id = request.session_id.clone();
        let lock = self.turn_lock(&session_id).await;
        let _guard = lock.lock().await;

        let mut failure = None;
        for attempt in 0..2u8 {
            let attachment = match self.start(app.clone(), request.clone()).await {
                Ok(_) => self
                    .sessions
                    .read()
                    .await
                    .get(&session_id)
                    .map(|session| session.attachment_id),
                Err(error) => {
                    let detached = is_detached_failure(&error);
                    failure = Some(error);
                    if detached && attempt == 0 {
                        self.discard_attachment(&session_id, None).await;
                        continue;
                    }
                    break;
                }
            };
            let session = self.session(&session_id).await?;
            self.pending_compact_sources
                .lock()
                .await
                .insert(session_id.clone(), "manual".into());
            match session
                .server
                .request(
                    "thread/compact/start",
                    json!({"threadId": session.thread_id}),
                )
                .await
            {
                Ok(_) => return Ok(()),
                Err(error) => {
                    self.pending_compact_sources
                        .lock()
                        .await
                        .remove(&session_id);
                    let detached = is_detached_failure(&error);
                    failure = Some(error);
                    if !detached {
                        break;
                    }
                    self.discard_attachment(&session_id, attachment).await;
                    if attempt == 0 {
                        continue;
                    }
                    break;
                }
            }
        }
        Err(failure.unwrap_or_else(|| anyhow!("context compaction could not be started")))
    }

    /// Steers an in-flight turn with additional user input. Unlike `interrupt`,
    /// this requires an attached session with an active turn: there is
    /// nothing sensible to steer once the app-server has detached or the turn
    /// has already finished, so both cases are surfaced as errors instead of
    /// treated as idempotent no-ops.
    pub async fn steer_turn<R: tauri::Runtime>(
        &self,
        app: AppHandle<R>,
        request: CodexSteerRequest,
    ) -> Result<()> {
        if request.text.trim().is_empty() {
            return Err(anyhow!("steer text must not be empty"));
        }
        let session = self.session(&request.session_id).await?;
        let Some(turn_id) = session.turn_id.read().await.clone() else {
            return Err(anyhow!(
                "session {} has no active turn to steer",
                request.session_id
            ));
        };
        self.record_user_prompt(&app, &request.session_id, &request.text)
            .await;
        session
            .server
            .request(
                "turn/steer",
                json!({
                    "threadId": session.thread_id,
                    "expectedTurnId": turn_id,
                    "input": [{"type":"text","text": request.text, "textElements": []}],
                }),
            )
            .await?;
        Ok(())
    }

    pub async fn close(&self, session_id: &str) -> Result<()> {
        let session = self.sessions.write().await.remove(session_id);
        if let Some(session) = session {
            session.server.stop().await?;
        }
        // The child is gone; its loopback lease must not outlive it.
        credential_broker::revoke_shared(session_id);
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
        let mut detached = Vec::new();
        {
            let mut records = self.records.write().await;
            for record in records.values_mut() {
                let is_attached = attached.contains_key(&record.session_id);
                let reconciled = reconcile_detached_status(&mut record.status, is_attached);
                changed |= reconciled;
                if !is_attached {
                    detached.push(record.session_id.clone());
                }
            }
        }
        drop(attached);
        if changed {
            let _ = self.persist_records().await;
        }
        // Reconcile the durable run independently of the attachment record.
        // During a graceful desktop shutdown the stdout task can persist the
        // JSON record as interrupted and then lose the race with process exit
        // before it transitions SQLite. On the next launch that record no
        // longer changes, but its active run still needs to be interrupted.
        if let Some(core) = &self.core {
            for session_id in detached {
                let _ = interrupt_active_core_run(core, &session_id, "desktop_restarted").await;
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
            .ok_or_else(|| anyhow!(SessionDetached).context(format!("Codex session {id}")))
    }

    async fn turn_lock(&self, session_id: &str) -> Arc<Mutex<()>> {
        self.turn_locks
            .lock()
            .await
            .entry(session_id.to_owned())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    async fn record_user_prompt<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        session_id: &str,
        prompt: &str,
    ) {
        let Some(core) = &self.core else { return };
        let persistence = core.append_and_emit(
            app,
            EventAppend::codex(
                session_id.to_owned(),
                "message.created",
                json!({
                    "messageId": format!("user-{}", uuid::Uuid::new_v4()),
                    "role": "user",
                    "content": prompt,
                }),
            ),
        );
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), persistence).await;
    }

    /// Removes a dead attachment, but only when it is still the current one.
    /// `expected` is `None` when no attachment was resolved at all.
    async fn discard_attachment(&self, session_id: &str, expected: Option<uuid::Uuid>) {
        let mut sessions = self.sessions.write().await;
        let matches = sessions.get(session_id).is_some_and(|session| {
            expected.is_none_or(|attachment_id| session.attachment_id == attachment_id)
        });
        if matches {
            sessions.remove(session_id);
        }
    }

    /// Brings durable state back in line after a turn could not be started.
    /// The JSON record must never stay `running`, and any active SQLite run for
    /// this session belongs to a turn nobody can finish.
    async fn reconcile_failed_turn_start(&self, session_id: &str, reason: &str) -> Result<()> {
        let was_running = self
            .records
            .read()
            .await
            .get(session_id)
            .is_some_and(|record| record.status == "running");
        if was_running {
            self.set_status(session_id, "interrupted").await?;
        }
        if let Some(core) = &self.core {
            let _ = interrupt_active_core_run(core, session_id, reason).await;
        }
        Ok(())
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

/// Environment variables reaching a Codex child are not private: Codex writes
/// its inherited environment to `CODEX_HOME/shell_snapshots` as plain
/// `export NAME=value`. These names are the ones a real provider credential
/// lives under, so a value under any of them is refused at the spawn boundary —
/// a `credential_broker` lease is the only thing that may cross it.
const CREDENTIAL_ENV_NAMES: &[&str] = &["SYNTH_API_KEY", "OPENROUTER_API_KEY", "OPENAI_API_KEY"];

/// The single provider variable the Codex child is allowed to receive, if any.
///
/// Refusing a real credential name is an error, not an omission: launching the
/// child without the variable it was configured to read would surface later as
/// an unauthenticated provider, and the reason would be nowhere in the logs.
fn provider_child_env(
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
    compact_waiters: CompactWaiters,
    pending_compact_sources: Arc<Mutex<HashMap<String, String>>>,
    performance_trackers: PerformanceTrackers,
    attachment_id: uuid::Uuid,
) -> Result<Arc<AppServer>> {
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
            compact_waiters,
            pending_compact_sources,
            performance_trackers,
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
    compact_waiters: CompactWaiters,
    pending_compact_sources: Arc<Mutex<HashMap<String, String>>>,
    performance_trackers: PerformanceTrackers,
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
            track_performance_event(
                core,
                &persistence.performance_trackers,
                &session_id,
                &method,
                &params,
            )
            .await;
        }
        if matches!(method.as_str(), "turn/completed" | "thread/compact/completed") {
            if let Some(waiter) = persistence.compact_waiters.lock().await.remove(&session_id) {
                let _ = waiter.send(Ok(()));
            }
        } else if matches!(method.as_str(), "turn/failed" | "turn/interrupted") {
            if let Some(waiter) = persistence.compact_waiters.lock().await.remove(&session_id) {
                let _ = waiter.send(Err(format!("context compaction ended with {method}")));
            }
        }
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
            persistence
                .pending_compact_sources
                .lock()
                .await
                .remove(&session_id);
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
            finalize_performance_tracker(
                core,
                &persistence.performance_trackers,
                &session_id,
                "interrupted",
                None,
            )
            .await;
            if let Ok(Some(event)) =
                interrupt_active_core_run(core, &session_id, "app_server_exited").await
            {
                let _ = core.publish_event(&app, event).await;
            }
        }
    }
}

fn is_context_compaction_notification(method: &str, params: &Value) -> bool {
    method == "thread/compacted"
        || params
            .get("item")
            .and_then(|item| item.get("type"))
            .and_then(Value::as_str)
            == Some("contextCompaction")
}

fn positive_i64(value: Option<i64>) -> Option<i64> {
    value.filter(|value| *value >= 0)
}

fn integer_field(value: &Value, aliases: &[&str]) -> Option<i64> {
    aliases
        .iter()
        .find_map(|key| value.get(*key).and_then(Value::as_i64))
}

fn usage_from_object(value: &Value) -> Option<TurnTokenUsage> {
    let output_tokens = positive_i64(integer_field(
        value,
        &[
            "output_tokens",
            "outputTokens",
            "completion_tokens",
            "completionTokens",
        ],
    ));
    output_tokens?;
    let details = value
        .get("output_tokens_details")
        .or_else(|| value.get("outputTokensDetails"));
    Some(TurnTokenUsage {
        input_tokens: positive_i64(integer_field(
            value,
            &["input_tokens", "inputTokens", "prompt_tokens", "promptTokens"],
        )),
        cached_input_tokens: positive_i64(integer_field(
            value,
            &["cached_input_tokens", "cachedInputTokens", "cached_tokens", "cachedTokens"],
        )),
        cache_write_tokens: positive_i64(integer_field(
            value,
            &[
                "cache_write_input_tokens",
                "cacheWriteInputTokens",
                "cache_creation_input_tokens",
                "cacheCreationInputTokens",
            ],
        )),
        reasoning_tokens: details.and_then(|details| {
            positive_i64(integer_field(details, &["reasoning_tokens", "reasoningTokens"]))
        }),
        output_tokens,
    })
}

fn extract_turn_usage(params: &Value) -> Option<TurnTokenUsage> {
    // Prefer explicitly per-turn/last-usage objects. Thread-wide totals are
    // excluded: after a restart they cannot produce an honest turn rate.
    const PATHS: &[&str] = &[
        "/lastUsage",
        "/lastTokenUsage",
        "/tokenUsage/last",
        "/tokenUsage/lastUsage",
        "/tokenUsage/lastTokenUsage",
        "/turn/lastUsage",
        "/turn/lastTokenUsage",
        "/turn/tokenUsage/last",
        "/turn/tokenUsage/lastUsage",
        "/turn/usage",
        "/usage",
    ];
    PATHS
        .iter()
        .find_map(|path| params.pointer(path).and_then(usage_from_object))
}

fn is_output_delta(method: &str, params: &Value) -> bool {
    let normalized = method.to_ascii_lowercase();
    let is_agent_delta = normalized.contains("agentmessage/delta")
        || normalized.contains("agent_message/delta")
        || normalized.contains("outputtext/delta")
        || normalized.contains("output_text/delta");
    is_agent_delta
        && params
            .get("delta")
            .and_then(Value::as_str)
            .is_some_and(|delta| !delta.is_empty())
}

async fn track_performance_event(
    core: &Arc<CoreRuntime>,
    trackers: &PerformanceTrackers,
    session_id: &str,
    method: &str,
    params: &Value,
) {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let terminal = matches!(method, "turn/completed" | "turn/failed" | "turn/interrupted");
    {
        let mut trackers = trackers.lock().await;
        let Some(tracker) = trackers.get_mut(session_id) else {
            return;
        };
        if is_output_delta(method, params) {
            tracker.first_output_at_ms.get_or_insert(now_ms);
            tracker.last_output_at_ms = Some(now_ms);
        }
        if method.to_ascii_lowercase().contains("usage") || terminal {
            if let Some(usage) = extract_turn_usage(params) {
                tracker.usage = usage;
            }
        }
    }
    if terminal {
        let status = match method {
            "turn/completed" => "completed",
            "turn/failed" => "failed",
            _ => "interrupted",
        };
        finalize_performance_tracker(core, trackers, session_id, status, Some(now_ms)).await;
    }
}

async fn finalize_performance_tracker(
    core: &Arc<CoreRuntime>,
    trackers: &PerformanceTrackers,
    session_id: &str,
    status: &str,
    completed_at_ms: Option<i64>,
) {
    let Some(tracker) = trackers.lock().await.remove(session_id) else {
        return;
    };
    let completed_at_ms = completed_at_ms.unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
    let output_tokens = tracker.usage.output_tokens.filter(|tokens| *tokens > 0);
    let stream_seconds = tracker
        .first_output_at_ms
        .zip(tracker.last_output_at_ms)
        .map(|(first, last)| (last - first) as f64 / 1_000.0)
        .filter(|seconds| *seconds > 0.0);
    let end_to_end_seconds = ((completed_at_ms - tracker.started_at_ms) as f64 / 1_000.0).max(0.0);
    let observed_output_tps = output_tokens
        .zip(stream_seconds)
        .map(|(tokens, seconds)| tokens as f64 / seconds);
    let end_to_end_output_tps = output_tokens
        .filter(|_| end_to_end_seconds > 0.0)
        .map(|tokens| tokens as f64 / end_to_end_seconds);
    let measurement_kind = if observed_output_tps.is_some() {
        MeasurementKind::ObservedStream
    } else {
        MeasurementKind::EndToEnd
    };
    // A failed or interrupted turn still consumed whatever the provider
    // reported, so it is recorded — and estimated — like any other request.
    let estimated_cost_usd = crate::tariffs::estimate_cost_usd(
        &tracker.provider,
        &tracker.model_id,
        completed_at_ms,
        crate::tariffs::BillableTokens {
            input_tokens: tracker.usage.input_tokens,
            cached_input_tokens: tracker.usage.cached_input_tokens,
            cache_write_tokens: tracker.usage.cache_write_tokens,
            output_tokens: tracker.usage.output_tokens,
        },
    );
    // Settled Synth Cloud accounting, captured by the credential broker as the
    // child's responses streamed through it. Only cloud turns drain: local /
    // on-device providers have no provider charge and their rows stay exactly
    // as the tracker built them — billed stays `None`, never $0.
    //
    // Exactly-once contract: draining removes the receipts, and the
    // `(provider, request_id)` upsert key dedupes a replayed finalize. A
    // receipt landing after this drain (cancellation race) stays queued no
    // longer than the session's next finalize; if the session closes first,
    // the broker logs one line and drops it rather than inventing a row.
    // The drain reads a module-level store — it never starts a broker.
    let settled_cost_usd = if provider_class(Some(&tracker.provider)) == ProviderClass::SynthCloud {
        settled_cost_from_receipts(&credential_broker::drain_settled_receipts(session_id))
    } else {
        None
    };
    // A settled receipt is authoritative; the tariff figure stays in
    // `estimated_cost_usd` and must never override it.
    let cost_source = if settled_cost_usd.is_some() {
        CostSource::SynthCloud
    } else if estimated_cost_usd.is_some() {
        CostSource::TariffEstimate
    } else {
        CostSource::None
    };
    let record = UsageRecord {
        id: format!("perf:{}:{}", tracker.provider, tracker.turn_id),
        provider: tracker.provider,
        model_id: tracker.model_id,
        model_revision: None,
        session_id: Some(session_id.to_owned()),
        run_id: Some(tracker.turn_id.clone()),
        request_id: tracker.turn_id,
        measurement_kind,
        status: status.to_owned(),
        started_at_ms: tracker.started_at_ms,
        first_output_at_ms: tracker.first_output_at_ms,
        last_output_at_ms: tracker.last_output_at_ms,
        completed_at_ms,
        input_tokens: tracker.usage.input_tokens,
        cached_input_tokens: tracker.usage.cached_input_tokens,
        cache_write_tokens: tracker.usage.cache_write_tokens,
        reasoning_tokens: tracker.usage.reasoning_tokens,
        output_tokens,
        ttft_ms: tracker
            .first_output_at_ms
            .map(|first| (first - tracker.started_at_ms).max(0) as f64),
        observed_output_tps,
        end_to_end_output_tps,
        billed_cost_usd: settled_cost_usd,
        estimated_cost_usd,
        cost_source,
        source: "codex_app_server".into(),
    };
    let repository = UsageRecordsRepository::new(core.storage().database().clone());
    if let Err(error) = repository.record(record).await {
        eprintln!("usage record could not be persisted: {error:#}");
    }
}

/// Sum of the settled charges a turn's receipts carried. A turn is allowed to
/// span several upstream requests, so several receipts sum into one figure.
/// `None` when no receipt reported money — token-only receipts never fabricate
/// a $0 settled charge.
fn settled_cost_from_receipts(receipts: &[credential_broker::SettledReceipt]) -> Option<f64> {
    receipts
        .iter()
        .filter_map(|receipt| receipt.cost_usd)
        .fold(None, |total, cost| Some(total.unwrap_or(0.0) + cost))
}

fn normalized_turn_method<'a>(method: &'a str, params: &Value) -> &'a str {
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

fn automatic_approval_response(available: &[String], id: Value) -> Value {
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

/// The four ways a session sources its provider endpoint and credential.
/// Every start request falls into exactly one class, and both preparation
/// paths (`lib.rs` commands and the eval driver) branch on this — nowhere
/// else decides how a credential is handled.
///
/// | class        | endpoint from        | credential from        | custody |
/// |--------------|----------------------|------------------------|---------|
/// | `LocalLaguna`| local Laguna daemon  | loopback service token | none — the token is process-owned and only works against loopback |
/// | `SynthCloud` | resolved backend cfg | user's Synth API key   | staged, leased at spawn |
/// | `OpenRouter` | renderer (public URL)| user's OpenRouter key  | staged, leased at spawn |
/// | `Direct`     | renderer             | none on the Rust side  | pass-through — renderer-supplied `api_key` is never treated as a credential Rust vouches for |
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderClass {
    LocalLaguna,
    SynthCloud,
    OpenRouter,
    Direct,
}

pub fn provider_class(provider_name: Option<&str>) -> ProviderClass {
    match provider_name {
        Some(name) if name.eq_ignore_ascii_case("local-laguna") => ProviderClass::LocalLaguna,
        Some(name) if name.eq_ignore_ascii_case("synth-cloud") => ProviderClass::SynthCloud,
        Some(name) if name.eq_ignore_ascii_case("openrouter") => ProviderClass::OpenRouter,
        _ => ProviderClass::Direct,
    }
}

/// Point a session start request at the Synth Cloud provider.
///
/// `gateway_url` must already be the profile's resolved, fail-closed
/// Responses gateway — see `synth_config::require_responses_gateway_url`,
/// which every caller runs before reaching this function. This function
/// itself never falls back to a backend URL.
///
/// Fail-closed when the Synth API key is missing. Always overwrites any
/// renderer-supplied `api_key` / `base_url` / env key — credentials never
/// originate from the renderer.
///
/// The key is only *staged* here; `CodexManager::start` exchanges it for a
/// revocable loopback lease at spawn time, so what the child process, its
/// shell snapshots, and its config end up carrying is never the real key. See
/// `credential_broker`.
pub fn apply_synth_cloud_provider(
    request: &mut CodexSessionStartRequest,
    gateway_url: &str,
    api_key: Option<&str>,
) -> Result<(), String> {
    let key = api_key
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Synth API key not configured — Settings → Account".to_string())?;
    request.base_url = format!("{}/api/v1", normalize_gateway_origin(gateway_url));
    request.provider_name = Some("synth-cloud".into());
    request.provider_title = Some("Synth Cloud Responses".into());
    stage_brokered_credential(request, key)
}

/// Stage a user credential for native custody.
///
/// The request carries the real key only between preparation and spawn, and
/// only inside this process. Deliberately no lease is minted here: preparation
/// runs on every send, and minting for a session whose live child is about to
/// be reused would invalidate the token that child is still presenting. The
/// exchange happens in `CodexManager::start`, on its spawn path.
pub fn stage_brokered_credential(
    request: &mut CodexSessionStartRequest,
    api_key: &str,
) -> Result<(), String> {
    // Surface a malformed endpoint at preparation time, where the caller can
    // still map it to a typed provider failure for the renderer.
    validated_provider_endpoint(request)?;
    request.api_key = api_key.to_owned();
    request.broker_credential = true;
    Ok(())
}

/// Move a staged credential into native custody at spawn time.
///
/// The request keeps its logical endpoint path but is re-pointed at the
/// loopback proxy, and the staged key it carries becomes a revocable lease
/// token. Every provider whose credential belongs to the user — not to a local
/// loopback service — goes through here before a child process can observe it.
pub fn apply_brokered_credential(
    request: &mut CodexSessionStartRequest,
    broker: &CredentialBroker,
) -> Result<(), String> {
    let endpoint = validated_provider_endpoint(request)?;
    let lease = broker.lease(
        &request.session_id,
        &endpoint.origin().ascii_serialization(),
        &request.api_key,
    );
    request.base_url = format!("{}{}", lease.origin, endpoint.path().trim_end_matches('/'));
    request.api_key = lease.token;
    request.provider_env_key = Some(credential_broker::LEASE_ENV_KEY.into());
    request.broker_credential = false;
    Ok(())
}

fn validated_provider_endpoint(
    request: &CodexSessionStartRequest,
) -> Result<reqwest::Url, String> {
    reqwest::Url::parse(&request.base_url).map_err(|_| {
        format!(
            "{} could not start because its endpoint is invalid: {}. Update it in Settings → Account → Backend API.",
            request
                .provider_title
                .as_deref()
                .or(request.provider_name.as_deref())
                .unwrap_or("Selected provider"),
            safe_endpoint_label(&request.base_url)
        )
    })
}

/// Local services often advertise `0.0.0.0` as their bind address. That is
/// not a usable client destination, so turn it into loopback before Codex
/// validates or connects to the Synth Cloud Responses provider.
fn client_base_url(backend_url: &str) -> String {
    backend_url
        .trim()
        .trim_end_matches('/')
        .replacen("http://0.0.0.0:", "http://127.0.0.1:", 1)
}

/// A configured `[intern.gateways]` entry may already include the `/api/v1`
/// (or `/api/v1/responses`) suffix `apply_synth_cloud_provider` is about to
/// append. Strip it first so the composed `base_url` never doubles the path.
fn normalize_gateway_origin(gateway_url: &str) -> String {
    let mut origin = client_base_url(gateway_url);
    for suffix in ["/api/v1/responses", "/api/v1"] {
        if let Some(stripped) = origin.strip_suffix(suffix) {
            origin = stripped.to_owned();
            break;
        }
    }
    origin
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
    let optimizers_skill = home.join("skills/use-synth-optimizers");
    fs::create_dir_all(&optimizers_skill)?;
    fs::write(
        optimizers_skill.join("SKILL.md"),
        include_str!("../../skills/use-synth-optimizers/SKILL.md"),
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
    let mut writable_roots = vec![request.workspace.clone()];
    writable_roots.extend(request.writable_roots.clone());
    writable_roots.sort();
    writable_roots.dedup();
    let workspace_write_config = workspace_write_config(&writable_roots);
    let compaction_config = if supports_provider_compaction(request) {
        // OpenAI and Azure Responses providers are recognized by Codex itself.
        // Leave their compaction configuration untouched so Codex uses the
        // provider-hosted /responses/compact implementation.
        String::new()
    } else {
        format!(
            "model_auto_compact_token_limit = {}\ntool_output_token_limit = 12000\ncompact_prompt = \"{}\"\n",
            auto_compact_token_limit(request),
            toml_string(COMPACT_PROMPT)
        )
    };
    // The Synth Hosted Laguna gateway behind `synth-cloud` is itself a
    // stateless native Responses passthrough with no `previous_response_id`
    // session store (see `laguna_daemon.responses_api.backends.remote_responses`).
    // Telling Codex to disable response storage keeps both sides of the wire
    // consistent: Codex sends `store: false` and the full turn history on
    // every request instead of a bare `previous_response_id`, so nothing
    // here ever depends on a server-held session or a `submit_tool_outputs`
    // continuation the gateway cannot serve.
    let disable_response_storage_config = if requires_disabled_response_storage(request) {
        "disable_response_storage = true\n"
    } else {
        ""
    };
    let config = format!(
        "model = \"{}\"\nmodel_provider = \"{}\"\napproval_policy = \"{}\"\nsandbox_mode = \"{}\"\nservice_tier = \"default\"\n{}{}\n{}[model_providers.{}]\nname = \"{}\"\nbase_url = \"{}\"\nenv_key = \"{}\"\nwire_api = \"responses\"\nrequires_openai_auth = false\n# Codex selects provider-hosted compaction for OpenAI/Azure and local summarization otherwise.\n\n[agents]\nenabled = {}\n\n[features]\nmulti_agent = {}\nmulti_agent_v2 = {}\ntool_call_mcp_elicitation = false\nshell_tool = true\nunified_exec = true\n",
        toml_string(&request.model), toml_string(provider), toml_string(request.approval_policy.as_deref().unwrap_or("untrusted")), toml_string(request.sandbox.as_deref().unwrap_or("workspace-write")), disable_response_storage_config, compaction_config, workspace_write_config, toml_key(provider), toml_string(title), toml_string(&responses_base_url(&request.base_url)), toml_string(env_key), agents_enabled, multi_agent_v1, multi_agent_v2
    );
    fs::write(home.join("config.toml"), config)?;
    let auth = home.join("auth.json");
    if !auth.exists() {
        fs::write(
            auth,
            "{\n  \"OPENAI_API_KEY\": \"synth-desktop-provider\"\n}\n",
        )?;
    }
    // Point Codex at the Rust noun adapters (all forward to CoreRuntime IPC).
    if let Ok(exe) = env::current_exe() {
        let ipc = crate::storage::app_data_root().join("visuals-ipc.json");
        let mut existing = fs::read_to_string(home.join("config.toml")).unwrap_or_default();
        for (server, binary) in [
            ("synth_containers", "synth-containers-mcp"),
            ("synth_visuals", "synth-visuals-mcp"),
            ("synth_optimizers", "synth-optimizers-mcp"),
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
    if !supports_provider_compaction(request) {
        let compact_limit = auto_compact_token_limit(request);
        let max_compact_limit = model_context_window(&request.model) * 9 / 10;
        if !(MIN_AUTO_COMPACT_TOKEN_LIMIT..=max_compact_limit).contains(&compact_limit) {
            return Err(anyhow!(
                "autoCompactTokenLimit must be between {MIN_AUTO_COMPACT_TOKEN_LIMIT} and {max_compact_limit} for {}",
                request.model
            ));
        }
    }
    if !(request.base_url.starts_with("http://127.0.0.1:")
        || request.base_url.starts_with("http://localhost:")
        || request.base_url.starts_with("https://"))
    {
        let provider = request
            .provider_title
            .as_deref()
            .or(request.provider_name.as_deref())
            .unwrap_or("Selected provider");
        return Err(anyhow!(
            "{provider} could not start because its endpoint is invalid: {}. Use an HTTPS endpoint, or a local endpoint such as http://127.0.0.1:<port>. Update it in Settings → Account → Backend API.",
            safe_endpoint_label(&request.base_url),
        ));
    }
    Ok(())
}

fn model_context_window(model: &str) -> u64 {
    if model.to_ascii_lowercase().contains("laguna-xs") {
        262_144
    } else if model.to_ascii_lowercase().contains("muse-spark-1.2") {
        1_048_576
    } else if model.to_ascii_lowercase().contains("laguna-s-2.1")
        || model.to_ascii_lowercase().contains("gpt-5.6-luna")
    {
        1_050_000
    } else {
        262_144
    }
}

fn auto_compact_token_limit(request: &CodexSessionStartRequest) -> u64 {
    request.auto_compact_token_limit.unwrap_or_else(|| {
        let model = request.model.to_ascii_lowercase();
        if model.contains("laguna-s-2.1")
            || model.contains("gpt-5.6-luna")
            || model.contains("muse-spark-1.2")
        {
            250_000
        } else if model.contains("laguna-xs") {
            150_000
        } else {
            model_context_window(&request.model) * 4 / 5
        }
    })
}

/// Whether Codex should disable server-side response storage for this
/// session's provider.
///
/// `synth-cloud` is the only provider this applies to today: the Synth
/// Hosted Laguna gateway behind it is a stateless native Responses
/// passthrough (`store: false` is forced upstream, and any
/// `previous_response_id` a client sends is dropped rather than resolved —
/// see `remote_responses.py`'s `_passthrough_body`). Setting
/// `disable_response_storage = true` makes Codex match that contract: it
/// sends `store: false` and full turn history with every request, so it
/// never depends on the gateway resolving a `previous_response_id` or
/// serving a `submit_tool_outputs` continuation against session state the
/// gateway does not keep.
fn requires_disabled_response_storage(request: &CodexSessionStartRequest) -> bool {
    request
        .provider_name
        .as_deref()
        .is_some_and(|name| name.eq_ignore_ascii_case("synth-cloud"))
}

fn supports_provider_compaction(request: &CodexSessionStartRequest) -> bool {
    if request.provider_name.as_deref() == Some("openai")
        || request.provider_title.as_deref() == Some("OpenAI")
        || request
            .provider_name
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case("azure"))
        || request
            .provider_title
            .as_deref()
            .is_some_and(|name| name.eq_ignore_ascii_case("azure"))
    {
        return true;
    }
    let base_url = request.base_url.to_ascii_lowercase();
    [
        "openai.azure.",
        "cognitiveservices.azure.",
        "aoai.azure.",
        "azure-api.",
        "azurefd.",
        "windows.net/openai",
    ]
    .iter()
    .any(|marker| base_url.contains(marker))
}

/// Produces an endpoint label that is useful in UI errors without exposing a
/// query-string token or credentials embedded in the URL authority.
fn safe_endpoint_label(endpoint: &str) -> String {
    let without_query = endpoint.trim().split(['?', '#']).next().unwrap_or_default();
    let redacted = match without_query.split_once("://") {
        Some((scheme, remainder)) => match remainder.split_once('@') {
            Some((_, host_and_path)) => format!("{scheme}://[credentials]@{host_and_path}"),
            None => without_query.to_owned(),
        },
        None => without_query.to_owned(),
    };
    const MAX_CHARS: usize = 160;
    if redacted.chars().count() <= MAX_CHARS {
        redacted
    } else {
        format!(
            "{}…",
            redacted
                .chars()
                .take(MAX_CHARS.saturating_sub(1))
                .collect::<String>()
        )
    }
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

pub fn codex_root() -> PathBuf {
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
        "synth_optimizers" => "enabled_tools = [\"optimizer_manage\"]\n",
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
    use crate::storage::UsageBreakdown;
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
            auto_compact_token_limit: None,
            writable_roots: Vec::new(),
            broker_credential: false,
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
    async fn steer_turn_sends_turn_steer_with_the_active_turn_id() {
        let temp = tempdir().unwrap();
        let codex_root = temp.path().join("codex");
        let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
        let manager =
            CodexManager::with_paths(Some(core.clone()), codex_root.clone(), fixture_binary());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let request = test_request(temp.path(), "steer-me");

        manager
            .start(app_handle.clone(), request.clone())
            .await
            .unwrap();
        let turn_id = manager
            .start_turn(
                app_handle.clone(),
                CodexTurnStartRequest {
                    session_id: request.session_id.clone(),
                    prompt: "keep working on the task".into(),
                    effort: Some("none".into()),
                },
            )
            .await
            .unwrap()
            .turn_id
            .unwrap();

        manager
            .steer_turn(
                app_handle.clone(),
                CodexSteerRequest {
                    session_id: request.session_id.clone(),
                    text: "actually, focus on the tests first".into(),
                },
            )
            .await
            .unwrap();

        // The turn id is unchanged: steering augments the in-flight turn
        // rather than starting a new one.
        assert_eq!(
            manager
                .sessions
                .read()
                .await
                .get(&request.session_id)
                .unwrap()
                .turn_id
                .read()
                .await
                .clone(),
            Some(turn_id.clone())
        );

        let requests = fixture_requests(&codex_root, &request.session_id);
        let steer = requests
            .iter()
            .find(|message| message["method"] == "turn/steer")
            .expect("fixture did not see turn/steer");
        assert_eq!(steer["params"]["threadId"], "thread-fixture");
        assert_eq!(steer["params"]["expectedTurnId"], turn_id);
        assert_eq!(
            steer["params"]["input"][0]["text"],
            "actually, focus on the tests first"
        );
        assert_eq!(steer["params"]["input"][0]["type"], "text");

        manager.close(&request.session_id).await.unwrap();
    }

    #[tokio::test]
    async fn compact_sends_thread_compact_start_for_the_attached_thread() {
        let temp = tempdir().unwrap();
        let codex_root = temp.path().join("codex");
        let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
        let manager = CodexManager::with_paths(Some(core), codex_root.clone(), fixture_binary());
        let app = tauri::test::mock_app();
        let request = test_request(temp.path(), "compact-me");

        manager
            .compact(app.handle().clone(), request.clone())
            .await
            .unwrap();

        let requests = fixture_requests(&codex_root, &request.session_id);
        let compact = requests
            .iter()
            .find(|message| message["method"] == "thread/compact/start")
            .expect("fixture did not see thread/compact/start");
        assert_eq!(compact["params"]["threadId"], "thread-fixture");

        manager.close(&request.session_id).await.unwrap();
    }

    #[tokio::test]
    async fn steer_turn_fails_when_there_is_no_active_turn() {
        let temp = tempdir().unwrap();
        let codex_root = temp.path().join("codex");
        let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
        let manager =
            CodexManager::with_paths(Some(core.clone()), codex_root.clone(), fixture_binary());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let request = test_request(temp.path(), "steer-without-turn");

        manager
            .start(app_handle.clone(), request.clone())
            .await
            .unwrap();

        let error = manager
            .steer_turn(
                app_handle.clone(),
                CodexSteerRequest {
                    session_id: request.session_id.clone(),
                    text: "hello".into(),
                },
            )
            .await
            .expect_err("steering without an active turn must fail");
        assert!(error.to_string().contains("no active turn"));

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
    async fn startup_reconciles_sqlite_when_detached_record_is_already_interrupted() {
        let temp = tempdir().unwrap();
        let codex_root = temp.path().join("codex");
        fs::create_dir_all(&codex_root).unwrap();
        let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
        let sessions = SessionService::new(core.storage().database().clone());
        sessions
            .create_or_update(SessionCreate {
                id: "orphan-after-graceful-exit".into(),
                title: "Partially reconciled local turn".into(),
                target: json!({"kind":"codex"}),
                project_id: None,
                remote_id: None,
                codex_thread_id: Some("thread-partial".into()),
                status: SessionStatus::Ready,
                state_generation: None,
                metadata: json!({}),
                source: EventSource::Codex,
            })
            .await
            .unwrap();
        RunService::new(core.storage().database().clone())
            .start(RunCreate {
                id: "turn-partial".into(),
                session_id: "orphan-after-graceful-exit".into(),
                mode: "codex_turn".into(),
                model: Some("laguna".into()),
                adapter: None,
                metadata: json!({}),
                source: EventSource::Codex,
            })
            .await
            .unwrap();
        let record = CodexSessionRecord {
            session_id: "orphan-after-graceful-exit".into(),
            thread_id: "thread-partial".into(),
            workspace: temp.path().display().to_string(),
            model: "laguna".into(),
            provider_name: "local-laguna".into(),
            provider_title: "Laguna fixture".into(),
            base_url: "http://127.0.0.1:7333/v1".into(),
            status: "interrupted".into(),
            title: Some("Partially reconciled local turn".into()),
            title_origin: Some("automatic".into()),
            approval_policy: "never".into(),
            sandbox: "workspace-write".into(),
        };
        fs::write(
            codex_root.join("threads.json"),
            serde_json::to_vec_pretty(&HashMap::from([(
                "orphan-after-graceful-exit".to_owned(),
                record,
            )]))
            .unwrap(),
        )
        .unwrap();

        let restarted = CodexManager::with_paths(Some(core.clone()), codex_root, fixture_binary());
        assert_eq!(restarted.list().await[0].status, "interrupted");
        let run = RunService::new(core.storage().database().clone())
            .get("turn-partial".into())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(run.status, "interrupted");
        assert_eq!(run.outcome.unwrap()["reason"], "desktop_restarted");
        assert_eq!(
            sessions
                .get("orphan-after-graceful-exit".into())
                .await
                .unwrap()
                .unwrap()
                .status,
            "interrupted"
        );
    }

    fn session_home(root: &Path, session_id: &str) -> PathBuf {
        root.join("homes").join(safe_component(session_id))
    }

    /// Makes the fixture app-server exit the moment `turn/start` arrives.
    /// `once` clears the marker first so a retry can succeed.
    fn arm_turn_start_exit(root: &Path, session_id: &str, mode: &str) {
        let home = session_home(root, session_id);
        fs::create_dir_all(&home).unwrap();
        fs::write(home.join("exit-on-turn-start"), mode).unwrap();
    }

    fn disarm_turn_start_exit(root: &Path, session_id: &str) {
        let marker = session_home(root, session_id).join("exit-on-turn-start");
        if marker.exists() {
            fs::remove_file(marker).unwrap();
        }
    }

    fn send_request(start: CodexSessionStartRequest, prompt: &str) -> CodexTurnSendRequest {
        CodexTurnSendRequest {
            start,
            prompt: prompt.into(),
            effort: Some("none".into()),
            compact_before_model_switch: false,
        }
    }

    /// The screenshot bug: the app-server exits between attach and turn/start.
    /// The renderer must get a typed detachment, and durable state must already
    /// be reconciled when it does. A later retry resumes the same thread.
    #[tokio::test]
    async fn turn_send_reports_detachment_and_reconciles_before_returning() {
        let temp = tempdir().unwrap();
        let codex_root = temp.path().join("codex");
        let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
        let manager =
            CodexManager::with_paths(Some(core.clone()), codex_root.clone(), fixture_binary());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let request = test_request(temp.path(), "turn-send-detached");

        // A healthy first turn establishes the thread and an active SQLite run.
        let first = manager
            .send_turn(
                app_handle.clone(),
                send_request(request.clone(), "start the acceptance turn"),
            )
            .await
            .unwrap();
        let first_turn = first.turn_id.clone().unwrap();
        assert_eq!(first.thread_id, "thread-fixture");
        assert_eq!(
            manager.records.read().await[&request.session_id].status,
            "running"
        );

        arm_turn_start_exit(&codex_root, &request.session_id, "always");
        let failure = manager
            .send_turn(
                app_handle.clone(),
                send_request(request.clone(), "this turn can never start"),
            )
            .await
            .expect_err("a dead app-server must reject the turn");
        assert_eq!(failure.code, CODEX_SESSION_DETACHED);
        assert_eq!(failure.message, DETACHED_MESSAGE);
        assert_eq!(failure.session_id, request.session_id);
        // The raw session id belongs in debug detail, never in the message.
        assert!(!failure.message.contains(&request.session_id));

        // Reconciliation already happened, so the UI can never show Working.
        let status = manager.records.read().await[&request.session_id]
            .status
            .clone();
        assert_ne!(status, "running");
        assert!(
            status == "interrupted" || status == "ready",
            "unexpected reconciled status: {status}"
        );
        assert!(!manager
            .sessions
            .read()
            .await
            .contains_key(&request.session_id));
        let run = RunService::new(core.storage().database().clone())
            .get(first_turn.clone())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(run.status, "interrupted");
        // Whichever of the exiting stdout task and this command wins the race,
        // the run is closed with a lost-process reason and stops being active.
        let reason = run.outcome.unwrap()["reason"].as_str().unwrap().to_owned();
        assert!(
            reason == "turn_start_detached" || reason == "app_server_exited",
            "unexpected interruption reason: {reason}"
        );
        assert_eq!(
            SessionService::new(core.storage().database().clone())
                .get(request.session_id.clone())
                .await
                .unwrap()
                .unwrap()
                .active_run_id,
            None
        );
        // Stop stays idempotent while nothing is attached.
        manager.interrupt(&request.session_id).await.unwrap();

        // Retry reattaches, resumes the same Codex thread and succeeds.
        disarm_turn_start_exit(&codex_root, &request.session_id);
        let retried = manager
            .send_turn(
                app_handle,
                send_request(request.clone(), "this turn can never start"),
            )
            .await
            .unwrap();
        assert_eq!(retried.thread_id, "thread-fixture");
        assert_ne!(retried.turn_id.clone().unwrap(), first_turn);
        assert_eq!(
            manager.records.read().await[&request.session_id].status,
            "running"
        );
        let requests = fixture_requests(&codex_root, &request.session_id);
        assert!(requests.iter().any(|message| {
            message["method"] == "thread/resume"
                && message["params"]["threadId"] == "thread-fixture"
        }));
        manager.close(&request.session_id).await.unwrap();
    }

    /// A single exit is absorbed inside the command: the renderer sees one
    /// successful send, never a transient error it has to model.
    #[tokio::test]
    async fn turn_send_retries_once_through_a_dying_app_server() {
        let temp = tempdir().unwrap();
        let codex_root = temp.path().join("codex");
        let manager = CodexManager::with_paths(None, codex_root.clone(), fixture_binary());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let request = test_request(temp.path(), "turn-send-retry");

        arm_turn_start_exit(&codex_root, &request.session_id, "once");
        let info = manager
            .send_turn(
                app_handle,
                send_request(request.clone(), "survive one process exit"),
            )
            .await
            .unwrap();
        assert!(info.turn_id.is_some());
        assert_eq!(
            manager.records.read().await[&request.session_id].status,
            "running"
        );
        manager.close(&request.session_id).await.unwrap();
    }

    /// Restored state after a crash or relaunch: the JSON record claims
    /// `running` but nothing is attached. Sending must reattach and resume.
    #[tokio::test]
    async fn turn_send_reattaches_a_restored_running_record_without_an_attachment() {
        let temp = tempdir().unwrap();
        let codex_root = temp.path().join("codex");
        fs::create_dir_all(&codex_root).unwrap();
        let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
        let record = CodexSessionRecord {
            session_id: "restored-running".into(),
            thread_id: "thread-restored".into(),
            workspace: temp.path().display().to_string(),
            model: "laguna".into(),
            provider_name: "local-laguna".into(),
            provider_title: "Laguna fixture".into(),
            base_url: "http://127.0.0.1:7333/v1".into(),
            status: "running".into(),
            title: Some("Restored local turn".into()),
            title_origin: Some("automatic".into()),
            approval_policy: "never".into(),
            sandbox: "workspace-write".into(),
        };
        fs::write(
            codex_root.join("threads.json"),
            serde_json::to_vec_pretty(&HashMap::from([("restored-running".to_owned(), record)]))
                .unwrap(),
        )
        .unwrap();

        let manager =
            CodexManager::with_paths(Some(core.clone()), codex_root.clone(), fixture_binary());
        assert!(!manager
            .sessions
            .read()
            .await
            .contains_key("restored-running"));
        let app = tauri::test::mock_app();
        let request = test_request(temp.path(), "restored-running");
        let info = manager
            .send_turn(
                app.handle().clone(),
                send_request(request.clone(), "reconnect and continue"),
            )
            .await
            .unwrap();
        assert_eq!(info.thread_id, "thread-restored");
        assert!(info.turn_id.is_some());
        let requests = fixture_requests(&codex_root, "restored-running");
        assert!(requests.iter().any(|message| {
            message["method"] == "thread/resume"
                && message["params"]["threadId"] == "thread-restored"
        }));
        manager.close("restored-running").await.unwrap();
    }

    /// The partially reconciled shape: JSON already says `interrupted` while
    /// SQLite still holds an active run. A rejected send must close that run.
    #[tokio::test]
    async fn turn_send_interrupts_an_active_run_when_the_record_is_already_interrupted() {
        let temp = tempdir().unwrap();
        let codex_root = temp.path().join("codex");
        fs::create_dir_all(&codex_root).unwrap();
        let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
        let sessions = SessionService::new(core.storage().database().clone());
        sessions
            .create_or_update(SessionCreate {
                id: "half-reconciled".into(),
                title: "Half reconciled".into(),
                target: json!({"kind":"codex"}),
                project_id: None,
                remote_id: None,
                codex_thread_id: Some("thread-half".into()),
                status: SessionStatus::Ready,
                state_generation: None,
                metadata: json!({}),
                source: EventSource::Codex,
            })
            .await
            .unwrap();
        RunService::new(core.storage().database().clone())
            .start(RunCreate {
                id: "turn-half".into(),
                session_id: "half-reconciled".into(),
                mode: "codex_turn".into(),
                model: Some("laguna".into()),
                adapter: None,
                metadata: json!({}),
                source: EventSource::Codex,
            })
            .await
            .unwrap();
        let record = CodexSessionRecord {
            session_id: "half-reconciled".into(),
            thread_id: "thread-half".into(),
            workspace: temp.path().display().to_string(),
            model: "laguna".into(),
            provider_name: "local-laguna".into(),
            provider_title: "Laguna fixture".into(),
            base_url: "http://127.0.0.1:7333/v1".into(),
            status: "interrupted".into(),
            title: Some("Half reconciled".into()),
            title_origin: Some("automatic".into()),
            approval_policy: "never".into(),
            sandbox: "workspace-write".into(),
        };
        fs::write(
            codex_root.join("threads.json"),
            serde_json::to_vec_pretty(&HashMap::from([("half-reconciled".to_owned(), record)]))
                .unwrap(),
        )
        .unwrap();

        let manager =
            CodexManager::with_paths(Some(core.clone()), codex_root.clone(), fixture_binary());
        arm_turn_start_exit(&codex_root, "half-reconciled", "always");
        let app = tauri::test::mock_app();
        let request = test_request(temp.path(), "half-reconciled");
        let failure = manager
            .send_turn(
                app.handle().clone(),
                send_request(request, "try to continue"),
            )
            .await
            .expect_err("the fixture never answers turn/start");
        assert_eq!(failure.code, CODEX_SESSION_DETACHED);
        assert_ne!(
            manager.records.read().await["half-reconciled"].status,
            "running"
        );
        let run = RunService::new(core.storage().database().clone())
            .get("turn-half".into())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(run.status, "interrupted");
        assert_eq!(run.outcome.unwrap()["reason"], "turn_start_detached");
        assert_eq!(
            sessions
                .get("half-reconciled".into())
                .await
                .unwrap()
                .unwrap()
                .active_run_id,
            None
        );
    }

    #[tokio::test]
    async fn rejected_turn_send_arguments_never_mark_the_session_running() {
        let temp = tempdir().unwrap();
        let codex_root = temp.path().join("codex");
        let manager = CodexManager::with_paths(None, codex_root, fixture_binary());
        let app = tauri::test::mock_app();
        let request = test_request(temp.path(), "invalid-turn");
        let blank = manager
            .send_turn(app.handle().clone(), send_request(request.clone(), "   "))
            .await
            .expect_err("a blank prompt is rejected");
        assert_eq!(blank.code, CODEX_TURN_START_FAILED);
        let bad_effort = manager
            .send_turn(
                app.handle().clone(),
                CodexTurnSendRequest {
                    start: request.clone(),
                    prompt: "hello".into(),
                    effort: Some("ultra".into()),
                    compact_before_model_switch: false,
                },
            )
            .await
            .expect_err("an unsupported effort is rejected");
        assert_eq!(bad_effort.code, CODEX_TURN_START_FAILED);
        // Neither rejection may spawn an app-server or claim the session runs.
        assert!(manager.sessions.read().await.is_empty());
        assert!(manager
            .records
            .read()
            .await
            .get(&request.session_id)
            .is_none());
    }

    #[tokio::test]
    async fn turn_send_compacts_on_source_model_before_rebind() {
        let temp = tempdir().unwrap();
        let codex_root = temp.path().join("codex");
        let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
        let manager =
            CodexManager::with_paths(Some(core.clone()), codex_root.clone(), fixture_binary());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let mut request = test_request(temp.path(), "compact-before-switch");

        let first = manager
            .send_turn(
                app_handle.clone(),
                send_request(request.clone(), "establish history on source model"),
            )
            .await
            .expect("first turn starts");
        assert!(first.turn_id.is_some());

        request.model = "openai/gpt-5.6-luna".into();
        request.provider_name = Some("openrouter".into());
        let switched = manager
            .send_turn(
                app_handle.clone(),
                CodexTurnSendRequest {
                    start: request.clone(),
                    prompt: "continue on destination".into(),
                    effort: Some("medium".into()),
                    compact_before_model_switch: true,
                },
            )
            .await
            .expect("switch turn starts after compact");
        assert_eq!(switched.thread_id, first.thread_id);
        assert!(switched.turn_id.is_some());

        let messages = fixture_requests(&codex_root, &request.session_id);
        let methods: Vec<&str> = messages
            .iter()
            .filter_map(|message| message.get("method").and_then(Value::as_str))
            .collect();
        assert!(
            methods
                .iter()
                .any(|method| *method == "thread/compact/start"),
            "expected compact before rebind, got {methods:?}"
        );
        let compact_idx = methods
            .iter()
            .position(|method| *method == "thread/compact/start")
            .unwrap();
        let turn_after_compact = methods
            .iter()
            .enumerate()
            .filter(|(_, method)| **method == "turn/start")
            .map(|(idx, _)| idx)
            .max()
            .unwrap();
        assert!(turn_after_compact > compact_idx);
        assert_eq!(
            manager
                .records
                .read()
                .await
                .get(&request.session_id)
                .map(|record| record.model.as_str()),
            Some("openai/gpt-5.6-luna")
        );
        assert!(
            manager
                .pending_compact_sources
                .lock()
                .await
                .get(&request.session_id)
                .is_none(),
            "model-switch compact source should be consumed when thread/compacted arrives"
        );
        let events = core
            .journal()
            .session_events_after(request.session_id.clone(), 0, 200)
            .await
            .expect("session events");
        assert!(
            events.iter().any(|event| {
                event.kind == "thread/compacted"
                    && event.payload.get("source").and_then(Value::as_str) == Some("model_switch")
            }),
            "expected persisted thread/compacted with source=model_switch, got {:?}",
            events
                .iter()
                .map(|event| (&event.kind, event.payload.get("source")))
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn only_lost_process_failures_are_treated_as_detachment() {
        assert!(is_detached_failure(
            &anyhow!(SessionDetached).context("Codex session abc")
        ));
        assert!(!is_detached_failure(&anyhow!(
            "codex app-server turn/start error: model unavailable"
        )));
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
    fn allow_all_auto_approves_without_emitting_a_modal() {
        let session = automatic_approval_response(
            &["decline".into(), "accept".into(), "acceptForSession".into()],
            json!(9),
        );
        assert_eq!(
            session.pointer("/result/decision").and_then(Value::as_str),
            Some("acceptForSession")
        );
        let once = automatic_approval_response(&["accept".into()], json!(10));
        assert_eq!(
            once.pointer("/result/decision").and_then(Value::as_str),
            Some("accept")
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
        assert_eq!(
            mcp_enabled_tools("synth_optimizers"),
            "enabled_tools = [\"optimizer_manage\"]\n"
        );
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
        assert_eq!(
            responses_base_url("http://127.0.0.1:41209/api/v1"),
            "http://127.0.0.1:41209/api/v1"
        );
    }

    #[test]
    fn normalizes_a_gateway_origin_that_already_carries_the_api_v1_suffix() {
        assert_eq!(
            normalize_gateway_origin("http://127.0.0.1:41124/api/v1"),
            "http://127.0.0.1:41124"
        );
        assert_eq!(
            normalize_gateway_origin("https://gateway.example/api/v1/responses"),
            "https://gateway.example"
        );
        assert_eq!(
            normalize_gateway_origin("http://0.0.0.0:41124"),
            "http://127.0.0.1:41124"
        );
        assert_eq!(
            normalize_gateway_origin("https://gateway.example/"),
            "https://gateway.example"
        );
    }

    #[test]
    fn synth_cloud_provider_does_not_double_the_api_v1_path() {
        let temp = tempdir().unwrap();
        let mut request = test_request(temp.path(), "synth-cloud-double-path");
        apply_synth_cloud_provider(
            &mut request,
            "http://127.0.0.1:41124/api/v1",
            Some("sk_dev_double_path"),
        )
        .unwrap();
        assert_eq!(request.base_url, "http://127.0.0.1:41124/api/v1");
    }

    #[test]
    fn synth_cloud_provider_writes_expected_config() {
        let temp = tempdir().unwrap();
        let home = temp.path().join("home");
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let (broker, _listener) = CredentialBroker::bind().unwrap();
        let mut request = test_request(&workspace, "synth-cloud-config");
        apply_synth_cloud_provider(
            &mut request,
            "http://127.0.0.1:41209",
            Some("sk_dev_00000000000000000000000000000001"),
        )
        .unwrap();
        apply_brokered_credential(&mut request, &broker).unwrap();
        request.model = "openrouter/poolside/laguna-s-2.1".into();
        ensure_home(&home, &request).unwrap();
        let config = fs::read_to_string(home.join("config.toml")).unwrap();
        assert!(config.contains("model = \"openrouter/poolside/laguna-s-2.1\""));
        assert!(config.contains("model_provider = \"synth-cloud\""));
        assert!(config.contains("[model_providers.\"synth-cloud\"]"));
        // Codex is pointed at the native proxy, not at the backend directly.
        assert!(config.contains(&format!(
            "base_url = \"{}/api/v1\"",
            broker.origin()
        )));
        assert!(config.contains("wire_api = \"responses\""));
        assert!(config.contains(&format!(
            "env_key = \"{}\"",
            credential_broker::LEASE_ENV_KEY
        )));
        assert!(!config.contains("SYNTH_API_KEY"));
        assert_eq!(
            broker.upstream_for(&request.api_key).as_deref(),
            Some("http://127.0.0.1:41209")
        );
        assert!(config.contains("model_auto_compact_token_limit = 250000"));
        assert!(config.contains("tool_output_token_limit = 12000"));
        assert!(config.contains("CONTEXT CHECKPOINT COMPACTION for a coding agent"));
        // The Laguna gateway behind synth-cloud is a stateless passthrough
        // with no server-side session store: Codex must send `store: false`
        // and full history on every turn rather than leaning on
        // `previous_response_id` / `submit_tool_outputs` continuity the
        // gateway cannot serve.
        assert!(config.contains("disable_response_storage = true"));
        let optimizer_skill =
            fs::read_to_string(home.join("skills/use-synth-optimizers/SKILL.md")).unwrap();
        assert!(optimizer_skill.contains("optimizer_manage"));
    }

    #[test]
    fn only_synth_cloud_gets_disable_response_storage() {
        let temp = tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();

        let mut openrouter = test_request(&workspace, "openrouter-storage");
        openrouter.provider_name = Some("openrouter".into());
        openrouter.provider_title = Some("OpenRouter Responses".into());
        openrouter.base_url = "https://openrouter.ai/api/v1".into();
        let openrouter_home = temp.path().join("openrouter-home");
        ensure_home(&openrouter_home, &openrouter).unwrap();
        let openrouter_config = fs::read_to_string(openrouter_home.join("config.toml")).unwrap();
        assert!(!openrouter_config.contains("disable_response_storage"));

        let mut local_laguna = test_request(&workspace, "local-laguna-storage");
        local_laguna.provider_name = Some("local-laguna".into());
        local_laguna.provider_title = Some("Local Laguna".into());
        local_laguna.base_url = "http://127.0.0.1:7333".into();
        let local_laguna_home = temp.path().join("local-laguna-home");
        ensure_home(&local_laguna_home, &local_laguna).unwrap();
        let local_laguna_config =
            fs::read_to_string(local_laguna_home.join("config.toml")).unwrap();
        assert!(!local_laguna_config.contains("disable_response_storage"));

        assert!(requires_disabled_response_storage(&{
            let mut synth_cloud = test_request(&workspace, "synth-cloud-storage");
            synth_cloud.provider_name = Some("synth-cloud".into());
            synth_cloud
        }));
        assert!(!requires_disabled_response_storage(&openrouter));
        assert!(!requires_disabled_response_storage(&local_laguna));
    }

    #[test]
    fn synth_cloud_provider_fails_closed_without_api_key() {
        let temp = tempdir().unwrap();
        let mut request = test_request(temp.path(), "synth-cloud-missing-key");
        request.api_key = "renderer-supplied-should-not-matter".into();
        request.provider_name = Some("synth-cloud".into());
        let error = apply_synth_cloud_provider(&mut request, "http://127.0.0.1:41209", None)
            .expect_err("missing Synth API key must fail closed");
        assert!(error.contains("Synth API key not configured"));
        assert!(error.contains("Settings → Account"));
        assert_eq!(request.api_key, "renderer-supplied-should-not-matter");
        assert!(!request.broker_credential);
    }

    #[test]
    fn rejects_auto_compact_limits_outside_the_desktop_range() {
        let temp = tempdir().unwrap();
        let mut request = test_request(temp.path(), "compact-limit");
        request.auto_compact_token_limit = Some(MIN_AUTO_COMPACT_TOKEN_LIMIT - 1);
        assert!(validate_start(&request)
            .unwrap_err()
            .to_string()
            .contains("autoCompactTokenLimit"));
        request.auto_compact_token_limit = Some(235_930);
        assert!(validate_start(&request)
            .unwrap_err()
            .to_string()
            .contains("autoCompactTokenLimit"));
    }

    #[test]
    fn defaults_luna_and_laguna_s_compaction_to_250k() {
        let temp = tempdir().unwrap();
        let mut request = test_request(temp.path(), "compact-defaults");
        request.model = "poolside/Laguna-XS-2.1-NVFP4-mlx".into();
        assert_eq!(auto_compact_token_limit(&request), 150_000);
        request.model = "poolside/laguna-s-2.1".into();
        assert_eq!(auto_compact_token_limit(&request), 250_000);
        request.model = "openai/gpt-5.6-luna".into();
        assert_eq!(auto_compact_token_limit(&request), 250_000);
    }

    #[test]
    fn leaves_compaction_to_openai_and_azure_responses_providers() {
        let temp = tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();

        let mut openai = test_request(&workspace, "openai-compaction");
        openai.provider_name = Some("openai".into());
        openai.provider_title = Some("OpenAI".into());
        openai.base_url = "https://api.openai.com/v1".into();
        openai.model = "gpt-5.6-luna".into();
        openai.auto_compact_token_limit = Some(999_999_999);
        validate_start(&openai).unwrap();
        let openai_home = temp.path().join("openai-home");
        ensure_home(&openai_home, &openai).unwrap();
        let openai_config = fs::read_to_string(openai_home.join("config.toml")).unwrap();
        assert!(!openai_config.contains("model_auto_compact_token_limit"));
        assert!(!openai_config.contains("compact_prompt"));

        let mut azure = test_request(&workspace, "azure-compaction");
        azure.provider_name = Some("custom-azure".into());
        azure.provider_title = Some("Azure".into());
        azure.base_url = "https://example.openai.azure.com/openai/v1".into();
        assert!(supports_provider_compaction(&azure));

        let mut openrouter = test_request(&workspace, "openrouter-compaction");
        openrouter.provider_name = Some("openrouter".into());
        openrouter.provider_title = Some("OpenRouter Responses".into());
        openrouter.base_url = "https://openrouter.ai/api/v1".into();
        openrouter.model = "openai/gpt-5.6-luna".into();
        assert!(!supports_provider_compaction(&openrouter));
    }

    #[test]
    fn synth_cloud_provider_overwrites_renderer_api_key() {
        let temp = tempdir().unwrap();
        let (broker, _listener) = CredentialBroker::bind().unwrap();
        let mut request = test_request(temp.path(), "synth-cloud-overwrite");
        request.api_key = "renderer-leaked-key".into();
        request.base_url = "https://evil.example/v1".into();
        apply_synth_cloud_provider(&mut request, "http://127.0.0.1:41209/", Some("sk_dev_real_key"))
            .unwrap();
        // Staging discards the renderer's value and endpoint outright.
        assert_eq!(request.api_key, "sk_dev_real_key");
        assert!(request.broker_credential);
        apply_brokered_credential(&mut request, &broker).unwrap();
        // At spawn the staged key is replaced by a lease rather than being
        // carried on the request.
        assert_ne!(request.api_key, "renderer-leaked-key");
        assert_ne!(request.api_key, "sk_dev_real_key");
        assert!(request.api_key.starts_with("sdl_"));
        assert_eq!(request.base_url, format!("{}/api/v1", broker.origin()));
        assert_eq!(request.provider_name.as_deref(), Some("synth-cloud"));
        assert_eq!(
            request.provider_env_key.as_deref(),
            Some(credential_broker::LEASE_ENV_KEY)
        );
        assert_eq!(
            broker.upstream_for(&request.api_key).as_deref(),
            Some("http://127.0.0.1:41209")
        );
    }

    #[test]
    fn synth_cloud_normalizes_a_local_bind_address_for_the_client() {
        let temp = tempdir().unwrap();
        let (broker, _listener) = CredentialBroker::bind().unwrap();
        let mut request = test_request(temp.path(), "synth-cloud-loopback");
        apply_synth_cloud_provider(
            &mut request,
            "http://0.0.0.0:41209/",
            Some("sk_dev_00000000000000000000000000000001"),
        )
        .unwrap();
        apply_brokered_credential(&mut request, &broker).unwrap();
        validate_start(&request).unwrap();
        assert_eq!(request.base_url, format!("{}/api/v1", broker.origin()));
        // `0.0.0.0` is a bind address, not a destination; the proxy's upstream
        // must still be rewritten to loopback.
        assert_eq!(
            broker.upstream_for(&request.api_key).as_deref(),
            Some("http://127.0.0.1:41209")
        );
    }

    /// The CUA-found 401: preparing a send re-ran provider setup for a live
    /// session, minted a fresh lease, and thereby killed the token the reused
    /// child was still presenting. Preparing the same binding again must leave
    /// the live child's lease untouched.
    #[tokio::test]
    async fn reusing_a_live_child_leaves_its_lease_untouched() {
        let temp = tempdir().unwrap();
        let manager = CodexManager::with_paths(None, temp.path().join("codex"), fixture_binary());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let mut request = test_request(temp.path(), "lease-live-reuse");
        apply_synth_cloud_provider(&mut request, "http://127.0.0.1:41209", Some("sk_dev_reuse"))
            .unwrap();

        manager
            .start(app_handle.clone(), request.clone())
            .await
            .unwrap();
        let broker = credential_broker::shared().unwrap();
        let token = broker
            .token_for("lease-live-reuse")
            .expect("spawning the child mints its lease");

        // Second send: same staged binding, live child gets reused.
        manager.start(app_handle, request.clone()).await.unwrap();
        assert_eq!(
            broker.token_for("lease-live-reuse").as_deref(),
            Some(token.as_str())
        );
        assert!(broker.resolves(&token));

        manager.close("lease-live-reuse").await.unwrap();
        assert!(!broker.resolves(&token));
    }

    /// Rebind (for example a model switch) closes the old child, which revokes
    /// its lease. The replacement child must be spawned with a lease minted
    /// *after* that revocation — leasing during preparation handed it a token
    /// `close()` had already deleted.
    #[tokio::test]
    async fn rebinding_a_session_spawns_the_new_child_with_a_live_lease() {
        let temp = tempdir().unwrap();
        let manager = CodexManager::with_paths(None, temp.path().join("codex"), fixture_binary());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let mut request = test_request(temp.path(), "lease-rebind");
        apply_synth_cloud_provider(&mut request, "http://127.0.0.1:41209", Some("sk_dev_rebind"))
            .unwrap();
        manager
            .start(app_handle.clone(), request.clone())
            .await
            .unwrap();
        let broker = credential_broker::shared().unwrap();
        let before = broker.token_for("lease-rebind").unwrap();

        let mut switched = request.clone();
        switched.model = "openrouter/poolside/laguna-s-2.1".into();
        manager.start(app_handle, switched).await.unwrap();
        let after = broker
            .token_for("lease-rebind")
            .expect("the respawned child must hold a lease that survived close()");
        assert_ne!(before, after);
        assert!(!broker.resolves(&before));
        assert!(broker.resolves(&after));
    }

    /// Provider identity is part of the reuse comparison in its own right.
    /// `provider_name` is the sole input to `provider_class`, which gates the
    /// settled-receipt drain — two providers sharing endpoint, credential and
    /// model must still respawn (revoking, and discarding queued receipts)
    /// when the name changes, or a finalize under the new name could drain
    /// receipts born under the old one.
    #[tokio::test]
    async fn a_provider_name_change_alone_respawns_the_child() {
        let temp = tempdir().unwrap();
        let manager = CodexManager::with_paths(None, temp.path().join("codex"), fixture_binary());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let mut request = test_request(temp.path(), "lease-provider-identity");
        apply_synth_cloud_provider(&mut request, "http://127.0.0.1:41209", Some("sk_dev_shared"))
            .unwrap();
        manager
            .start(app_handle.clone(), request.clone())
            .await
            .unwrap();
        let broker = credential_broker::shared().unwrap();
        let before = broker.token_for("lease-provider-identity").unwrap();
        credential_broker::push_settled_receipt(credential_broker::SettledReceipt {
            session_id: "lease-provider-identity".into(),
            provider_response_id: "resp-born-under-old-name".into(),
            model: None,
            prompt_tokens: None,
            completion_tokens: None,
            cached_tokens: None,
            reasoning_tokens: None,
            cost_usd: Some(0.25),
            completed_at_ms: 0,
        });

        // Same endpoint, credential, model, workspace, approval and sandbox —
        // only the provider name differs.
        let mut renamed = request.clone();
        renamed.provider_name = Some("openrouter".into());
        manager.start(app_handle, renamed).await.unwrap();
        let after = broker
            .token_for("lease-provider-identity")
            .expect("the respawned child must hold a fresh lease");
        assert_ne!(before, after);
        assert!(!broker.resolves(&before));
        assert!(broker.resolves(&after));
        assert!(
            credential_broker::drain_settled_receipts("lease-provider-identity").is_empty(),
            "receipts born under the old provider name must not survive the switch"
        );
    }

    /// A changed credential or endpoint is part of the reuse comparison: the
    /// old child was spawned against the old binding, so rotation must respawn
    /// it rather than leave it talking through the stale credential.
    #[tokio::test]
    async fn a_rotated_credential_respawns_the_child_with_a_fresh_lease() {
        let temp = tempdir().unwrap();
        let manager = CodexManager::with_paths(None, temp.path().join("codex"), fixture_binary());
        let app = tauri::test::mock_app();
        let app_handle = app.handle().clone();
        let mut request = test_request(temp.path(), "lease-rotation");
        apply_synth_cloud_provider(&mut request, "http://127.0.0.1:41209", Some("sk_dev_old"))
            .unwrap();
        manager
            .start(app_handle.clone(), request.clone())
            .await
            .unwrap();
        let broker = credential_broker::shared().unwrap();
        let old_token = broker.token_for("lease-rotation").unwrap();
        let old_attachment = manager
            .sessions
            .read()
            .await
            .get("lease-rotation")
            .unwrap()
            .attachment_id;

        let mut rotated = test_request(temp.path(), "lease-rotation");
        apply_synth_cloud_provider(&mut rotated, "http://127.0.0.1:41209", Some("sk_dev_new"))
            .unwrap();
        manager.start(app_handle, rotated).await.unwrap();
        let new_attachment = manager
            .sessions
            .read()
            .await
            .get("lease-rotation")
            .unwrap()
            .attachment_id;
        let new_token = broker.token_for("lease-rotation").unwrap();
        assert_ne!(old_attachment, new_attachment);
        assert_ne!(old_token, new_token);
        assert!(!broker.resolves(&old_token));
        assert!(broker.resolves(&new_token));
    }

    #[test]
    fn completed_envelope_with_a_failed_turn_is_normalized_to_failed() {
        let failed = json!({
            "turn": {
                "status": "failed",
                "error": {"message": "provider disconnected"}
            }
        });
        assert_eq!(
            normalized_turn_method("turn/completed", &failed),
            "turn/failed"
        );
        assert_eq!(
            normalized_turn_method("turn/completed", &json!({"turn": {"status": "completed"}})),
            "turn/completed"
        );
    }

    #[test]
    fn invalid_provider_endpoint_explains_the_fix_without_leaking_credentials() {
        let temp = tempdir().unwrap();
        let mut request = test_request(temp.path(), "invalid-provider-endpoint");
        request.provider_title = Some("Synth Cloud Responses".into());
        request.base_url =
            "http://user:secret-token@0.0.0.0:41209/api/v1?api_key=secret-token".into();

        let error = validate_start(&request).unwrap_err().to_string();

        assert!(error.contains("Synth Cloud Responses could not start"));
        assert!(error.contains("http://[credentials]@0.0.0.0:41209/api/v1"));
        assert!(error.contains("Settings → Account → Backend API"));
        assert!(!error.contains("secret-token"));
    }

    #[test]
    fn synth_cloud_home_redacts_api_key_from_generated_files() {
        let temp = tempdir().unwrap();
        let home = temp.path().join("home");
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let secret = "sk_dev_SYNTH_CLOUD_SECRET_VALUE_DO_NOT_LEAK";
        let (broker, _listener) = CredentialBroker::bind().unwrap();
        let mut request = test_request(&workspace, "synth-cloud-redact");
        apply_synth_cloud_provider(&mut request, "http://127.0.0.1:41209", Some(secret)).unwrap();
        apply_brokered_credential(&mut request, &broker).unwrap();
        request.model = "openrouter/poolside/laguna-s-2.1".into();
        ensure_home(&home, &request).unwrap();
        let config = fs::read_to_string(home.join("config.toml")).unwrap();
        let auth = fs::read_to_string(home.join("auth.json")).unwrap();
        assert!(!config.contains(secret));
        assert!(!auth.contains(secret));
        assert!(config.contains(&format!(
            "env_key = \"{}\"",
            credential_broker::LEASE_ENV_KEY
        )));
        assert!(auth.contains("synth-desktop-provider"));
    }

    /// Every path that ever wrote a file under a generated Codex home, checked
    /// against one sentinel value: the credential must exist only in the native
    /// broker.
    ///
    /// `shell_snapshots` is the leak this guards. Codex serializes its inherited
    /// environment there as `export NAME=value`, so the test reproduces that
    /// step from the exact environment `spawn_server` would hand the child.
    #[test]
    fn the_synth_credential_never_reaches_a_generated_codex_home() {
        const SENTINEL: &str = "sk_live_SENTINEL_ONLY_IN_NATIVE_CUSTODY";
        let temp = tempdir().unwrap();
        let root = temp.path().join("codex");
        let home = root.join("homes/session-sentinel");
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();

        let (broker, _listener) = CredentialBroker::bind().unwrap();
        let mut request = test_request(&workspace, "session-sentinel");
        apply_synth_cloud_provider(&mut request, "http://127.0.0.1:41209", Some(SENTINEL)).unwrap();
        apply_brokered_credential(&mut request, &broker).unwrap();
        validate_start(&request).unwrap();
        ensure_home(&home, &request).unwrap();

        // Stand in for the Codex child: write the snapshot it would write from
        // the environment we actually pass it.
        let snapshots = home.join("shell_snapshots");
        fs::create_dir_all(&snapshots).unwrap();
        let exported = provider_child_env(&request)
            .expect("the brokered lease is allowed across the spawn boundary")
            .map(|(name, value)| format!("export {name}={value}\n"))
            .unwrap_or_default();
        fs::write(snapshots.join("snapshot.sh"), format!("#!/bin/sh\n{exported}")).unwrap();
        // Session logs and event payloads are the other things a home accumulates.
        fs::create_dir_all(home.join("sessions")).unwrap();
        fs::write(
            home.join("sessions/rollout.jsonl"),
            serde_json::to_string(&json!({
                "base_url": request.base_url,
                "provider": request.provider_name,
                "env_key": request.provider_env_key,
            }))
            .unwrap(),
        )
        .unwrap();

        let mut scanned = 0usize;
        let mut pending = vec![root.clone()];
        while let Some(dir) = pending.pop() {
            for entry in fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    pending.push(path);
                    continue;
                }
                scanned += 1;
                let bytes = fs::read(&path).unwrap();
                assert!(
                    !String::from_utf8_lossy(&bytes).contains(SENTINEL),
                    "the Synth credential reached {}",
                    path.display()
                );
            }
        }
        assert!(scanned > 3, "the sentinel scan must actually read files");

        // The broker holds it, and only the broker.
        assert!(request.api_key.starts_with("sdl_"));
        assert!(broker.upstream_for(&request.api_key).is_some());
        // Nothing that renders to the user or a log can reproduce it either.
        let rendered = format!(
            "{:?} {:?} {}",
            broker,
            request.provider_env_key,
            validate_start(&request)
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default()
        );
        assert!(!rendered.contains(SENTINEL));
    }

    #[test]
    fn a_real_credential_variable_is_refused_at_the_spawn_boundary() {
        let temp = tempdir().unwrap();
        let mut request = test_request(temp.path(), "spawn-boundary");
        request.api_key = "sk_live_should_never_be_exported".into();
        for name in CREDENTIAL_ENV_NAMES {
            request.provider_env_key = Some((*name).into());
            let error = provider_child_env(&request)
                .expect_err(&format!("{name} must never be exported to a Codex child"))
                .to_string();
            // The refusal names the variable and the way out of it.
            assert!(error.contains(name), "{error}");
            assert!(error.contains("credential broker"), "{error}");
            assert!(
                !error.contains("sk_live_should_never_be_exported"),
                "the refusal must not quote the credential: {error}"
            );
        }
        // The broker lease, and the local loopback token, still cross.
        request.provider_env_key = Some(credential_broker::LEASE_ENV_KEY.into());
        assert_eq!(
            provider_child_env(&request).unwrap(),
            Some((
                credential_broker::LEASE_ENV_KEY.to_owned(),
                "sk_live_should_never_be_exported".to_owned()
            ))
        );
        request.provider_env_key = None;
        request.api_key = String::new();
        assert_eq!(provider_child_env(&request).unwrap(), None);
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

    #[test]
    fn extracts_only_authoritative_per_turn_usage_shapes() {
        let snake_case = extract_turn_usage(&json!({
            "turn": {"usage": {
                "input_tokens": 100,
                "output_tokens": 25,
                "output_tokens_details": {"reasoning_tokens": 4}
            }}
        }))
        .unwrap();
        assert_eq!(snake_case.input_tokens, Some(100));
        assert_eq!(snake_case.output_tokens, Some(25));
        assert_eq!(snake_case.reasoning_tokens, Some(4));

        let camel_case = extract_turn_usage(&json!({
            "tokenUsage": {
                "totalUsage": {"inputTokens": 9999, "outputTokens": 9999},
                "lastUsage": {"inputTokens": 50, "outputTokens": 8, "cachedInputTokens": 20}
            }
        }))
        .unwrap();
        assert_eq!(camel_case.input_tokens, Some(50));
        assert_eq!(camel_case.output_tokens, Some(8));
        assert_eq!(camel_case.cached_input_tokens, Some(20));
        assert!(extract_turn_usage(&json!({
            "tokenUsage": {"totalUsage": {"inputTokens": 9999, "outputTokens": 9999}}
        }))
        .is_none());
    }

    #[test]
    fn recognizes_answer_deltas_but_not_reasoning_or_empty_events() {
        assert!(is_output_delta(
            "item/agentMessage/delta",
            &json!({"delta": "answer"})
        ));
        assert!(!is_output_delta(
            "item/reasoning/delta",
            &json!({"delta": "private reasoning"})
        ));
        assert!(!is_output_delta(
            "item/agentMessage/delta",
            &json!({"delta": ""})
        ));
    }

    // ---- settled Synth Cloud accounting at turn finalize ----
    // Session ids are unique per test: the broker's receipt store is
    // process-wide and these tests run in parallel.

    fn tracker_for(provider: &str, turn_id: &str) -> TurnPerformanceTracker {
        TurnPerformanceTracker {
            provider: provider.into(),
            model_id: "openrouter/poolside/laguna-s-2.1".into(),
            turn_id: turn_id.into(),
            started_at_ms: 1_000,
            first_output_at_ms: Some(1_100),
            last_output_at_ms: Some(1_900),
            usage: TurnTokenUsage {
                input_tokens: Some(1_000),
                cached_input_tokens: None,
                cache_write_tokens: None,
                reasoning_tokens: None,
                output_tokens: Some(200),
            },
        }
    }

    fn settled_receipt(
        session_id: &str,
        response_id: &str,
        cost_usd: Option<f64>,
    ) -> credential_broker::SettledReceipt {
        credential_broker::SettledReceipt {
            session_id: session_id.into(),
            provider_response_id: response_id.into(),
            model: Some("openrouter/poolside/laguna-s-2.1".into()),
            prompt_tokens: Some(500),
            completion_tokens: Some(100),
            cached_tokens: None,
            reasoning_tokens: None,
            cost_usd,
            completed_at_ms: 1_950,
        }
    }

    async fn finalize_turn(core: &Arc<CoreRuntime>, session_id: &str, provider: &str, turn: &str) {
        let trackers: PerformanceTrackers = Arc::default();
        trackers
            .lock()
            .await
            .insert(session_id.to_owned(), tracker_for(provider, turn));
        finalize_performance_tracker(core, &trackers, session_id, "completed", Some(2_000)).await;
    }

    async fn usage_totals(core: &Arc<CoreRuntime>) -> UsageBreakdown {
        UsageRecordsRepository::new(core.storage().database().clone())
            .summary("all".into(), None)
            .await
            .unwrap()
            .totals
    }

    #[test]
    fn settled_cost_sums_only_receipts_that_carried_money() {
        assert_eq!(settled_cost_from_receipts(&[]), None);
        assert_eq!(
            settled_cost_from_receipts(&[settled_receipt("s", "a", None)]),
            None
        );
        let mixed = [
            settled_receipt("s", "a", Some(0.01)),
            settled_receipt("s", "b", None),
            settled_receipt("s", "c", Some(0.02)),
        ];
        let sum = settled_cost_from_receipts(&mixed).unwrap();
        assert!((sum - 0.03).abs() < 1e-12, "{sum}");
    }

    #[tokio::test]
    async fn a_synth_cloud_turn_records_the_sum_of_its_settled_receipts() {
        let temp = tempdir().unwrap();
        let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
        let session = "wp4-cloud-settles";
        // One turn may make several upstream requests; their settled charges
        // sum, and a token-only receipt contributes no invented money.
        credential_broker::push_settled_receipt(settled_receipt(session, "resp-1", Some(0.01)));
        credential_broker::push_settled_receipt(settled_receipt(session, "resp-2", Some(0.02)));
        credential_broker::push_settled_receipt(settled_receipt(session, "resp-3", None));
        finalize_turn(&core, session, "synth-cloud", "turn-1").await;

        let totals = usage_totals(&core).await;
        assert_eq!(totals.requests, 1);
        assert!((totals.billed_cost_usd.unwrap() - 0.03).abs() < 1e-12);
        assert_eq!(totals.cost_source, CostSource::SynthCloud);
        // Tokens stay the tracker's own counts, and the settled charge left
        // nothing in the estimate column.
        assert_eq!(totals.input_tokens, 1_000);
        assert_eq!(totals.output_tokens, 200);
        assert_eq!(totals.estimated_cost_usd, None);
        // Drained: a replayed finalize finds nothing to double-bill.
        assert!(credential_broker::drain_settled_receipts(session).is_empty());
    }

    #[tokio::test]
    async fn cloud_receipts_without_money_leave_billed_unset() {
        let temp = tempdir().unwrap();
        let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
        let session = "wp4-cloud-token-only";
        credential_broker::push_settled_receipt(settled_receipt(session, "resp-1", None));
        finalize_turn(&core, session, "synth-cloud", "turn-1").await;

        let totals = usage_totals(&core).await;
        assert_eq!(totals.requests, 1);
        assert_eq!(totals.billed_cost_usd, None);
        assert_eq!(totals.cost_source, CostSource::None);
        assert_eq!(totals.input_tokens, 1_000);
    }

    #[tokio::test]
    async fn local_turns_neither_drain_receipts_nor_carry_any_charge() {
        let temp = tempdir().unwrap();
        let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
        let session = "wp4-local-untouched";
        // Even a stray receipt under a local session's id must not turn an
        // on-device row into a billed one — billed stays None, never $0.
        credential_broker::push_settled_receipt(settled_receipt(session, "resp-1", Some(0.42)));
        finalize_turn(&core, session, "local-laguna", "turn-1").await;

        let totals = usage_totals(&core).await;
        assert_eq!(totals.requests, 1);
        assert_eq!(totals.billed_cost_usd, None);
        assert_eq!(totals.estimated_cost_usd, None);
        assert_eq!(totals.cost_source, CostSource::None);
        // The local finalize did not consume the queue.
        assert_eq!(credential_broker::drain_settled_receipts(session).len(), 1);
    }

    /// The cancellation-race contract: a receipt landing after its turn
    /// finalized stays queued no longer than the session's next finalize, and
    /// never becomes a row of its own.
    #[tokio::test]
    async fn a_late_receipt_waits_for_the_next_finalize_and_never_invents_a_row() {
        let temp = tempdir().unwrap();
        let core = Arc::new(CoreRuntime::open(temp.path().join("core")).unwrap());
        let session = "wp4-late-receipt";
        finalize_turn(&core, session, "synth-cloud", "turn-1").await;
        let totals = usage_totals(&core).await;
        assert_eq!((totals.requests, totals.billed_cost_usd), (1, None));

        credential_broker::push_settled_receipt(settled_receipt(session, "resp-late", Some(0.05)));
        // Still exactly one row: a queued receipt is not a usage record.
        assert_eq!(usage_totals(&core).await.requests, 1);

        finalize_turn(&core, session, "synth-cloud", "turn-2").await;
        let totals = usage_totals(&core).await;
        assert_eq!(totals.requests, 2);
        assert!((totals.billed_cost_usd.unwrap() - 0.05).abs() < 1e-12);
        assert_eq!(totals.cost_source, CostSource::SynthCloud);
    }
}
