//! The `turn_ownership` row: one live claim per session, held by one boot epoch.
//!
//! `runs` stays an immutable historical record. Liveness is a separate,
//! deliberately short-lived fact, so it gets its own row with its own lease
//! rather than more mutable columns on run history.

use super::{HEARTBEAT_INTERVAL, LEASE_DURATION};
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TurnClaim {
    pub session_id: String,
    pub run_id: String,
    pub owner_instance_id: String,
    pub owner_attachment_id: Option<String>,
    pub claimed_at: String,
    pub heartbeat_at: String,
    pub lease_expires_at: String,
    pub recovery_attempt: i64,
    pub last_checkpoint: Option<Value>,
}

impl TurnClaim {
    /// Live means both halves: the current process holds it, and it has been
    /// refreshed recently enough. Either half alone is exactly the stale state
    /// this module exists to refuse.
    pub fn is_live(&self, instance_id: &str, now: DateTime<Utc>) -> bool {
        self.owner_instance_id == instance_id && !self.lease_expired(now)
    }

    pub fn lease_expired(&self, now: DateTime<Utc>) -> bool {
        match DateTime::parse_from_rfc3339(&self.lease_expires_at) {
            // An unparseable lease is not evidence of liveness.
            Err(_) => true,
            Ok(expires) => now > expires.with_timezone(&Utc),
        }
    }
}

/// Take ownership of `run_id` for this boot epoch.
///
/// Overwrites any previous claim on the session: the caller has already proven
/// the old attachment is gone (a new turn cannot start while one is running),
/// and leaving a dead claim behind would keep the session out of reconciliation.
pub fn claim(
    conn: &Connection,
    session_id: &str,
    run_id: &str,
    instance_id: &str,
    attachment_id: Option<&str>,
    recovery_attempt: i64,
    now: DateTime<Utc>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO turn_ownership(
            session_id, run_id, owner_instance_id, owner_attachment_id,
            claimed_at, heartbeat_at, lease_expires_at, recovery_attempt, last_checkpoint_json
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7, NULL)
         ON CONFLICT(session_id) DO UPDATE SET
            run_id = excluded.run_id,
            owner_instance_id = excluded.owner_instance_id,
            owner_attachment_id = excluded.owner_attachment_id,
            claimed_at = excluded.claimed_at,
            heartbeat_at = excluded.heartbeat_at,
            lease_expires_at = excluded.lease_expires_at,
            recovery_attempt = excluded.recovery_attempt,
            last_checkpoint_json = NULL",
        params![
            session_id,
            run_id,
            instance_id,
            attachment_id,
            now.to_rfc3339(),
            (now + LEASE_DURATION).to_rfc3339(),
            recovery_attempt,
        ],
    )?;
    Ok(())
}

/// Extend the lease. Only the current owner may; a foreign claim is left alone
/// so a second process cannot keep a turn it does not run looking alive.
///
/// Returns whether a row was refreshed.
pub fn heartbeat(
    conn: &Connection,
    session_id: &str,
    instance_id: &str,
    checkpoint: Option<&Value>,
    now: DateTime<Utc>,
) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE turn_ownership
         SET heartbeat_at = ?1,
             lease_expires_at = ?2,
             last_checkpoint_json = COALESCE(?3, last_checkpoint_json)
         WHERE session_id = ?4 AND owner_instance_id = ?5",
        params![
            now.to_rfc3339(),
            (now + LEASE_DURATION).to_rfc3339(),
            checkpoint.map(Value::to_string),
            session_id,
            instance_id,
        ],
    )?;
    Ok(changed > 0)
}

/// Whether enough time has passed that a heartbeat write is worth doing.
/// Provider events arrive far faster than the heartbeat interval; writing on
/// every one would turn the event pump into a database load generator.
pub fn heartbeat_due(claim: &TurnClaim, now: DateTime<Utc>) -> bool {
    match DateTime::parse_from_rfc3339(&claim.heartbeat_at) {
        Err(_) => true,
        Ok(last) => now - last.with_timezone(&Utc) >= HEARTBEAT_INTERVAL,
    }
}

pub fn release(conn: &Connection, session_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM turn_ownership WHERE session_id = ?1",
        params![session_id],
    )?;
    Ok(())
}

pub fn load(conn: &Connection, session_id: &str) -> Result<Option<TurnClaim>> {
    let claim = conn
        .query_row(
            "SELECT session_id, run_id, owner_instance_id, owner_attachment_id,
                    claimed_at, heartbeat_at, lease_expires_at, recovery_attempt,
                    last_checkpoint_json
             FROM turn_ownership WHERE session_id = ?1",
            params![session_id],
            |row| {
                Ok(TurnClaim {
                    session_id: row.get(0)?,
                    run_id: row.get(1)?,
                    owner_instance_id: row.get(2)?,
                    owner_attachment_id: row.get(3)?,
                    claimed_at: row.get(4)?,
                    heartbeat_at: row.get(5)?,
                    lease_expires_at: row.get(6)?,
                    recovery_attempt: row.get(7)?,
                    last_checkpoint: row
                        .get::<_, Option<String>>(8)?
                        .and_then(|raw| serde_json::from_str(&raw).ok()),
                })
            },
        )
        .optional()?;
    Ok(claim)
}

/// Sessions this process claims to own. Used by the lease watchdog and by the
/// renderer-independent liveness projection.
pub fn owned_sessions(conn: &Connection, instance_id: &str) -> Result<Vec<TurnClaim>> {
    let mut stmt = conn.prepare(
        "SELECT session_id, run_id, owner_instance_id, owner_attachment_id,
                claimed_at, heartbeat_at, lease_expires_at, recovery_attempt,
                last_checkpoint_json
         FROM turn_ownership WHERE owner_instance_id = ?1",
    )?;
    let rows = stmt.query_map(params![instance_id], |row| {
        Ok(TurnClaim {
            session_id: row.get(0)?,
            run_id: row.get(1)?,
            owner_instance_id: row.get(2)?,
            owner_attachment_id: row.get(3)?,
            claimed_at: row.get(4)?,
            heartbeat_at: row.get(5)?,
            lease_expires_at: row.get(6)?,
            recovery_attempt: row.get(7)?,
            last_checkpoint: row
                .get::<_, Option<String>>(8)?
                .and_then(|raw| serde_json::from_str(&raw).ok()),
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}
