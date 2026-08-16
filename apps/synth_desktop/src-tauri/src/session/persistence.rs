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
            .transition(session_id, status, source, metadata)
            .await
        {
            Ok(mutation) => Ok(Some(mutation)),
            // Missing row or illegal edge: callers already updated the cache.
            Err(_) => Ok(None),
        }
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
        let mutation = core
            .runs()
            .transition(
                run_id,
                RunStatus::Interrupted,
                Some(serde_json::json!({ "reason": reason })),
                EventSource::Codex,
            )
            .await?;
        Ok(mutation.event)
    }

    pub async fn record_usage(&self, record: UsageRecord) -> Result<()> {
        let Self::Core(core) = self else {
            return Ok(());
        };
        let repository = UsageRecordsRepository::new(core.storage().database().clone());
        repository.record(record).await
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
