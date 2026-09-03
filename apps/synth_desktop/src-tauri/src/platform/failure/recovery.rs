//! Recovery plans and receipts. Failures never hold executable callbacks.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::definition::FailureId;
use crate::platform::approval::ApprovalRequirement;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RecoveryId(pub String);

impl RecoveryId {
    pub fn generate() -> Self {
        Self(format!("rec_{}", uuid::Uuid::new_v4().simple()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IdempotencyKey(pub String);

impl IdempotencyKey {
    pub fn for_restart(container_id: &str, failure_id: &str) -> Self {
        Self(format!("restart:{container_id}:{failure_id}"))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    RestartContainer {
        container_id: String,
        declaration_id: String,
    },
    ResumeSession {
        session_id: String,
    },
    ReconnectStream {
        visual_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecoveryBounds {
    pub max_attempts: u32,
    pub timeout_secs: u32,
}

impl Default for RecoveryBounds {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            timeout_secs: 120,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecoveryPlan {
    pub recovery_id: RecoveryId,
    pub failure_id: FailureId,
    pub action: RecoveryAction,
    pub approval_requirement: ApprovalRequirement,
    pub idempotency_key: IdempotencyKey,
    pub bounds: RecoveryBounds,
}

impl RecoveryPlan {
    pub fn restart_container(
        failure_id: FailureId,
        container_id: String,
        declaration_id: String,
    ) -> Self {
        let idempotency_key = IdempotencyKey::for_restart(&container_id, failure_id.as_str());
        Self {
            recovery_id: RecoveryId::generate(),
            failure_id,
            action: RecoveryAction::RestartContainer {
                container_id,
                declaration_id,
            },
            approval_requirement: ApprovalRequirement::OperatorClick {
                kind: "container_lifecycle".into(),
            },
            idempotency_key,
            bounds: RecoveryBounds::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecoveryReceipt {
    pub recovery_id: RecoveryId,
    pub failure_id: FailureId,
    pub status: String,
    pub approval_id: Option<String>,
    pub completed_at: DateTime<Utc>,
    pub detail: serde_json::Value,
}

pub fn insert_plan(conn: &rusqlite::Connection, plan: &RecoveryPlan) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO recovery_plans(
            recovery_id, failure_id, action_json, approval_requirement_json,
            idempotency_key, bounds_json, created_at
        ) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        rusqlite::params![
            plan.recovery_id.as_str(),
            plan.failure_id.as_str(),
            serde_json::to_string(&plan.action)?,
            serde_json::to_string(&plan.approval_requirement)?,
            plan.idempotency_key.as_str(),
            serde_json::to_string(&plan.bounds)?,
            Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(())
}

pub fn insert_receipt(
    conn: &rusqlite::Connection,
    receipt: &RecoveryReceipt,
) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO recovery_receipts(
            recovery_id, failure_id, status, approval_id, detail_json, completed_at
        ) VALUES (?1,?2,?3,?4,?5,?6)",
        rusqlite::params![
            receipt.recovery_id.as_str(),
            receipt.failure_id.as_str(),
            receipt.status,
            receipt.approval_id,
            receipt.detail.to_string(),
            receipt.completed_at.to_rfc3339(),
        ],
    )?;
    Ok(())
}

pub fn find_plan_by_idempotency(
    conn: &rusqlite::Connection,
    key: &str,
) -> anyhow::Result<Option<String>> {
    use rusqlite::OptionalExtension;
    Ok(conn
        .query_row(
            "SELECT recovery_id FROM recovery_plans WHERE idempotency_key = ?1",
            [key],
            |row| row.get(0),
        )
        .optional()?)
}
