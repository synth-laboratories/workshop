//! Append-only secret audit ledger. Forbidden fields: secret values,
//! authorization headers, capability bodies, prompts, and responses.

use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const AUDIT_SCHEMA: &str = "workshop.secret-audit.v1";

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SecretAuditEvent {
    pub schema: String,
    pub event_id: String,
    pub at: String,
    pub actor_kind: String,
    pub actor_id: String,
    pub action: String,
    pub secret_id: Option<String>,
    pub provider: Option<String>,
    pub operation: Option<String>,
    pub model: Option<String>,
    pub decision: String,
    pub capability_id: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub usage: Option<Value>,
    pub detail: Option<String>,
}

impl SecretAuditEvent {
    pub fn new(actor_kind: &str, actor_id: &str, action: &str, decision: &str) -> Self {
        Self {
            schema: AUDIT_SCHEMA.into(),
            event_id: format!("sae_{}", Uuid::new_v4().simple()),
            at: Utc::now().to_rfc3339(),
            actor_kind: actor_kind.into(),
            actor_id: actor_id.into(),
            action: action.into(),
            secret_id: None,
            provider: None,
            operation: None,
            model: None,
            decision: decision.into(),
            capability_id: None,
            usage: None,
            detail: None,
        }
    }
}

pub fn append(conn: &Connection, event: &SecretAuditEvent) -> Result<()> {
    conn.execute(
        "INSERT INTO secret_audit(
            event_id, at, actor_kind, actor_id, action, secret_id, provider,
            operation, model, decision, capability_id, usage_json, detail
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
        params![
            event.event_id,
            event.at,
            event.actor_kind,
            event.actor_id,
            event.action,
            event.secret_id,
            event.provider,
            event.operation,
            event.model,
            event.decision,
            event.capability_id,
            event
                .usage
                .as_ref()
                .map(|value| serde_json::to_string(value).ok())
                .flatten(),
            event.detail,
        ],
    )?;
    Ok(())
}

pub fn list(conn: &Connection, limit: i64) -> Result<Vec<SecretAuditEvent>> {
    let mut stmt = conn.prepare(
        "SELECT event_id, at, actor_kind, actor_id, action, secret_id, provider,
                operation, model, decision, capability_id, usage_json, detail
         FROM secret_audit ORDER BY at DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map([limit.max(1).min(500)], |row| {
        let usage_json: Option<String> = row.get(11)?;
        Ok(SecretAuditEvent {
            schema: AUDIT_SCHEMA.into(),
            event_id: row.get(0)?,
            at: row.get(1)?,
            actor_kind: row.get(2)?,
            actor_id: row.get(3)?,
            action: row.get(4)?,
            secret_id: row.get(5)?,
            provider: row.get(6)?,
            operation: row.get(7)?,
            model: row.get(8)?,
            decision: row.get(9)?,
            capability_id: row.get(10)?,
            usage: usage_json.and_then(|raw| serde_json::from_str(&raw).ok()),
            detail: row.get(12)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}
