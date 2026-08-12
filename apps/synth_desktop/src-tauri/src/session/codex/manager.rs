//! CodexManager — SessionKind::Codex transport authority over app-server attachments.
use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    env, fs,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tauri::AppHandle;
use tokio::sync::{oneshot, Mutex, RwLock};

use crate::credential_broker;
use crate::domain::{
    RunCreate, RuntimeTarget, SessionCreate, SessionKind, SessionStatus, SessionTitleOrigin,
};
use crate::storage::{EventAppend, EventSource};

use super::event_pump::{spawn_server, EventPumpState, SpawnServerRequest};
use super::home::{
    apply_brokered_credential, automatic_thread_title, auto_compact_token_limit, codex_root,
    ensure_home, nested_id, responses_base_url, safe_component, session_info,
    validate_reasoning_effort, validate_start,
};
use super::proto::{
    default_approval_policy, default_sandbox, is_detached_failure, CodexApprovalDecisionRequest,
    CodexSessionInfo, CodexSessionRecord, CodexSessionRequest, CodexSessionStartRequest,
    CodexSteerRequest, CodexTurnFailure, CodexTurnSendRequest, CodexTurnStartRequest,
    CompactWaiters, ProviderTransport, Session, SessionDetached, DETACHED_MESSAGE, COMPACT_PROMPT,
};
use super::telemetry::{PerformanceTrackers, TurnPerformanceTracker, TurnTokenUsage};

pub struct CodexManager {
    pub(crate) sessions: Arc<RwLock<HashMap<String, Arc<Session>>>>,
    pub(crate) records: Arc<RwLock<HashMap<String, CodexSessionRecord>>>,
    /// Serializes attach + turn/start per session so no caller can observe the
    /// window between "the attachment exists" and "the turn is running".
    pub(crate) turn_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    pub(crate) compact_waiters: CompactWaiters,
    /// Session ids awaiting a `thread/compacted` notification, mapped to the
    /// UI source label (`manual` or `model_switch`). Auto token-threshold
    /// compaction leaves this empty and renders as "automatically compacted".
    pub(crate) pending_compact_sources: Arc<Mutex<HashMap<String, String>>>,
    pub(crate) performance_trackers: PerformanceTrackers,
    pub(crate) root: PathBuf,
    pub(crate) state_path: PathBuf,
    pub(crate) binary: PathBuf,
    pub(crate) persistence: crate::session::SessionPersistence,
}

impl CodexManager {
    pub fn new(core: Option<Arc<crate::core_runtime::CoreRuntime>>) -> Self {
        let root = codex_root();
        let binary = PathBuf::from(env::var("SYNTH_CODEX_BIN").unwrap_or_else(|_| "codex".into()));
        Self::with_paths(crate::session::SessionPersistence::from_core(core), root, binary)
    }

    pub(crate) fn with_paths(
        persistence: crate::session::SessionPersistence,
        root: PathBuf,
        binary: PathBuf,
    ) -> Self {
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
            persistence,
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
                performance_trackers: self.performance_trackers.clone(),
                attachment_id,
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
                Err(error)
                    if crate::error::error_is::<crate::error::DatabaseLocked>(&error)
                        && attempts < 5 =>
                {
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
                status: SessionStatus::Ready.as_str().into(),
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
        let create = SessionCreate {
            id: request.session_id.clone(),
            title,
            kind: SessionKind::Codex,
            target: RuntimeTarget::from_codex_provider(
                &session.provider_name,
                &session.model,
            ),
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
        let start_run = self.persistence.start_run(RunCreate {
            id: turn_id,
            session_id: request.session_id.clone(),
            mode: "codex_turn".into(),
            model: Some(session.model.clone()),
            adapter: None,
            metadata: json!({"threadId": session.thread_id, "effort": effort}),
            source: EventSource::Codex,
        });
        if let Ok(Ok(Some(mutation))) =
            tokio::time::timeout(std::time::Duration::from_secs(2), start_run).await
        {
            if let Some(event) = mutation.event {
                let _ = self.persistence.publish_event(&app, event).await;
            }
        }
        self.set_status(&request.session_id, SessionStatus::Running)
            .await?;
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
        self.set_status(session_id, SessionStatus::Closed).await?;
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
        self.persistence
            .notify_codex_event(&app, request.session_id, kind, payload)
            .await;
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
        for session_id in detached {
            let _ = self
                .persistence
                .interrupt_active_run(&session_id, "desktop_restarted")
                .await;
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
        let persist = self.persistence.append_and_emit(
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
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), persist).await;
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
    /// The JSON cache must never stay `running`, and any active SQLite run for
    /// this session belongs to a turn nobody can finish. SQLite session status
    /// flows through `SessionService` / `RunService` transitions — not direct
    /// string writes (`state_machines_have_explicit_transitions`).
    async fn reconcile_failed_turn_start(&self, session_id: &str, reason: &str) -> Result<()> {
        let was_running = self
            .records
            .read()
            .await
            .get(session_id)
            .is_some_and(|record| SessionStatus::Running.equals_str(&record.status));
        if was_running {
            self.set_status(session_id, SessionStatus::Interrupted)
                .await?;
        }
        let _ = self
            .persistence
            .interrupt_active_run(session_id, reason)
            .await;
        Ok(())
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

    async fn mark_detached_turn_interrupted(&self, session_id: &str) -> Result<()> {
        let should_change = self
            .records
            .read()
            .await
            .get(session_id)
            .is_some_and(|record| SessionStatus::Running.equals_str(&record.status));
        if should_change {
            self.set_status(session_id, SessionStatus::Interrupted)
                .await?;
            let _ = self
                .persistence
                .interrupt_active_run(session_id, "runtime_detached")
                .await;
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

    async fn persist_records(&self) -> Result<()> {
        super::home::persist_records(&self.records, &self.state_path).await
    }
}

pub(crate) fn reconcile_detached_status(status: &mut String, attached: bool) -> bool {
    if SessionStatus::Running.equals_str(status) && !attached {
        *status = SessionStatus::Interrupted.as_str().into();
        true
    } else {
        false
    }
}
