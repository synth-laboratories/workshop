//! CodexManager — SessionKind::Codex transport authority over app-server attachments.
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::{
    collections::HashMap, env, fs, future::Future, path::PathBuf, sync::Arc, time::Duration,
};
use tauri::AppHandle;
use tokio::sync::{oneshot, Mutex, RwLock};

use crate::credential_broker::{CredentialBroker, ReceiptStore};
use crate::domain::{
    PresentationField, RunCreate, RunStatus, RuntimeTarget, SessionCreate, SessionKind,
    SessionStatus, SessionTitleOrigin,
};
use crate::session::approval::ApprovalBroker;
use crate::storage::{AppEvent, EventAppend, EventSource};

use super::event_pump::{spawn_server, EventPumpState, SpawnServerRequest};
use super::home::{
    apply_brokered_credential, auto_compact_token_limit, automatic_thread_title, codex_root,
    ensure_home, install_local_laguna_catalog, nested_id, responses_base_url, safe_component,
    session_info, uniquify_title, validate_reasoning_effort, validate_start,
};
use super::proto::{
    default_approval_policy, default_sandbox, is_detached_failure, is_not_recorded_failure,
    CodexApprovalDecisionRequest, CodexSessionInfo, CodexSessionRecord, CodexSessionRequest,
    CodexSessionStartRequest, CodexSteerRequest, CodexThreadItemsRequest, CodexThreadReadRequest,
    CodexTurnFailure, CodexTurnSendRequest, CodexTurnStartRequest, CompactWaiters,
    ProviderTransport, RunNotPersisted, Session, SessionDetached, COMPACT_PROMPT, DETACHED_MESSAGE,
};
use super::telemetry::{PerformanceTrackers, TurnPerformanceTracker, TurnTokenUsage};

pub struct CodexManager {
    pub(crate) sessions: Arc<RwLock<HashMap<String, Arc<Session>>>>,
    pub(crate) records: Arc<RwLock<HashMap<String, CodexSessionRecord>>>,
    /// Starts may run concurrently, but account disconnect takes the write side
    /// so credential deletion and attachment fencing are one atomic lifecycle
    /// boundary. No child can attach in between the snapshot and the fence.
    attachment_lifecycle: RwLock<()>,
    /// Serializes attach + turn/start per session so no caller can observe the
    /// window between "the attachment exists" and "the turn is running".
    pub(crate) turn_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    pub(crate) compact_waiters: CompactWaiters,
    /// Session ids awaiting a `thread/compacted` notification, mapped to the
    /// UI source label (`manual` or `model_switch`). Auto token-threshold
    /// compaction leaves this empty and renders as "automatically compacted".
    pub(crate) pending_compact_sources: Arc<Mutex<HashMap<String, String>>>,
    /// A per-session `turn/start` handoff mailbox. `None` marks a start whose
    /// durable run is not registered yet; `Some` carries a terminal protocol
    /// notification that won that race.
    pub(crate) early_terminal_turns: Arc<Mutex<HashMap<String, Option<(String, Value)>>>>,
    pub(crate) performance_trackers: PerformanceTrackers,
    pub(crate) root: PathBuf,
    pub(crate) state_path: PathBuf,
    pub(crate) binary: PathBuf,
    pub(crate) persistence: crate::session::SessionPersistence,
    pub(crate) approvals: Arc<ApprovalBroker>,
    /// Injected loopback credential proxy (composition root). Every cloud
    /// session leases through this same Arc.
    pub(crate) broker: Arc<CredentialBroker>,
}

impl CodexManager {
    pub fn new(
        core: Option<Arc<crate::core_runtime::CoreRuntime>>,
        broker: Arc<CredentialBroker>,
        approvals: Arc<ApprovalBroker>,
    ) -> Self {
        let root = codex_root();
        let binary = PathBuf::from(env::var("SYNTH_CODEX_BIN").unwrap_or_else(|_| "codex".into()));
        Self::with_paths_and_approvals(
            crate::session::SessionPersistence::from_core(core),
            root,
            binary,
            broker,
            approvals,
        )
    }

    pub(crate) fn with_paths(
        persistence: crate::session::SessionPersistence,
        root: PathBuf,
        binary: PathBuf,
        broker: Arc<CredentialBroker>,
    ) -> Self {
        let approvals = Arc::new(ApprovalBroker::new(persistence.clone()));
        Self::with_paths_and_approvals(persistence, root, binary, broker, approvals)
    }

    fn with_paths_and_approvals(
        persistence: crate::session::SessionPersistence,
        root: PathBuf,
        binary: PathBuf,
        broker: Arc<CredentialBroker>,
        approvals: Arc<ApprovalBroker>,
    ) -> Self {
        let state_path = root.join("threads.json");
        let mut records: HashMap<String, CodexSessionRecord> = fs::read_to_string(&state_path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        // This cache — not SQLite — is what the renderer lists Codex chats
        // from at boot, so reconciling only the database left the sidebar
        // still showing Working. Correct it here, before `list()` can be
        // called, and carry the durable notice across so the UI can say what
        // happened rather than just "not running".
        let notices = persistence
            .database()
            .and_then(|database| {
                database
                    .with_conn(crate::recovery::pending_recovery_notices)
                    .ok()
            })
            .unwrap_or_default();
        let mut reconciled = 0_usize;
        for record in records.values_mut() {
            let notice = notices.get(&record.session_id);
            if !SessionStatus::Running.equals_str(&record.status) && notice.is_none() {
                continue;
            }
            if SessionStatus::Running.equals_str(&record.status) {
                record.status = SessionStatus::Interrupted.as_str().into();
                reconciled += 1;
            }
            record.recovery = notice.cloned();
        }
        if reconciled > 0 {
            eprintln!(
                "synth-desktop: {reconciled} Codex chat(s) were left running by a previous \
                 process and are now interrupted"
            );
            if let Ok(body) = serde_json::to_vec_pretty(&records) {
                let temporary = state_path.with_extension("json.tmp");
                if fs::write(&temporary, body).is_ok() {
                    let _ = fs::rename(temporary, &state_path);
                }
            }
        }
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            records: Arc::new(RwLock::new(records)),
            attachment_lifecycle: RwLock::new(()),
            turn_locks: Mutex::new(HashMap::new()),
            compact_waiters: Arc::new(Mutex::new(HashMap::new())),
            pending_compact_sources: Arc::new(Mutex::new(HashMap::new())),
            early_terminal_turns: Arc::new(Mutex::new(HashMap::new())),
            performance_trackers: Arc::new(Mutex::new(HashMap::new())),
            root,
            state_path,
            binary,
            persistence,
            approvals,
            broker,
        }
    }

    /// Receipt store shared with the broker's relay (composition-root Arc).
    pub(crate) fn receipts(&self) -> Arc<ReceiptStore> {
        self.broker.receipts()
    }

    /// Test helper: local broker + receipts, matching production injection.
    #[cfg(test)]
    pub(crate) fn test_broker() -> Arc<CredentialBroker> {
        Arc::new(
            CredentialBroker::start(Arc::new(ReceiptStore::new()))
                .expect("bind test credential broker"),
        )
    }

    pub async fn start<R: tauri::Runtime>(
        &self,
        app: AppHandle<R>,
        mut request: CodexSessionStartRequest,
    ) -> Result<CodexSessionInfo> {
        let _attachment_lifecycle = self.attachment_lifecycle.read().await;
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
            // not reused. The conversation itself continues, so only the
            // attachment is dropped — never the durable session.
            self.fence_attachment_inner(&request.session_id).await?;
        }
        // Custody is taken here — at spawn, after the reuse decision — and
        // nowhere earlier. Leasing during request preparation invalidated the
        // token a live, reused child was still presenting (a mid-conversation
        // 401), and on rebind the fence above revokes the previous lease,
        // so this is the one point where a fresh token's lifetime matches the
        // child's.
        let upstream_endpoint = request.base_url.clone();
        let upstream_credential = request.api_key.clone();
        if request.broker_credential {
            apply_brokered_credential(&mut request, &self.broker)
                .map_err(|message| anyhow!(message))?;
        }
        let home = self
            .root
            .join("homes")
            .join(safe_component(&request.session_id));
        install_local_laguna_catalog(&home, &request)?;
        ensure_home(&home, &request)?;
        let attachment_id = uuid::Uuid::new_v4();
        let server = spawn_server(
            app.clone(),
            SpawnServerRequest {
                binary: &self.binary,
                session_id: &request.session_id,
                home: &home,
                request: &request,
            },
            EventPumpState {
                records: self.records.clone(),
                state_path: self.state_path.clone(),
                persistence: self.persistence.clone(),
                sessions: self.sessions.clone(),
                compact_waiters: self.compact_waiters.clone(),
                pending_compact_sources: self.pending_compact_sources.clone(),
                early_terminal_turns: self.early_terminal_turns.clone(),
                performance_trackers: self.performance_trackers.clone(),
                receipts: self.receipts(),
                approvals: self.approvals.clone(),
                attachment_id,
                codex_oauth: super::home::provider_class(request.provider_name.as_deref())
                    == super::home::ProviderClass::OpenaiCodexOauth,
            },
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
                    if crate::error::error_is::<crate::error::DatabaseLocked>(&error)
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
        let mut method = if requested_thread.is_some() {
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
                Err(error)
                    if crate::error::error_is::<crate::error::DatabaseLocked>(&error)
                        && attempts < 5 =>
                {
                    attempts += 1;
                    tokio::time::sleep(std::time::Duration::from_millis(200 * attempts)).await;
                }
                Err(error)
                    if method == "thread/resume"
                        && error.to_string().contains("no rollout found for thread id") =>
                {
                    // A locally remembered thread can outlive the Codex rollout that
                    // backed it (for example after switching CODEX_HOME or clearing
                    // app-server state). Treat that exact resume miss as a stale
                    // pointer and create a replacement thread once. Other resume
                    // failures remain visible to the caller.
                    method = "thread/start";
                    if let Some(object) = params.as_object_mut() {
                        object.remove("threadId");
                    }
                    attempts = 0;
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
        // A desktop crash can leave CoreRuntime's last run marked active even
        // though this fresh manager owns no process or turn for it. Reattaching a
        // thread is the authoritative proof that the old attachment is gone. Close
        // that orphan before publishing Ready or accepting another turn; otherwise
        // start_run rejects the new turn after the model has already started it.
        if let Some(event) = self
            .persistence
            .interrupt_active_run(&request.session_id, "desktop_reattached")
            .await?
        {
            self.persistence.publish_event(&app, event).await?;
        }
        self.sessions
            .write()
            .await
            .insert(request.session_id.clone(), session.clone());
        let default_title = if request.provider_name.as_deref() == Some("local-laguna") {
            "Laguna XS".to_owned()
        } else {
            request.model.clone()
        };
        let title = if let Some(title) = remembered.as_ref().and_then(|record| record.title.clone())
        {
            title
        } else {
            let taken = self
                .records
                .read()
                .await
                .values()
                .filter_map(|record| record.title.clone())
                .collect::<Vec<_>>();
            uniquify_title(&default_title, taken.iter().map(String::as_str))
        };
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
                status: SessionStatus::Ready.as_str().into(),
                title: Some(title.clone()),
                title_origin: Some(title_origin.clone()),
                presentation_emotion: remembered
                    .as_ref()
                    .and_then(|record| record.presentation_emotion.clone()),
                presentation_summary: remembered
                    .as_ref()
                    .and_then(|record| record.presentation_summary.clone()),
                approval_policy: request
                    .approval_policy
                    .clone()
                    .unwrap_or_else(default_approval_policy),
                sandbox: request.sandbox.clone().unwrap_or_else(default_sandbox),
                // Reattaching does not resolve the previous attempt. The notice
                // survives until a new turn actually claims ownership, so a
                // chat that is merely reopened still explains itself.
                recovery: remembered
                    .as_ref()
                    .and_then(|record| record.recovery.clone()),
            },
        );
        self.persist_records().await?;
        let create = SessionCreate {
            id: request.session_id.clone(),
            title,
            kind: SessionKind::Codex,
            target: RuntimeTarget::from_codex_provider(&session.provider_name, &session.model),
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
                "presentationEmotion": remembered.as_ref().and_then(|record| record.presentation_emotion.clone()),
                "presentationSummary": remembered.as_ref().and_then(|record| record.presentation_summary.clone()),
            }),
            source: EventSource::Codex,
        };
        let persist_session = self.persistence.create_or_update_session(create);
        if let Ok(Ok(Some(mutation))) =
            tokio::time::timeout(std::time::Duration::from_secs(2), persist_session).await
        {
            if let Some(event) = mutation.event {
                let _ = self.persistence.publish_event(&app, event).await;
            }
        }
        if let Some(database) = self.persistence.database() {
            if crate::workspace_scope::get(&database, &request.session_id)
                .await?
                .is_none()
            {
                crate::workspace_scope::provision(
                    &database,
                    &request.session_id,
                    &request.workspace,
                )
                .await?;
            }
            crate::workspace_scope::mark_bound(&database, &request.session_id).await?;
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
    /// process is really gone the event pump finalizes the durable run before
    /// releasing the failed request back to this caller.
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
        // still preserves the text the operator typed. Reuse the renderer's
        // optimistic message id when provided so the transcript does not grow
        // a second bubble for the same submission.
        self.record_user_prompt(
            &app,
            &session_id,
            &request.prompt,
            request.client_message_id.as_deref(),
        )
        .await;

        // Compact-on-send model switch: while the live attachment is still the
        // source model, summarize before start() closes it and resumes as B.
        if let Err(error) = self.maybe_compact_before_model_switch(&request).await {
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
                        client_message_id: request.client_message_id.clone(),
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
        // A storage failure is not a provider failure: the upstream turn may
        // have completed normally. Report it as its own code so the transcript
        // never claims the agent disconnected, and keep the storage text out of
        // the user-facing message.
        if is_not_recorded_failure(&error) {
            return Err(CodexTurnFailure::not_recorded(&session_id, &error));
        }
        if !is_detached_failure(&error) {
            return Err(CodexTurnFailure::rejected(&session_id, &error));
        }
        self.persistence
            .notify_codex_event(
                &app,
                session_id.clone(),
                "session/unhealthy",
                json!({
                    "reason": "turn_start_detached",
                    "message": DETACHED_MESSAGE
                }),
            )
            .await;
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
            self.record_user_prompt(
                &app,
                &request.session_id,
                &request.prompt,
                request.client_message_id.as_deref(),
            )
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
        let receipt_scope = uuid::Uuid::new_v4().simple().to_string();
        self.broker.begin_turn(&request.session_id, &receipt_scope);
        self.performance_trackers.lock().await.insert(
            request.session_id.clone(),
            TurnPerformanceTracker {
                segments: super::generation_speed::TurnSegmentTracker::new(
                    request.session_id.clone(),
                    pending_turn_id.clone(),
                    Some(provider.clone()),
                    Some(session.model.clone()),
                ),
                provider,
                model_id: session.model.clone(),
                turn_id: pending_turn_id.clone(),
                receipt_scope,
                started_at_ms,
                first_output_at_ms: None,
                last_output_at_ms: None,
                usage: TurnTokenUsage::default(),
            },
        );
        // The map entry is both an ownership marker and a terminal mailbox.
        // The event pump updates it atomically if completion beats durable run
        // creation; after registration, absence means the pump owns terminals.
        self.early_terminal_turns
            .lock()
            .await
            .insert(request.session_id.clone(), None);
        let result = match session.server.request("turn/start", turn_params).await {
            Ok(result) => result,
            Err(error) => {
                self.early_terminal_turns
                    .lock()
                    .await
                    .remove(&request.session_id);
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
            self.early_terminal_turns
                .lock()
                .await
                .remove(&request.session_id);
            self.performance_trackers
                .lock()
                .await
                .remove(&request.session_id);
            return Err(anyhow!(
                "Codex turn/start response missing turn id: {result}"
            ));
        };
        if let Some(tracker) = self
            .performance_trackers
            .lock()
            .await
            .get_mut(&request.session_id)
        {
            if tracker.turn_id == pending_turn_id {
                tracker.turn_id = turn_id.clone();
                // Acceptance, rather than queue/send time, owns elapsed-work timing.
                tracker.started_at_ms = chrono::Utc::now().timestamp_millis();
            }
        }
        self.record_turn_accepted(
            &app,
            &request.session_id,
            &turn_id,
            request.client_message_id.as_deref(),
        )
        .await;
        *session.turn_id.write().await = Some(turn_id.clone());
        // Mark the session running before any queued terminal notification can
        // be observed. A fast app-server may emit its final answer in the same
        // stdout read cycle as the `turn/start` response.
        self.set_status(&request.session_id, SessionStatus::Running)
            .await?;
        self.set_automatic_title(&app, &request.session_id, &request.prompt, &session)
            .await;
        // A turn that replaces a crashed one stays visibly a *new* attempt: the
        // interrupted run keeps its history and this one records what it
        // continues. Flipping the old run back to running would erase the fact
        // that Workshop died mid-task.
        let recovery = self.persistence.pending_recovery(&request.session_id).await;
        let mut run_metadata = json!({"threadId": session.thread_id, "effort": effort});
        if let (Some(recovery), Some(object)) = (&recovery, run_metadata.as_object_mut()) {
            object.insert("recoveryAttempt".into(), json!(recovery.recovery_attempt));
            object.insert("recoveredAfterCrash".into(), json!(true));
            if let Some(previous) = &recovery.run_id {
                object.insert("recoveredFromRunId".into(), json!(previous));
            }
        }
        let start_run = self.persistence.start_run(RunCreate {
            id: turn_id.clone(),
            session_id: request.session_id.clone(),
            mode: "codex_turn".into(),
            model: Some(session.model.clone()),
            adapter: None,
            metadata: run_metadata,
            source: EventSource::Codex,
        });
        let mutation = match start_run.await {
            Ok(mutation) => mutation,
            Err(error) => {
                self.early_terminal_turns
                    .lock()
                    .await
                    .remove(&request.session_id);
                // The model turn already exists, but without its durable run the
                // product cannot represent or reconcile it. Interrupt best-effort
                // and surface the storage failure instead of claiming Running.
                let interrupt_error = session
                    .server
                    .request(
                        "turn/interrupt",
                        json!({"threadId": session.thread_id, "turnId": turn_id}),
                    )
                    .await
                    .err()
                    .map(|interrupt_error| interrupt_error.to_string());
                self.performance_trackers
                    .lock()
                    .await
                    .remove(&request.session_id);
                self.diagnose_turn_not_recorded(
                    &request.session_id,
                    &turn_id,
                    &error,
                    interrupt_error.as_deref(),
                );
                return Err(error.context(RunNotPersisted));
            }
        };
        self.record_turn_accepted(
            &app,
            &request.session_id,
            &turn_id,
            request.client_message_id.as_deref(),
        )
        .await;
        *session.turn_id.write().await = Some(turn_id.clone());
        // Mirror the committed run into the record cache. A fast app-server may
        // emit its final answer in the same stdout read cycle as the
        // `turn/start` response, so this must precede the early-terminal check.
        self.set_status(&request.session_id, SessionStatus::Running)
            .await?;
        self.set_automatic_title(&app, &request.session_id, &request.prompt, &session)
            .await;
        let early_terminal = {
            let mut terminals = self.early_terminal_turns.lock().await;
            match terminals.get(&request.session_id).cloned().flatten() {
                Some(terminal) => Some(terminal),
                None => {
                    terminals.remove(&request.session_id);
                    None
                }
            }
        };
        // The durable run now exists; claim it for this boot epoch. Until this
        // line lands, nothing may present the session as Working — that is the
        // whole point of the claim.
        if let Err(error) = self
            .persistence
            .claim_turn(
                &request.session_id,
                &turn_id,
                Some(session.attachment_id.to_string()),
            )
            .await
        {
            eprintln!("could not claim turn ownership for {turn_id}: {error}");
        }
        if recovery.is_some() {
            if let Some(record) = self.records.write().await.get_mut(&request.session_id) {
                record.recovery = None;
            }
            self.persist_records().await?;
        }
        crate::recovery::crash_checkpoint(crate::recovery::checkpoints::AFTER_TURN_START);
        if early_terminal.is_none() {
            if let Some(mutation) = mutation {
                if let Some(event) = mutation.event {
                    self.persistence.publish_event(&app, event).await?;
                }
            }
        }
        if let Some((terminal_method, params)) = early_terminal {
            let run_status = match terminal_method.as_str() {
                "turn/completed" => RunStatus::Completed,
                "turn/failed" => RunStatus::Failed,
                _ => RunStatus::Interrupted,
            };
            let session_status = session_status_for_run(run_status);
            if let Some(runs) = self.persistence.runs() {
                let mutation = runs
                    .transition(
                        turn_id.clone(),
                        run_status,
                        Some(params),
                        EventSource::Codex,
                    )
                    .await
                    .context("reconcile typed terminal received before run creation")?;
                if let Some(event) = mutation.event {
                    self.persistence.publish_event(&app, event).await?;
                }
            }
            self.set_status(&request.session_id, session_status).await?;
            self.early_terminal_turns
                .lock()
                .await
                .remove(&request.session_id);
            return Ok(session_info(&request.session_id, &session).await);
        }
        // If the terminal arrived outside the narrow typed-final window, its
        // durable journal row restores the exact terminal state. A later
        // terminal remains owned by the event pump.
        self.reconcile_terminal_before_run_start(&app, &request.session_id, &turn_id)
            .await?;
        Ok(session_info(&request.session_id, &session).await)
    }

    /// A fast app-server may emit its terminal notification before the
    /// `turn/start` response gives us the durable run id. The event pump cannot
    /// transition a run that does not exist yet, so reconcile that exact turn
    /// immediately after creation. A later terminal remains owned by the pump.
    async fn reconcile_terminal_before_run_start<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        session_id: &str,
        turn_id: &str,
    ) -> Result<bool> {
        let mut after = 0_i64;
        loop {
            let events = self
                .persistence
                .session_events_after(session_id.to_owned(), after, 500)
                .await?;
            if events.is_empty() {
                return Ok(false);
            }
            for event in &events {
                after = after.max(event.session_sequence.unwrap_or(event.sequence));
                let Some((status, outcome)) = terminal_run_for_turn(event, turn_id) else {
                    continue;
                };
                let Some(runs) = self.persistence.runs() else {
                    self.set_status(session_id, session_status_for_run(status))
                        .await?;
                    return Ok(true);
                };
                let Some(run) = runs.get(turn_id.to_owned()).await? else {
                    anyhow::bail!("durable Codex run {turn_id} disappeared during reconciliation");
                };
                if run.status == status.as_str() {
                    self.set_status(session_id, session_status_for_run(status))
                        .await?;
                    return Ok(true);
                }
                if let Some(session_status) = terminal_session_status(&run.status) {
                    self.set_status(session_id, session_status).await?;
                    return Ok(true);
                }
                let mutation = runs
                    .transition(
                        turn_id.to_owned(),
                        status,
                        Some(outcome),
                        EventSource::Codex,
                    )
                    .await
                    .context("reconcile terminal Codex event received before run creation")?;
                if let Some(event) = mutation.event {
                    self.persistence.publish_event(app, event).await?;
                }
                self.set_status(session_id, session_status_for_run(status))
                    .await?;
                return Ok(true);
            }
        }
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

    pub async fn interrupt<R: tauri::Runtime>(
        &self,
        app: AppHandle<R>,
        session_id: &str,
    ) -> Result<()> {
        let Some(session) = self.sessions.read().await.get(session_id).cloned() else {
            // Stop is idempotent once no live transport owns the turn.
            return Ok(());
        };
        let Some(turn_id) = session.turn_id.read().await.clone() else {
            return Ok(());
        };
        session
            .server
            .request(
                "turn/interrupt",
                json!({"threadId":session.thread_id,"turnId":turn_id}),
            )
            .await?;
        self.approvals
            .expire_session(&app, session_id, "origin_interrupted")
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
        self.record_user_prompt(&app, &request.session_id, &request.text, None)
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
        // Steering text is queued/journalled before the provider responds, but it
        // resets the work clock only after the active turn accepts it.
        self.record_turn_accepted(&app, &request.session_id, &turn_id, None)
            .await;
        Ok(())
    }

    /// Tears down the live attachment without touching durable session state.
    ///
    /// Attachment lifetime and conversation lifetime are different things. A
    /// rebind (model, provider, approval, sandbox or workspace change) must
    /// replace the child process while the conversation keeps running, so this
    /// path deliberately writes no status: `closed` is terminal in the session
    /// state machine, and recording it here left the next turn unable to create
    /// its durable run.
    pub async fn fence_attachment(&self, session_id: &str) -> Result<()> {
        let _attachment_lifecycle = self.attachment_lifecycle.write().await;
        self.fence_attachment_inner(session_id).await
    }

    async fn fence_attachment_inner(&self, session_id: &str) -> Result<()> {
        let session = self.sessions.write().await.remove(session_id);
        if let Some(session) = session {
            session.server.stop().await?;
        }
        // The child is gone; its loopback lease must not outlive it.
        self.broker.revoke(session_id);
        Ok(())
    }

    /// Runs a credential-authority mutation and fences its children without
    /// allowing an attachment start between those two operations.
    pub(crate) async fn fence_provider_attachments_after<T>(
        &self,
        provider_name: &str,
        authority_mutation: impl Future<Output = Result<T>>,
    ) -> Result<T> {
        let _attachment_lifecycle = self.attachment_lifecycle.write().await;
        let result = authority_mutation.await?;
        let session_ids: Vec<_> = self
            .sessions
            .read()
            .await
            .iter()
            .filter(|(_, session)| session.provider_name == provider_name)
            .map(|(session_id, _)| session_id.clone())
            .collect();
        for session_id in session_ids {
            self.fence_attachment_inner(&session_id).await?;
        }
        Ok(result)
    }

    /// Ends the conversation: drops the attachment *and* closes it durably.
    pub async fn close(&self, session_id: &str) -> Result<()> {
        self.fence_attachment(session_id).await?;
        self.set_status(session_id, SessionStatus::Closed).await?;
        Ok(())
    }

    pub async fn resolve_approval<R: tauri::Runtime>(
        &self,
        app: AppHandle<R>,
        request: CodexApprovalDecisionRequest,
    ) -> Result<()> {
        let decision = self
            .approvals
            .decision_from_shell(&request.approval_id, &request.decision)
            .await?;
        self.approvals
            .resolve(&app, &request.session_id, &request.approval_id, decision)
            .await?;
        Ok(())
    }

    pub async fn expire_restored_approvals<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
    ) -> Result<usize> {
        self.approvals.expire_restored(app).await
    }

    pub async fn list(&self) -> Vec<CodexSessionRecord> {
        let mut records: Vec<_> = self.records.read().await.values().cloned().collect();
        records.sort_by(|left, right| left.session_id.cmp(&right.session_id));
        records
    }

    pub async fn read_thread(&self, request: CodexThreadReadRequest) -> Result<Value> {
        let session = self
            .assert_thread_readable(&request.session_id, &request.thread_id)
            .await?;
        session
            .server
            .request(
                "thread/read",
                json!({
                    "threadId": request.thread_id,
                    "includeTurns": request.include_turns,
                }),
            )
            .await
    }

    pub async fn list_thread_items(&self, request: CodexThreadItemsRequest) -> Result<Value> {
        let session = self
            .assert_thread_readable(&request.session_id, &request.thread_id)
            .await?;
        let mut params = json!({ "threadId": request.thread_id });
        if let Some(cursor) = request.cursor {
            params["cursor"] = Value::String(cursor);
        }
        if let Some(limit) = request.limit {
            params["limit"] = json!(limit);
        }
        session.server.request("thread/items/list", params).await
    }

    async fn assert_thread_readable(
        &self,
        session_id: &str,
        thread_id: &str,
    ) -> Result<Arc<Session>> {
        let session = self.session(session_id).await?;
        if session.thread_id == thread_id {
            return Ok(session);
        }
        let events = self
            .persistence
            .session_events_after(session_id.to_owned(), 0, 10_000)
            .await?;
        let owned = events.iter().any(|event| {
            payload_mentions_thread(&event.payload, thread_id)
                || event.payload.get("threadId").and_then(Value::as_str) == Some(thread_id)
                || event.payload.get("thread_id").and_then(Value::as_str) == Some(thread_id)
        });
        if !owned {
            anyhow::bail!("thread {thread_id} is not owned by session {session_id}");
        }
        Ok(session)
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
        client_message_id: Option<&str>,
    ) {
        let message_id = client_message_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(|id| id.to_owned())
            .unwrap_or_else(|| format!("user-{}", uuid::Uuid::new_v4()));
        let persist = self.persistence.append_and_emit(
            app,
            EventAppend::codex(
                session_id.to_owned(),
                "message.created",
                json!({
                    "messageId": message_id,
                    "role": "user",
                    "content": prompt,
                }),
            ),
        );
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), persist).await;
    }

    /// Durable clock boundary for an accepted active turn. User bubbles may be
    /// journalled optimistically before provider acceptance; they intentionally
    /// do not own elapsed-work timing.
    async fn record_turn_accepted<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        session_id: &str,
        turn_id: &str,
        client_message_id: Option<&str>,
    ) {
        let persist = self.persistence.append_and_emit(
            app,
            EventAppend::codex(
                session_id.to_owned(),
                "turn/accepted",
                json!({ "turnId": turn_id, "userMessageId": client_message_id }),
            ),
        );
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), persist).await;
    }

    /// Removes a dead attachment, but only when it is still the current one.
    /// `expected` is `None` when no attachment was resolved at all.
    /// Records the exact failure the agent needs when an upstream turn cannot be
    /// anchored durably. Without this the only trace of the failure was a
    /// rejected composer message.
    fn diagnose_turn_not_recorded(
        &self,
        session_id: &str,
        turn_id: &str,
        error: &anyhow::Error,
        interrupt_error: Option<&str>,
    ) {
        let Some(service) = self.persistence.diagnostics() else {
            return;
        };
        let mut input = crate::diagnostics::DiagnosticInput::new(
            crate::diagnostics::Severity::Error,
            "session",
            "session.turn.not_recorded",
            crate::diagnostics::codes::TURN_NOT_RECORDED,
            "the provider turn started but could not be recorded as a run",
        )
        .retryable(true);
        input.correlation.session_id = Some(session_id.to_owned());
        input.correlation.turn_id = Some(turn_id.to_owned());
        input
            .details
            .insert("cause".into(), Value::String(error.to_string()));
        input.details.insert(
            "provider_interrupt".into(),
            Value::String(match interrupt_error {
                Some(error) => format!("request_failed: {error}"),
                None => "requested".into(),
            }),
        );
        service.emit(input);
    }

    async fn discard_attachment(&self, session_id: &str, expected: Option<uuid::Uuid>) {
        let mut sessions = self.sessions.write().await;
        let matches = sessions.get(session_id).is_some_and(|session| {
            expected.is_none_or(|attachment_id| session.attachment_id == attachment_id)
        });
        if matches {
            sessions.remove(session_id);
        }
    }

    /// Updates the threads.json cache and, when CoreRuntime is bound, mirrors
    /// the change through `SessionService::transition` so SQLite remains the
    /// Session authority. Cache-only Desktop installs (no core) keep threads.json
    /// as a temporary local mirror until Wave 4 demotes it entirely.
    async fn set_status(&self, session_id: &str, status: SessionStatus) -> Result<()> {
        if let Some(record) = self.records.write().await.get_mut(session_id) {
            record.status = status.as_str().into();
        }
        self.persist_records().await?;
        // Leaving a claim behind after the turn ended would keep the lease
        // watchdog reporting work nobody is doing. Every status that is not
        // Running is a terminal for the claim, whatever produced it.
        if status != SessionStatus::Running {
            self.persistence.release_turn(session_id).await;
        }
        if let Ok(Some(mutation)) = self
            .persistence
            .transition_session(
                session_id.to_owned(),
                status,
                EventSource::Codex,
                json!({ "source": "codex_manager" }),
            )
            .await
        {
            self.persistence.broadcast_committed(mutation.event);
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
        let Some(base) = automatic_thread_title(prompt) else {
            return;
        };
        let taken = {
            let records = self.records.read().await;
            records
                .iter()
                .filter(|(id, _)| id.as_str() != session_id)
                .filter_map(|(_, record)| record.title.clone())
                .collect::<Vec<_>>()
        };
        let title = uniquify_title(&base, taken.iter().map(String::as_str));
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
        if let Ok(Some(mutation)) = self
            .persistence
            .set_title(
                session_id.to_owned(),
                title.clone(),
                SessionTitleOrigin::Automatic,
            )
            .await
        {
            if let Some(event) = mutation.event {
                let _ = self.persistence.publish_event(app, event).await;
            }
        }
    }

    /// Sets the Codex thread name and commits a manual CoreRuntime title.
    /// Live attachments call `thread/name/set`; cold sessions still persist.
    pub async fn set_thread_name<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        session_id: &str,
        title: String,
    ) -> Result<()> {
        let title = title.trim().to_owned();
        if title.is_empty() {
            anyhow::bail!("session title must not be empty");
        }
        if let Some(session) = self.sessions.read().await.get(session_id).cloned() {
            session
                .server
                .request(
                    "thread/name/set",
                    json!({"threadId": session.thread_id, "name": title}),
                )
                .await?;
        }
        {
            let mut records = self.records.write().await;
            if let Some(record) = records.get_mut(session_id) {
                record.title = Some(title.clone());
                record.title_origin = Some("manual".into());
            }
        }
        let _ = self.persist_records().await;
        if let Ok(Some(mutation)) = self
            .persistence
            .set_title(session_id.to_owned(), title, SessionTitleOrigin::Manual)
            .await
        {
            if let Some(event) = mutation.event {
                let _ = self.persistence.publish_event(app, event).await;
            }
        }
        Ok(())
    }

    /// Persists the mascot overlay on the Codex record and CoreRuntime session.
    pub async fn set_presentation<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        session_id: &str,
        emotion: PresentationField<String>,
        summary: PresentationField<String>,
    ) -> Result<serde_json::Value> {
        let mutation = self
            .persistence
            .set_presentation(session_id.to_owned(), emotion.clone(), summary.clone())
            .await?;
        if let Some(event) = mutation.as_ref().and_then(|item| item.event.clone()) {
            let _ = self.persistence.publish_event(app, event).await;
        }
        {
            let mut records = self.records.write().await;
            if let Some(record) = records.get_mut(session_id) {
                if let Some(value) = mutation.as_ref() {
                    record.presentation_emotion = value
                        .value
                        .metadata
                        .get("presentationEmotion")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    record.presentation_summary = value
                        .value
                        .metadata
                        .get("presentationSummary")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                } else {
                    match &emotion {
                        PresentationField::Unchanged => {}
                        PresentationField::Clear => record.presentation_emotion = None,
                        PresentationField::Set(value) => {
                            record.presentation_emotion = Some(value.clone());
                        }
                    }
                    match &summary {
                        PresentationField::Unchanged => {}
                        PresentationField::Clear => record.presentation_summary = None,
                        PresentationField::Set(value) => {
                            record.presentation_summary = Some(value.clone());
                        }
                    }
                }
            }
        }
        let _ = self.persist_records().await;
        let record = mutation.map(|item| item.value);
        let fallback = self.records.read().await.get(session_id).cloned();
        Ok(json!({
            "sessionId": session_id,
            "title": record.as_ref().map(|item| item.title.clone())
                .or_else(|| fallback.as_ref().and_then(|item| item.title.clone())),
            "emotion": record.as_ref().and_then(|item| item.metadata.get("presentationEmotion").cloned())
                .or_else(|| fallback.as_ref().and_then(|item| item.presentation_emotion.clone().map(Value::String))),
            "summary": record.as_ref().and_then(|item| item.metadata.get("presentationSummary").cloned())
                .or_else(|| fallback.as_ref().and_then(|item| item.presentation_summary.clone().map(Value::String))),
        }))
    }

    async fn persist_records(&self) -> Result<()> {
        super::home::persist_records(&self.records, &self.state_path).await
    }
}

fn terminal_run_for_turn(event: &AppEvent, turn_id: &str) -> Option<(RunStatus, Value)> {
    let status = match event.kind.as_str() {
        "turn/completed" => RunStatus::Completed,
        "turn/failed" => RunStatus::Failed,
        "turn/interrupted" => RunStatus::Interrupted,
        _ => return None,
    };
    let observed_turn_id = event
        .payload
        .pointer("/turn/id")
        .or_else(|| event.payload.get("turnId"))
        .or_else(|| event.payload.get("turn_id"))
        .and_then(Value::as_str)?;
    (observed_turn_id == turn_id).then(|| (status, event.payload.clone()))
}

fn session_status_for_run(status: RunStatus) -> SessionStatus {
    match status {
        RunStatus::Completed => SessionStatus::Ready,
        RunStatus::Failed => SessionStatus::Failed,
        RunStatus::Interrupted | RunStatus::Cancelled => SessionStatus::Interrupted,
        RunStatus::Created | RunStatus::Running => SessionStatus::Running,
    }
}

fn terminal_session_status(status: &str) -> Option<SessionStatus> {
    match status {
        "completed" => Some(SessionStatus::Ready),
        "failed" => Some(SessionStatus::Failed),
        "interrupted" | "cancelled" => Some(SessionStatus::Interrupted),
        _ => None,
    }
}

fn payload_mentions_thread(payload: &Value, thread_id: &str) -> bool {
    fn walk(value: &Value, thread_id: &str) -> bool {
        match value {
            Value::String(text) => text == thread_id,
            Value::Array(items) => items.iter().any(|item| walk(item, thread_id)),
            Value::Object(map) => {
                for (key, nested) in map {
                    if matches!(
                        key.as_str(),
                        "threadId"
                            | "thread_id"
                            | "agentThreadId"
                            | "agent_thread_id"
                            | "receiverThreadIds"
                            | "receiver_thread_ids"
                    ) && walk(nested, thread_id)
                    {
                        return true;
                    }
                    if walk(nested, thread_id)
                        && matches!(key.as_str(), "item" | "params" | "payload")
                    {
                        return true;
                    }
                }
                false
            }
            _ => false,
        }
    }
    walk(payload, thread_id)
}
