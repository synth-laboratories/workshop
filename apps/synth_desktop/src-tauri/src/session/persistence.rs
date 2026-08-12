//! Session durable write surface for provider transports.
//!
//! Replaces `Option<Arc<CoreRuntime>>` + per-call persistence branches in Codex.
//! `Null` no-ops every method so call sites always go through one type.

use crate::core_runtime::CoreRuntime;
use crate::domain::{
    DomainMutation, RunCreate, RunService, RunStatus, SessionCreate, SessionService, SessionStatus,
    SessionTitleOrigin,
};
use crate::storage::{
    AppEvent, Database, EventAppend, EventSource, RunRecord, SessionRecord, UsageRecord,
    UsageRecordsRepository,
};
use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;
use tauri::AppHandle;

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
            core.sessions()
                .set_title(session_id, title, origin)
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
