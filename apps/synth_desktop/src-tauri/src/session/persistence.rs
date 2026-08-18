//! Session durable write surface for provider transports.
//!
//! Replaces `Option<Arc<CoreRuntime>>` + per-call persistence branches in Codex.
//! `Null` no-ops every method so call sites always go through one type.

use crate::contract::events::{
    origin_for_boundary_kind, origin_for_source_and_kind, tag_event, EventChannel,
};
use crate::core_runtime::CoreRuntime;
use crate::domain::{
    DomainMutation, PresentationField, RunCreate, RunService, RunStatus, SessionCreate,
    SessionService, SessionStatus, SessionTitleOrigin,
};
use crate::storage::{
    AppEvent, Database, EventAppend, EventSource, RunRecord, SessionRecord, UsageRecord,
    UsageRecordsRepository, APP_EVENT_SCHEMA_VERSION,
};
use anyhow::Result;
use chrono::Utc;
use serde_json::Value;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

/// Capability surface Codex (and later Intern) use instead of holding CoreRuntime.
#[derive(Clone)]
pub enum SessionPersistence {
    Core(Arc<CoreRuntime>),
    Null,
}

impl SessionPersistence {
    pub fn from_core(core: Option<Arc<CoreRuntime>>) -> Self {
        match core {
            Some(core) => Self::Core(core),
            None => Self::Null,
        }
    }

    pub fn database(&self) -> Option<Arc<Database>> {
        match self {
            Self::Core(core) => Some(core.storage().database().clone()),
            Self::Null => None,
        }
    }

    pub async fn append_and_emit<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        append: EventAppend,
    ) -> Result<Option<AppEvent>> {
        match self {
            Self::Core(core) => Ok(Some(core.append_and_emit(app, append).await?)),
            Self::Null => Ok(None),
        }
    }

    /// One emission channel for Codex boundary notifications.
    ///
    /// Journals (when Core is bound) so the forwarder emits a single
    /// origin-tagged `runtime:event`. With Null persistence, emits the same
    /// envelope once directly — never also on deprecated `codex:event`.
    pub async fn notify_codex_event<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        session_id: impl Into<String>,
        method: impl Into<String>,
        params: Value,
    ) {
        let session_id = session_id.into();
        let method = method.into();
        let origin = origin_for_boundary_kind(&method);
        // Every Codex boundary notification passes through here, which makes it
        // the one place a provider going unhealthy can be recorded without
        // instrumenting each caller. Only the unhealthy transitions are worth a
        // diagnostic; the healthy ones are the transcript.
        if method == "session/unhealthy" {
            self.diagnose_unhealthy(&session_id, &params);
        }
        match self
            .append_and_emit(
                app,
                EventAppend::codex(session_id.clone(), method.clone(), params.clone()),
            )
            .await
        {
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => {
                let event = ephemeral_codex_app_event(&session_id, &method, params);
                let _ = app.emit(EventChannel::RUNTIME, &tag_event(origin, event));
            }
        }
    }

    /// Local diagnostics, when a runtime is bound.
    pub fn diagnostics(&self) -> Option<&Arc<crate::diagnostics::DiagnosticsService>> {
        match self {
            Self::Core(core) => Some(core.diagnostics_service()),
            Self::Null => None,
        }
    }

    fn diagnose_unhealthy(&self, session_id: &str, params: &Value) {
        let Some(service) = self.diagnostics() else {
            return;
        };
        let reason = params
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("unhealthy");
        // A stalled provider and a dropped connection are different failures
        // with different remediations; keep them apart at the code.
        let stalled = reason.contains("stall") || reason.contains("idle");
        let mut input = crate::diagnostics::DiagnosticInput::new(
            crate::diagnostics::Severity::Error,
            "provider",
            "provider.session.unhealthy",
            if stalled {
                crate::diagnostics::codes::PROVIDER_STALLED
            } else {
                crate::diagnostics::codes::PROVIDER_DISCONNECTED
            },
            params
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("the provider session became unhealthy"),
        )
        .retryable(true);
        input.correlation.session_id = Some(session_id.to_owned());
        input
            .details
            .insert("reason".into(), Value::String(reason.to_owned()));
        service.emit(input);
    }

    /// Strict persist-before-publish boundary used by the approval broker.
    /// Unlike the compatibility notification path above, a Core write error is
    /// returned so a request is never shown without a durable lifecycle row.
    pub async fn append_boundary_event<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        session_id: String,
        source: EventSource,
        kind: impl Into<String>,
        payload: Value,
    ) -> Result<()> {
        let kind = kind.into();
        match self {
            Self::Core(core) => {
                core.append_and_emit(
                    app,
                    EventAppend {
                        event_id: None,
                        session_id: Some(session_id),
                        run_id: None,
                        source,
                        kind,
                        payload,
                        remote_sequence: None,
                        command_id: None,
                        created_at: None,
                    },
                )
                .await?;
            }
            Self::Null => {
                let event = ephemeral_app_event(&session_id, source, &kind, payload);
                let origin = origin_for_source_and_kind(event.source.as_str(), &event.kind);
                let _ = app.emit(EventChannel::RUNTIME, &tag_event(origin, event));
            }
        }
        Ok(())
    }

    pub async fn events_after(&self, after_sequence: i64, limit: i64) -> Result<Vec<AppEvent>> {
        match self {
            Self::Core(core) => core.journal().events_after(after_sequence, limit).await,
            Self::Null => Ok(Vec::new()),
        }
    }

    pub async fn session_events_after(
        &self,
        session_id: String,
        after_sequence: i64,
        limit: i64,
    ) -> Result<Vec<AppEvent>> {
        match self {
            Self::Core(core) => {
                core.journal()
                    .session_events_after(session_id, after_sequence, limit)
                    .await
            }
            Self::Null => Ok(Vec::new()),
        }
    }

    pub async fn events_of_kinds_after(
        &self,
        after_sequence: i64,
        kinds: Vec<String>,
        limit: i64,
    ) -> Result<Vec<AppEvent>> {
        match self {
            Self::Core(core) => {
                core.journal()
                    .events_of_kinds_after(after_sequence, kinds, limit)
                    .await
            }
            Self::Null => Ok(Vec::new()),
        }
    }

    pub fn broadcast_committed(&self, event: Option<AppEvent>) {
        if let Self::Core(core) = self {
            core.broadcast_committed(event);
        }
    }

    pub async fn publish_event<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        event: AppEvent,
    ) -> Result<()> {
        match self {
            Self::Core(core) => {
                core.publish_event(app, event).await?;
                Ok(())
            }
            Self::Null => Ok(()),
        }
    }

    pub async fn create_or_update_session(
        &self,
        create: SessionCreate,
    ) -> Result<Option<DomainMutation<SessionRecord>>> {
        let Self::Core(core) = self else {
            return Ok(None);
        };
        Ok(Some(core.sessions().create_or_update(create).await?))
    }

    pub async fn start_run(&self, create: RunCreate) -> Result<Option<DomainMutation<RunRecord>>> {
        let Self::Core(core) = self else {
            return Ok(None);
        };
        Ok(Some(core.runs().start(create).await?))
    }

    pub async fn transition_session(
        &self,
        session_id: String,
        status: SessionStatus,
        source: EventSource,
        metadata: Value,
    ) -> Result<Option<DomainMutation<SessionRecord>>> {
        let Self::Core(core) = self else {
            return Ok(None);
        };
        match core
            .sessions()
            .transition(session_id.clone(), status, source, metadata)
            .await
        {
            Ok(mutation) => Ok(Some(mutation)),
            // Callers have already updated their cache, so a rejected durable
            // edge is not fatal here — but it must never be invisible. Silently
            // dropping it is what let a session sit at `closed` while the app
            // believed it was running, and surfaced later as an unrelated
            // storage error on the next run.
            Err(error) => {
                self.diagnose_rejected_transition(&session_id, status, &error);
                Ok(None)
            }
        }
    }

    fn diagnose_rejected_transition(
        &self,
        session_id: &str,
        status: SessionStatus,
        error: &anyhow::Error,
    ) {
        let Some(service) = self.diagnostics() else {
            return;
        };
        let mut input = crate::diagnostics::DiagnosticInput::new(
            crate::diagnostics::Severity::Error,
            "session",
            "session.transition.rejected",
            crate::diagnostics::codes::SESSION_TRANSITION_REJECTED,
            format!("the durable session could not move to {}", status.as_str()),
        )
        .retryable(false);
        input.correlation.session_id = Some(session_id.to_owned());
        input.details.insert(
            "requestedStatus".into(),
            Value::String(status.as_str().to_owned()),
        );
        input
            .details
            .insert("cause".into(), Value::String(error.to_string()));
        service.emit(input);
    }

    pub async fn set_title(
        &self,
        session_id: String,
        title: String,
        origin: SessionTitleOrigin,
    ) -> Result<Option<DomainMutation<SessionRecord>>> {
        let Self::Core(core) = self else {
            return Ok(None);
        };
        Ok(Some(
            core.sessions().set_title(session_id, title, origin).await?,
        ))
    }

    pub async fn set_presentation(
        &self,
        session_id: String,
        emotion: PresentationField<String>,
        summary: PresentationField<String>,
    ) -> Result<Option<DomainMutation<SessionRecord>>> {
        let Self::Core(core) = self else {
            return Ok(None);
        };
        Ok(Some(
            core.sessions()
                .set_presentation(session_id, emotion, summary)
                .await?,
        ))
    }

    pub async fn get_session(&self, session_id: String) -> Result<Option<SessionRecord>> {
        let Self::Core(core) = self else {
            return Ok(None);
        };
        core.sessions().get(session_id).await
    }

    pub fn runs(&self) -> Option<&RunService> {
        match self {
            Self::Core(core) => Some(core.runs()),
            Self::Null => None,
        }
    }

    pub fn sessions(&self) -> Option<&SessionService> {
        match self {
            Self::Core(core) => Some(core.sessions()),
            Self::Null => None,
        }
    }

    pub async fn interrupt_active_run(
        &self,
        session_id: &str,
        reason: &str,
    ) -> Result<Option<AppEvent>> {
        let Self::Core(core) = self else {
            return Ok(None);
        };
        let Some(session) = core.sessions().get(session_id.to_owned()).await? else {
            return Ok(None);
        };
        let Some(run_id) = session.active_run_id else {
            return Ok(None);
        };
        // Reconciling an orphan is idempotent: between reading the session and
        // acting, the run may have reached its own terminal state and cleared
        // itself from the session. That authoritative outcome is the one to
        // keep — interrupting it is neither possible nor wanted, and treating
        // the refused edge as an error would reject the caller's new turn over
        // a pointer that is already gone.
        //
        // Pre-checking is not enough on its own. A fast app-server can settle
        // the previous run in the window between the check and the transition,
        // and the refusal then surfaced as `codex_turn_start_failed: invalid run
        // transition: completed -> interrupted` — a new turn rejected because
        // the old one succeeded. So the settled case is absorbed on both sides:
        // once before doing the work, and once when the transition itself says
        // the run has already ended.
        if core
            .runs()
            .get(run_id.clone())
            .await?
            .and_then(|run| RunStatus::parse(&run.status).ok())
            .is_none_or(RunStatus::terminal)
        {
            return Ok(None);
        }
        let mutation = match core
            .runs()
            .transition(
                run_id.clone(),
                RunStatus::Interrupted,
                Some(serde_json::json!({ "reason": reason })),
                EventSource::Codex,
            )
            .await
        {
            Ok(mutation) => mutation,
            Err(error) => {
                let settled = core
                    .runs()
                    .get(run_id)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|run| RunStatus::parse(&run.status).ok())
                    .is_some_and(RunStatus::terminal);
                if settled {
                    // The run finished on its own. Nothing to interrupt, and the
                    // caller's turn is not at fault.
                    return Ok(None);
                }
                return Err(error);
            }
        };
        Ok(mutation.event)
    }

    /// The unresolved recovery notice for a session, if a previous turn was
    /// abandoned. Read before a new run is created so the replacement attempt
    /// can record what it is replacing.
    pub async fn pending_recovery(
        &self,
        session_id: &str,
    ) -> Option<crate::recovery::RecoveryNotice> {
        let Self::Core(core) = self else {
            return None;
        };
        let session_id = session_id.to_owned();
        core.storage()
            .database()
            .run(move |conn| {
                Ok(crate::recovery::pending_recovery_notices(conn)?.remove(&session_id))
            })
            .await
            .ok()
            .flatten()
    }

    /// Take the live ownership claim for a turn this process is running.
    /// Returns the recovery attempt this turn continues (0 for a first try).
    pub async fn claim_turn(
        &self,
        session_id: &str,
        run_id: &str,
        attachment_id: Option<String>,
    ) -> Result<i64> {
        let Self::Core(core) = self else {
            return Ok(0);
        };
        core.claim_turn(session_id.to_owned(), run_id.to_owned(), attachment_id)
            .await
    }

    /// Refresh the claim from live provider activity. Cheap to call on every
    /// event: the write is rate-limited against the stored heartbeat.
    pub async fn heartbeat_turn(&self, session_id: &str) {
        let Self::Core(core) = self else {
            return;
        };
        let _ = core.heartbeat_turn(session_id.to_owned()).await;
    }

    /// Give up the claim. A turn with no owner is not Working, so this must run
    /// on every terminal path — including the failure ones.
    pub async fn release_turn(&self, session_id: &str) {
        let Self::Core(core) = self else {
            return;
        };
        let _ = core.release_turn(session_id.to_owned()).await;
    }

    pub async fn record_usage(&self, record: UsageRecord) -> Result<()> {
        let Self::Core(core) = self else {
            return Ok(());
        };
        let repository = UsageRecordsRepository::new(core.storage().database().clone());
        repository.record(record).await
    }

    /// Persist one observed generation-speed measurement together with the raw
    /// samples it was derived from, so a displayed value stays recomputable.
    pub async fn record_generation_speed(
        &self,
        row: crate::storage::GenerationSpeedRow,
    ) -> Result<()> {
        let Self::Core(core) = self else {
            return Ok(());
        };
        let repository =
            crate::storage::GenerationSpeedRepository::new(core.storage().database().clone());
        repository.record(row).await
    }
}

fn ephemeral_codex_app_event(session_id: &str, method: &str, params: Value) -> AppEvent {
    ephemeral_app_event(session_id, EventSource::Codex, method, params)
}

fn ephemeral_app_event(
    session_id: &str,
    source: EventSource,
    method: &str,
    params: Value,
) -> AppEvent {
    AppEvent {
        schema_version: APP_EVENT_SCHEMA_VERSION.to_string(),
        sequence: 0,
        event_id: format!("evt_{}", Uuid::new_v4()),
        session_id: Some(session_id.to_owned()),
        session_sequence: None,
        run_id: None,
        source,
        kind: method.to_owned(),
        payload: params,
        remote_sequence: None,
        command_id: None,
        created_at: Utc::now().to_rfc3339(),
    }
}

#[cfg(test)]
mod interrupt_tests {
    use super::*;
    use crate::domain::{RunCreate, RunStatus, RuntimeTarget, SessionCreate, SessionKind, SessionStatus};
    use serde_json::json;
    use tempfile::tempdir;

    /// A new turn must not be refused because the previous one already finished.
    ///
    /// `send_turn` reconciles the session's active run before starting, and the
    /// run/session pointers settle independently. When the previous run reached
    /// `completed` first, the interrupt is not just impossible — it is not
    /// wanted, and surfacing its refusal aborted the caller's turn with
    /// `codex_turn_start_failed: invalid run transition: completed -> interrupted`.
    /// That was a live race, and the reason the codex suite failed roughly two
    /// runs in three under full-suite load.
    #[tokio::test]
    async fn interrupting_an_already_settled_run_is_a_no_op_not_a_failure() {
        let dir = tempdir().unwrap();
        let core = Arc::new(CoreRuntime::open(dir.path().join("core")).unwrap());
        let persistence = SessionPersistence::from_core(Some(core.clone()));

        core.sessions()
            .create_or_update(SessionCreate {
                id: "sess_settled".into(),
                title: "settled".into(),
                kind: SessionKind::Codex,
                target: RuntimeTarget::from_codex_provider("openrouter", "gpt-5.6-luna"),
                project_id: None,
                remote_id: None,
                codex_thread_id: Some("thread_settled".into()),
                status: SessionStatus::Ready,
                state_generation: None,
                metadata: json!({}),
                source: EventSource::Codex,
            })
            .await
            .unwrap();
        core.runs()
            .start(RunCreate {
                id: "run_settled".into(),
                session_id: "sess_settled".into(),
                mode: "codex_turn".into(),
                model: Some("gpt-5.6-luna".into()),
                adapter: None,
                metadata: json!({}),
                source: EventSource::Codex,
            })
            .await
            .unwrap();
        core.runs()
            .transition(
                "run_settled".into(),
                RunStatus::Completed,
                None,
                EventSource::Codex,
            )
            .await
            .unwrap();

        let event = persistence
            .interrupt_active_run("sess_settled", "desktop_reattached")
            .await
            .expect("a settled run must not fail the caller's next turn");
        assert!(event.is_none(), "nothing was interrupted, so nothing is announced");
        assert_eq!(
            core.runs()
                .get("run_settled".into())
                .await
                .unwrap()
                .map(|run| run.status),
            Some("completed".into()),
            "the run's own authoritative outcome is preserved"
        );
    }
}
