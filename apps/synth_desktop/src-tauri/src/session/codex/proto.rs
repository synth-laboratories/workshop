//! Codex request/response DTOs and the app-server ProviderTransport.
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fmt,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::{
    process::Child,
    sync::{oneshot, Mutex, RwLock},
};
use crate::contract::events::EventChannel;
use crate::synth_config::MultiAgentVersion;

pub(crate) const EVENT_NAME: &str = EventChannel::CODEX;
pub(crate) const MIN_AUTO_COMPACT_TOKEN_LIMIT: u64 = 16_000;
pub(crate) const COMPACT_PROMPT: &str = "You are performing a CONTEXT CHECKPOINT COMPACTION for a coding agent.\nWrite a handoff for another LLM that will continue the same workspace task.\nInclude:\n- Goal and acceptance criteria\n- Files read/changed (paths + one-line why)\n- Commands/tests run and outcomes\n- Decisions and constraints\n- Open bugs / next concrete steps\n- Any secrets-safe identifiers (branch names, ticket ids) needed to continue\nOmit raw file dumps, full command logs, and superseded plans.\nBe concise and structured (bullets).";

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
pub(crate) const DETACHED_MESSAGE: &str =
    "The local agent process disconnected before the turn started. Retry to reconnect.";
pub(crate) const STDOUT_CLOSED: &str = "codex app-server stdout closed";

impl CodexTurnFailure {
    pub(crate) fn detached(session_id: &str, detail: String) -> Self {
        Self {
            code: CODEX_SESSION_DETACHED.into(),
            message: DETACHED_MESSAGE.into(),
            session_id: session_id.to_owned(),
            detail,
        }
    }

    pub(crate) fn rejected(session_id: &str, error: &anyhow::Error) -> Self {
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
pub(crate) struct SessionDetached;

impl std::fmt::Display for SessionDetached {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("the local codex app-server is not attached")
    }
}

impl std::error::Error for SessionDetached {}

pub(crate) fn is_detached_failure(error: &anyhow::Error) -> bool {
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
pub(crate) struct CodexEvent {
    pub(crate) session_id: String,
    pub(crate) method: String,
    pub(crate) params: Value,
}

pub(crate) type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;
/// Waiters for `thread/compacted`, keyed by Desktop session id.
/// Model-switch compaction waiters. A `thread/compacted` notification means
/// the summary exists, but the source app-server still owns an active
/// compaction turn until its terminal turn event arrives. Rebinding before
/// that terminal event can consume the user's destination prompt as the
/// compaction turn and leave no answer.
pub(crate) type CompactWaiters = Arc<Mutex<HashMap<String, oneshot::Sender<Result<(), String>>>>>;

#[derive(Clone, Debug)]
pub(crate) struct PendingApproval {
    pub(crate) rpc_id: Value,
    pub(crate) available_decisions: Vec<String>,
}

pub(crate) type PendingApprovals = Arc<Mutex<HashMap<String, PendingApproval>>>;


pub(crate) struct AppServer {
    pub(crate) child: Mutex<Child>,
    pub(crate) stdin: Arc<Mutex<tokio::process::ChildStdin>>,
    pub(crate) pending: Pending,
    pub(crate) approvals: PendingApprovals,
    pub(crate) next_id: AtomicU64,
}

impl Drop for AppServer {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.try_lock() {
            let _ = child.start_kill();
        }
    }
}

impl AppServer {
    pub(crate) async fn perform_request(&self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        if let Err(error) = super::event_pump::write_message(
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

    pub(crate) async fn perform_notify(&self, method: &str) -> Result<()> {
        super::event_pump::write_message(&self.stdin, &json!({"jsonrpc":"2.0","method":method})).await
    }

    pub(crate) async fn perform_stop(&self) -> Result<()> {
        self.child
            .lock()
            .await
            .kill()
            .await
            .context("stop app-server")
    }

    pub(crate) async fn perform_resolve_approval(&self, approval_id: &str, requested: &str) -> Result<String> {
        let pending = self
            .approvals
            .lock()
            .await
            .get(approval_id)
            .cloned()
            .ok_or_else(|| anyhow!("approval is no longer pending: {approval_id}"))?;
        let decision = select_approval_decision(&pending.available_decisions, requested)?;
        super::event_pump::write_message(
            &self.stdin,
            &json!({"jsonrpc":"2.0","id":pending.rpc_id,"result":{"decision":decision}}),
        )
        .await?;
        self.approvals.lock().await.remove(approval_id);
        Ok(decision)
    }
}

/// App-server IO surface. Codex is the SessionKind transport; this trait is the
/// extension point (`protocols_for_extension_points`) — not a product noun.
pub trait ProviderTransport: Send + Sync {
    fn request(
        &self,
        method: &str,
        params: Value,
    ) -> impl std::future::Future<Output = Result<Value>> + Send;

    fn notify(&self, method: &str) -> impl std::future::Future<Output = Result<()>> + Send;

    fn stop(&self) -> impl std::future::Future<Output = Result<()>> + Send;

    fn resolve_approval(
        &self,
        approval_id: &str,
        requested: &str,
    ) -> impl std::future::Future<Output = Result<String>> + Send;
}

impl ProviderTransport for AppServer {
    async fn request(&self, method: &str, params: Value) -> Result<Value> {
        self.perform_request(method, params).await
    }

    async fn notify(&self, method: &str) -> Result<()> {
        self.perform_notify(method).await
    }

    async fn stop(&self) -> Result<()> {
        self.perform_stop().await
    }

    async fn resolve_approval(&self, approval_id: &str, requested: &str) -> Result<String> {
        self.perform_resolve_approval(approval_id, requested).await
    }
}


pub(crate) fn select_approval_decision(available: &[String], requested: &str) -> Result<String> {
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

pub(crate) fn default_approval_policy() -> String {
    "untrusted".into()
}
pub(crate) fn default_sandbox() -> String {
    "workspace-write".into()
}

pub(crate) struct Session {
    pub(crate) attachment_id: uuid::Uuid,
    pub(crate) server: Arc<AppServer>,
    pub(crate) thread_id: String,
    pub(crate) turn_id: RwLock<Option<String>>,
    pub(crate) model: String,
    pub(crate) approval_policy: String,
    pub(crate) sandbox: String,
    pub(crate) workspace: String,
    /// The provider binding the child was spawned against, as staged by
    /// `prepare_codex_start` — the upstream endpoint and credential *before*
    /// brokering. Held in memory only, never serialized, and compared on
    /// reuse so a rotated credential or endpoint respawns the child instead
    /// of leaving it bound to the old provider.
    pub(crate) upstream_endpoint: String,
    pub(crate) upstream_credential: String,
    /// The provider identity the child was spawned under. This is the sole
    /// input to `provider_class`, which gates the settled-receipt drain at
    /// finalize — so a name change must respawn (and thereby revoke, which
    /// discards queued receipts) even if endpoint, credential and model all
    /// coincide across the two providers.
    pub(crate) provider_name: String,
}
