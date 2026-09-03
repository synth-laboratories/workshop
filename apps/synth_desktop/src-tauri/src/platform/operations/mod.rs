//! Operation identity and context. Correlation is captured at the entry point.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OperationId(pub String);

impl OperationId {
    pub fn generate() -> Self {
        Self(format!("op_{}", uuid::Uuid::new_v4().simple()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationKind {
    ContainerProbe,
    ContainerRestart,
    ContainerRegister,
    ContainerPrepare,
    EvaluationAdmit,
    EvaluationExecute,
    SessionTurn,
    SessionRecover,
    VisualRender,
    Bootstrap,
    Query,
}

impl OperationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ContainerProbe => "container.probe",
            Self::ContainerRestart => "container.restart",
            Self::ContainerRegister => "container.register",
            Self::ContainerPrepare => "container.prepare",
            Self::EvaluationAdmit => "evaluation.admit",
            Self::EvaluationExecute => "evaluation.execute",
            Self::SessionTurn => "session.turn",
            Self::SessionRecover => "session.recover",
            Self::VisualRender => "visual.render",
            Self::Bootstrap => "runtime.bootstrap",
            Self::Query => "observability.query",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationPhase {
    Start,
    Probe,
    Admit,
    Approve,
    Execute,
    Settle,
    Recover,
    Shutdown,
}

impl OperationPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Probe => "probe",
            Self::Admit => "admit",
            Self::Approve => "approve",
            Self::Execute => "execute",
            Self::Settle => "settle",
            Self::Recover => "recover",
            Self::Shutdown => "shutdown",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OperationContext {
    pub instance_id: Option<String>,
    pub operation_id: Option<OperationId>,
    pub parent_operation_id: Option<OperationId>,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub container_id: Option<String>,
    pub evaluation_id: Option<String>,
    pub rollout_id: Option<String>,
    pub visual_id: Option<String>,
    pub approval_id: Option<String>,
    pub credential_capability: Option<String>,
}

impl OperationContext {
    pub fn bootstrap(instance_id: impl Into<String>) -> Self {
        Self {
            instance_id: Some(instance_id.into()),
            operation_id: Some(OperationId::generate()),
            ..Self::default()
        }
    }

    pub fn for_container(mut self, container_id: impl Into<String>) -> Self {
        self.container_id = Some(container_id.into());
        if self.operation_id.is_none() {
            self.operation_id = Some(OperationId::generate());
        }
        self
    }

    pub fn for_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        if self.operation_id.is_none() {
            self.operation_id = Some(OperationId::generate());
        }
        self
    }

    pub fn ensure_operation_id(&mut self) -> OperationId {
        if let Some(id) = &self.operation_id {
            return id.clone();
        }
        let id = OperationId::generate();
        self.operation_id = Some(id.clone());
        id
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OperationRecord {
    pub operation_id: OperationId,
    pub kind: OperationKind,
    pub phase: OperationPhase,
    pub context: OperationContext,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}

pub fn insert_operation(
    conn: &rusqlite::Connection,
    record: &OperationRecord,
) -> anyhow::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO operation_records(
            operation_id, kind, phase, parent_operation_id, session_id, turn_id,
            tool_call_id, container_id, evaluation_id, rollout_id, visual_id,
            approval_id, context_json, started_at, completed_at
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
        rusqlite::params![
            record.operation_id.as_str(),
            record.kind.as_str(),
            record.phase.as_str(),
            record
                .context
                .parent_operation_id
                .as_ref()
                .map(|id| id.as_str().to_owned()),
            record.context.session_id,
            record.context.turn_id,
            record.context.tool_call_id,
            record.context.container_id,
            record.context.evaluation_id,
            record.context.rollout_id,
            record.context.visual_id,
            record.context.approval_id,
            serde_json::to_string(&record.context)?,
            record.started_at.to_rfc3339(),
            record.completed_at.map(|t| t.to_rfc3339()),
        ],
    )?;
    Ok(())
}

pub(crate) struct OperationAuthority;

impl OperationAuthority {
    pub fn begin(
        conn: &rusqlite::Connection,
        kind: OperationKind,
        phase: OperationPhase,
        mut context: OperationContext,
        now: DateTime<Utc>,
    ) -> anyhow::Result<OperationRecord> {
        let operation_id = context.ensure_operation_id();
        let record = OperationRecord {
            operation_id,
            kind,
            phase,
            context,
            started_at: now,
            completed_at: None,
        };
        insert_operation(conn, &record)?;
        Ok(record)
    }
}
