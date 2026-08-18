//! Crash recovery: proving who owns a running turn, and telling the truth when
//! nobody does.
//!
//! Two different facts used to be stored as one. "The last known status was
//! `running`" survives a crash; "a worker in this process can still advance that
//! turn" does not. Rendering the first as the second is what left five chats
//! spinning at Working after the app died holding them.
//!
//! The rule this module enforces:
//!
//! > A turn is live only while a row in `turn_ownership` names the current boot
//! > epoch and its lease has not expired.
//!
//! Everything else is history, and startup reconciliation rewrites it as such
//! **before** any client can read it — see [`reconcile_orphaned_turns`], which
//! runs inside `CoreRuntime::open`, not in a spawned task.

pub mod ownership;
pub mod receipts;

use crate::storage::{append_event, EventAppend, EventSource};
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// How often a live owner refreshes its claim.
pub const HEARTBEAT_INTERVAL: Duration = Duration::seconds(5);
/// How long a claim outlives its last heartbeat. Four missed heartbeats, so a
/// briefly blocked event pump does not interrupt a healthy XHigh turn.
pub const LEASE_DURATION: Duration = Duration::seconds(20);

/// Session metadata key holding the pending [`RecoveryNotice`]. One durable
/// home, read by both the SQLite session list and the Codex record cache.
pub const RECOVERY_METADATA_KEY: &str = "recovery";

pub const RECOVERY_EVENT_KIND: &str = "session/recovery_required";

/// Why a `running` row stopped being live.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryReason {
    /// The owning process is gone: a different boot epoch holds the claim, or
    /// no claim exists at all.
    WorkshopRestarted,
    /// This process still owns the claim but stopped refreshing it.
    LeaseExpired,
}

impl RecoveryReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WorkshopRestarted => "workshop_restarted",
            Self::LeaseExpired => "lease_expired",
        }
    }
}

/// The operator-facing prompt that produced the abandoned turn, so Restart can
/// reuse it instead of asking the user to retype what they already sent.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryPrompt {
    pub text: String,
    #[serde(default)]
    pub client_message_id: Option<String>,
}

/// The last thing this turn durably did before its owner disappeared.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryActivity {
    pub kind: String,
    #[serde(default)]
    pub label: Option<String>,
    pub at: String,
}

/// Everything the product needs to say what happened and what is safe next.
///
/// Persisted on the session row (`metadata.recovery`) and journalled as
/// [`RECOVERY_EVENT_KIND`], so a client that missed the event still sees it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryNotice {
    pub session_id: String,
    #[serde(default)]
    pub run_id: Option<String>,
    pub reason: String,
    #[serde(default)]
    pub previous_owner_instance_id: Option<String>,
    #[serde(default)]
    pub last_heartbeat_at: Option<String>,
    /// Which attempt a restart would be. `u32` rather than `i64`: this crosses
    /// the specta boundary, which forbids BigInt-style types, and a retry count
    /// has no business being one.
    pub recovery_attempt: u32,
    /// Whether replaying the prompt can be offered as a plain retry.
    pub restartable: bool,
    /// Whether an external action's outcome is unknown, so a retry could
    /// duplicate consequential work and a human must reconcile first.
    pub needs_attention: bool,
    #[serde(default)]
    pub external_object_id: Option<String>,
    #[serde(default)]
    pub last_activity: Option<RecoveryActivity>,
    #[serde(default)]
    pub last_user_message: Option<RecoveryPrompt>,
    pub recovered_at: String,
}

impl RecoveryNotice {
    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}

/// Rewrite every `running` row whose owner cannot be proven live.
///
/// One transaction covers run, session, ownership and journal so no reader can
/// observe a session and its active run disagreeing. Idempotent: a second pass
/// finds nothing, because the first one left no `running` rows without a live
/// claim.
///
/// `now` is a parameter so the lease watchdog and the tests drive the same code.
pub fn reconcile_orphaned_turns(
    conn: &Connection,
    instance_id: &str,
    now: DateTime<Utc>,
) -> Result<Vec<RecoveryNotice>> {
    let mut notices = Vec::new();
    for session_id in orphan_candidate_sessions(conn)? {
        if let Some(notice) = reconcile_session(conn, &session_id, instance_id, now)? {
            notices.push(notice);
        }
    }
    // A run can outlive the session status that pointed at it (an interrupted
    // session whose run row never settled). Close those too, or `list_runs`
    // keeps reporting work that nothing is doing.
    for (run_id, session_id) in orphan_running_runs(conn)? {
        if is_live_owner(conn, &session_id, instance_id, now)? {
            continue;
        }
        interrupt_run(conn, &run_id, RecoveryReason::WorkshopRestarted, now)?;
    }
    Ok(notices)
}

/// Whether anything at all could need reconciling.
///
/// Read-only, so the five-second lease sweep does not take a write lock on an
/// idle database every tick. It deliberately over-reports — deciding whether a
/// candidate is actually stale stays in [`reconcile_orphaned_turns`], which owns
/// the liveness rule.
pub fn has_reconcilable_turns(conn: &Connection) -> Result<bool> {
    let candidates: i64 = conn.query_row(
        "SELECT (SELECT COUNT(*) FROM sessions WHERE status = 'running' OR active_run_id IS NOT NULL)
              + (SELECT COUNT(*) FROM runs WHERE status = 'running')",
        [],
        |row| row.get(0),
    )?;
    Ok(candidates > 0)
}

/// Sessions whose durable state claims a turn is in progress.
fn orphan_candidate_sessions(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM sessions
         WHERE status = 'running' OR active_run_id IS NOT NULL
         ORDER BY updated_at DESC",
    )?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn orphan_running_runs(conn: &Connection) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare("SELECT id, session_id FROM runs WHERE status = 'running'")?;
    let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// The single liveness predicate. Nothing else may decide that a turn is live.
pub fn is_live_owner(
    conn: &Connection,
    session_id: &str,
    instance_id: &str,
    now: DateTime<Utc>,
) -> Result<bool> {
    let Some(claim) = ownership::load(conn, session_id)? else {
        return Ok(false);
    };
    Ok(claim.is_live(instance_id, now))
}

fn reconcile_session(
    conn: &Connection,
    session_id: &str,
    instance_id: &str,
    now: DateTime<Utc>,
) -> Result<Option<RecoveryNotice>> {
    let Some((status, active_run_id, metadata_raw)) = conn
        .query_row(
            "SELECT status, active_run_id, metadata_json FROM sessions WHERE id = ?1",
            params![session_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?
    else {
        return Ok(None);
    };
    if status != "running" && active_run_id.is_none() {
        return Ok(None);
    }
    let claim = ownership::load(conn, session_id)?;
    if let Some(claim) = &claim {
        if claim.is_live(instance_id, now) {
            return Ok(None);
        }
    }
    let reason = match &claim {
        Some(claim) if claim.owner_instance_id == instance_id => RecoveryReason::LeaseExpired,
        _ => RecoveryReason::WorkshopRestarted,
    };
    let run_id = active_run_id
        .clone()
        .or_else(|| claim.as_ref().map(|claim| claim.run_id.clone()));
    let settlement = receipts::classify_settlement(conn, session_id, run_id.as_deref())?;
    let notice = RecoveryNotice {
        session_id: session_id.to_owned(),
        run_id: run_id.clone(),
        reason: reason.as_str().to_owned(),
        previous_owner_instance_id: claim.as_ref().map(|claim| claim.owner_instance_id.clone()),
        last_heartbeat_at: claim.as_ref().map(|claim| claim.heartbeat_at.clone()),
        recovery_attempt: claim
            .as_ref()
            .map_or(0, |claim| claim.recovery_attempt.max(0) as u32)
            + 1,
        restartable: settlement.restartable,
        needs_attention: settlement.needs_attention,
        external_object_id: settlement.external_object_id,
        last_activity: last_durable_activity(conn, session_id)?,
        last_user_message: last_user_message(conn, session_id)?,
        recovered_at: now.to_rfc3339(),
    };

    if let Some(run_id) = &run_id {
        interrupt_run(conn, run_id, reason, now)?;
    }
    let metadata = with_recovery_metadata(&metadata_raw, &notice);
    conn.execute(
        "UPDATE sessions
         SET status = 'interrupted', active_run_id = NULL, metadata_json = ?1, updated_at = ?2
         WHERE id = ?3",
        params![metadata.to_string(), now.to_rfc3339(), session_id],
    )?;
    ownership::release(conn, session_id)?;
    append_event(
        conn,
        EventAppend {
            event_id: None,
            session_id: Some(session_id.to_owned()),
            run_id: run_id.clone(),
            source: EventSource::System,
            kind: RECOVERY_EVENT_KIND.into(),
            payload: notice.to_json(),
            remote_sequence: None,
            command_id: None,
            created_at: Some(now.to_rfc3339()),
        },
    )
    .context("journal session recovery")?;
    Ok(Some(notice))
}

fn interrupt_run(
    conn: &Connection,
    run_id: &str,
    reason: RecoveryReason,
    now: DateTime<Utc>,
) -> Result<()> {
    conn.execute(
        "UPDATE runs
         SET status = 'interrupted',
             outcome_json = COALESCE(outcome_json, ?1),
             completed_at = COALESCE(completed_at, ?2),
             updated_at = ?2
         WHERE id = ?3 AND status IN ('created', 'running')",
        params![
            json!({ "reason": reason.as_str() }).to_string(),
            now.to_rfc3339(),
            run_id,
        ],
    )?;
    Ok(())
}

fn with_recovery_metadata(raw: &str, notice: &RecoveryNotice) -> Value {
    let mut metadata = serde_json::from_str::<Value>(raw).unwrap_or(Value::Null);
    if !metadata.is_object() {
        metadata = json!({});
    }
    if let Some(object) = metadata.as_object_mut() {
        object.insert(RECOVERY_METADATA_KEY.into(), notice.to_json());
    }
    metadata
}

/// Drop the pending notice once the session has moved on. Called when a new
/// turn is claimed, so a recovered chat does not keep offering to restart a
/// turn it has already replaced.
pub fn clear_recovery_metadata(conn: &Connection, session_id: &str) -> Result<Option<i64>> {
    let Some(raw) = conn
        .query_row(
            "SELECT metadata_json FROM sessions WHERE id = ?1",
            params![session_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
    else {
        return Ok(None);
    };
    let mut metadata = serde_json::from_str::<Value>(&raw).unwrap_or(Value::Null);
    let Some(object) = metadata.as_object_mut() else {
        return Ok(None);
    };
    let Some(previous) = object.remove(RECOVERY_METADATA_KEY) else {
        return Ok(None);
    };
    conn.execute(
        "UPDATE sessions SET metadata_json = ?1 WHERE id = ?2",
        params![metadata.to_string(), session_id],
    )?;
    Ok(Some(
        previous
            .get("recoveryAttempt")
            .and_then(Value::as_i64)
            .unwrap_or(0),
    ))
}

/// Every session still carrying an unresolved notice, keyed by session id.
pub fn pending_recovery_notices(
    conn: &Connection,
) -> Result<std::collections::HashMap<String, RecoveryNotice>> {
    let mut stmt = conn.prepare(
        "SELECT id, json_extract(metadata_json, '$.recovery') FROM sessions
         WHERE json_extract(metadata_json, '$.recovery') IS NOT NULL",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut notices = std::collections::HashMap::new();
    for row in rows {
        let (session_id, raw) = row?;
        if let Ok(notice) = serde_json::from_str::<RecoveryNotice>(&raw) {
            notices.insert(session_id, notice);
        }
    }
    Ok(notices)
}

fn last_durable_activity(conn: &Connection, session_id: &str) -> Result<Option<RecoveryActivity>> {
    let row = conn
        .query_row(
            "SELECT kind, payload_json, created_at FROM events
             WHERE session_id = ?1 AND kind NOT IN ('app-server/stderr', 'diagnostic.event')
             ORDER BY sequence DESC LIMIT 1",
            params![session_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?;
    Ok(row.map(|(kind, payload, at)| RecoveryActivity {
        label: activity_label(&serde_json::from_str(&payload).unwrap_or(Value::Null)),
        kind,
        at,
    }))
}

/// Best-effort human handle for the last event. A tool name is the detail an
/// operator can actually act on; anything else is left to the client.
fn activity_label(payload: &Value) -> Option<String> {
    for pointer in ["/item/name", "/item/toolName", "/tool", "/name", "/method"] {
        if let Some(value) = payload
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(value.to_owned());
        }
    }
    None
}

fn last_user_message(conn: &Connection, session_id: &str) -> Result<Option<RecoveryPrompt>> {
    let row = conn
        .query_row(
            "SELECT payload_json FROM events
             WHERE session_id = ?1 AND kind = 'message.created'
               AND json_extract(payload_json, '$.role') = 'user'
             ORDER BY sequence DESC LIMIT 1",
            params![session_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(raw) = row else { return Ok(None) };
    let payload = serde_json::from_str::<Value>(&raw).unwrap_or(Value::Null);
    let text = payload
        .get("content")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|text| !text.trim().is_empty());
    Ok(text.map(|text| RecoveryPrompt {
        text,
        client_message_id: payload
            .get("messageId")
            .and_then(Value::as_str)
            .map(str::to_owned),
    }))
}

/// Test-only crash points, so recovery is exercised deterministically instead
/// of waiting for an incidental crash.
///
/// `SYNTH_DESKTOP_CRASH_AT=<checkpoint>` aborts the process the moment the
/// named checkpoint is reached. Abort, not exit: a graceful shutdown would run
/// the drain path and hide exactly the failure under test.
pub fn crash_checkpoint(name: &str) {
    let Ok(requested) = std::env::var("SYNTH_DESKTOP_CRASH_AT") else {
        return;
    };
    if requested.split(',').any(|value| value.trim() == name) {
        eprintln!("synth-desktop: fault injection abort at checkpoint {name}");
        std::process::abort();
    }
}

pub mod checkpoints {
    pub const AFTER_TURN_START: &str = "after_turn_start";
    pub const AFTER_FIRST_ACTIVITY: &str = "after_first_activity";
    pub const BEFORE_TOOL_DISPATCH: &str = "before_tool_dispatch";
    pub const AFTER_TOOL_DISPATCH: &str = "after_tool_dispatch";
    pub const AFTER_TOOL_RECEIPT: &str = "after_tool_receipt";
    pub const AFTER_ROLLOUT_LAUNCH: &str = "after_rollout_launch";
    pub const AFTER_ROLLOUT_TERMINAL: &str = "after_rollout_terminal";
    pub const BEFORE_FINAL_MESSAGE: &str = "before_final_message";
}

#[cfg(test)]
mod tests;
