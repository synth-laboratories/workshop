//! Durable receipts for actions whose effects leave this process.
//!
//! Replaying a crashed turn is safe only while nothing consequential escaped.
//! A rollout launch is not free and is not idempotent from the user's side, so
//! "did that already happen?" has to be answerable from storage after the
//! process that asked is gone. Without this, Restart is a coin flip between
//! losing work and paying for it twice.

use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Outstanding: the request left, its outcome is unknown.
pub const STATUS_STARTED: &str = "started";
/// The external object exists and its identity is recorded.
pub const STATUS_SETTLED: &str = "settled";
/// The action provably did not take effect.
pub const STATUS_FAILED: &str = "failed";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActionReceipt {
    pub tool_call_id: String,
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub idempotency_key: String,
    pub operation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_object_id: Option<String>,
    pub request_digest: String,
    pub status: String,
    pub started_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settled_at: Option<String>,
}

/// What recovery is allowed to do with a turn, given its receipts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settlement {
    /// No consequential action escaped, so replaying the prompt is safe.
    pub restartable: bool,
    /// An action's outcome is unknown; a human must reconcile before retrying.
    pub needs_attention: bool,
    /// The external object a completed action produced, so a client can
    /// reattach to it instead of creating a second one.
    pub external_object_id: Option<String>,
}

/// Record that an external action is about to leave the process.
///
/// Written *before* the request, so a crash mid-flight is indistinguishable
/// from a crash after — which is the honest state, and the one that makes
/// recovery refuse to guess.
pub fn begin(
    conn: &Connection,
    session_id: &str,
    run_id: Option<&str>,
    idempotency_key: &str,
    operation: &str,
    request: &Value,
) -> Result<ActionReceipt> {
    let digest = request_digest(request);
    let now = Utc::now().to_rfc3339();
    if let Some(existing) = by_idempotency_key(conn, idempotency_key)? {
        return Ok(existing);
    }
    let tool_call_id = format!("act_{}", uuid::Uuid::new_v4().simple());
    conn.execute(
        "INSERT INTO action_receipts(
            tool_call_id, session_id, run_id, idempotency_key, operation,
            external_object_id, request_digest, status, started_at, settled_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, ?8, NULL)",
        params![
            tool_call_id,
            session_id,
            run_id,
            idempotency_key,
            operation,
            digest,
            STATUS_STARTED,
            now,
        ],
    )?;
    Ok(ActionReceipt {
        tool_call_id,
        session_id: session_id.to_owned(),
        run_id: run_id.map(str::to_owned),
        idempotency_key: idempotency_key.to_owned(),
        operation: operation.to_owned(),
        external_object_id: None,
        request_digest: digest_of(request),
        status: STATUS_STARTED.into(),
        started_at: now,
        settled_at: None,
    })
}

/// Close a receipt with the identity of what it produced.
pub fn settle(
    conn: &Connection,
    tool_call_id: &str,
    external_object_id: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE action_receipts
         SET status = ?1, external_object_id = COALESCE(?2, external_object_id), settled_at = ?3
         WHERE tool_call_id = ?4",
        params![
            STATUS_SETTLED,
            external_object_id,
            Utc::now().to_rfc3339(),
            tool_call_id,
        ],
    )?;
    Ok(())
}

/// Close a receipt that provably had no effect. Only call this when the request
/// is known not to have reached the other side; an ambiguous failure must stay
/// `started`, because that is what it is.
pub fn fail(conn: &Connection, tool_call_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE action_receipts SET status = ?1, settled_at = ?2 WHERE tool_call_id = ?3",
        params![STATUS_FAILED, Utc::now().to_rfc3339(), tool_call_id],
    )?;
    Ok(())
}

pub fn by_idempotency_key(conn: &Connection, key: &str) -> Result<Option<ActionReceipt>> {
    let receipt = conn
        .query_row(
            "SELECT tool_call_id, session_id, run_id, idempotency_key, operation,
                    external_object_id, request_digest, status, started_at, settled_at
             FROM action_receipts WHERE idempotency_key = ?1",
            params![key],
            from_row,
        )
        .optional()?;
    Ok(receipt)
}

pub fn for_run(conn: &Connection, run_id: &str) -> Result<Vec<ActionReceipt>> {
    let mut stmt = conn.prepare(
        "SELECT tool_call_id, session_id, run_id, idempotency_key, operation,
                external_object_id, request_digest, status, started_at, settled_at
         FROM action_receipts WHERE run_id = ?1 ORDER BY started_at",
    )?;
    let rows = stmt.query_map(params![run_id], from_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Decide what recovery may offer for one abandoned turn.
///
/// Ordered by danger, not by recency: an unknown settlement outranks a known
/// one, because the failure it prevents (paying twice) is worse than the one it
/// causes (asking a human).
pub fn classify_settlement(
    conn: &Connection,
    session_id: &str,
    run_id: Option<&str>,
) -> Result<Settlement> {
    let receipts = match run_id {
        Some(run_id) => for_run(conn, run_id)?,
        // Without a run id there is nothing to scope receipts to. Session-wide
        // history would drag in settled work from earlier turns and refuse a
        // restart that is in fact safe.
        None => {
            let _ = session_id;
            Vec::new()
        }
    };
    if receipts.is_empty() {
        return Ok(Settlement {
            restartable: true,
            needs_attention: false,
            external_object_id: None,
        });
    }
    if let Some(unsettled) = receipts
        .iter()
        .find(|receipt| receipt.status == STATUS_STARTED)
    {
        return Ok(Settlement {
            restartable: false,
            needs_attention: true,
            external_object_id: unsettled.external_object_id.clone(),
        });
    }
    if let Some(settled) = receipts
        .iter()
        .rev()
        .find(|receipt| receipt.status == STATUS_SETTLED)
    {
        // The work exists. Reattach to it; do not launch a second one.
        return Ok(Settlement {
            restartable: false,
            needs_attention: false,
            external_object_id: settled.external_object_id.clone(),
        });
    }
    // Everything failed outright, so nothing escaped.
    Ok(Settlement {
        restartable: true,
        needs_attention: false,
        external_object_id: None,
    })
}

fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ActionReceipt> {
    Ok(ActionReceipt {
        tool_call_id: row.get(0)?,
        session_id: row.get(1)?,
        run_id: row.get(2)?,
        idempotency_key: row.get(3)?,
        operation: row.get(4)?,
        external_object_id: row.get(5)?,
        request_digest: row.get(6)?,
        status: row.get(7)?,
        started_at: row.get(8)?,
        settled_at: row.get(9)?,
    })
}

fn request_digest(request: &Value) -> String {
    digest_of(request)
}

/// A stable fingerprint of the request, so a replay can be recognized as the
/// same action rather than compared field by field at every call site.
fn digest_of(request: &Value) -> String {
    let mut hasher = Sha256::new();
    hasher.update(canonical_json(request).as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

/// Serde preserves map order, so serialize keys sorted before hashing.
fn canonical_json(value: &Value) -> String {
    match value {
        Value::Object(map) => {
            let mut keys = map.keys().collect::<Vec<_>>();
            keys.sort();
            let body = keys
                .into_iter()
                .map(|key| format!("{}:{}", key, canonical_json(&map[key])))
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{body}}}")
        }
        Value::Array(values) => {
            let body = values
                .iter()
                .map(canonical_json)
                .collect::<Vec<_>>()
                .join(",");
            format!("[{body}]")
        }
        other => other.to_string(),
    }
}
