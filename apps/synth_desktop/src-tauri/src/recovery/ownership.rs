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
    /// Live means a worker is still advancing the turn.
    ///
    /// Two cases, because a second process used to treat a live peer as a
    /// dead owner and interrupt its turns:
    /// * we own it and the lease has not expired; or
    /// * someone else owns it and their heartbeat is still fresh.
    ///
    /// The second arm is why a concurrent boot must refuse to open the DB
    /// rather than run `reconcile_orphaned_turns` against a peer that is
    /// still writing.
    pub fn is_live(&self, instance_id: &str, now: DateTime<Utc>) -> bool {
        if self.owner_instance_id == instance_id {
            !self.lease_expired(now)
        } else {
            self.heartbeat_fresh(now)
        }
    }

    /// A peer is still here if it heartbeated inside the lease window.
    /// An unparseable timestamp is not evidence of liveness.
    pub fn heartbeat_fresh(&self, now: DateTime<Utc>) -> bool {
        match DateTime::parse_from_rfc3339(&self.heartbeat_at) {
            Err(_) => false,
            Ok(last) => now - last.with_timezone(&Utc) < LEASE_DURATION,
        }
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

/// Ownership kinds this module speaks. Turns stay on `turn_ownership`;
/// optimizer campaigns use `optimizer_run_ownership`. Both are `(kind, id)`
/// claims; `TurnClaim::is_live` is unchanged.
pub const KIND_TURN: &str = "turn";
pub const KIND_OPTIMIZER_RUN: &str = "optimizer_run";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptimizerRunClaim {
    pub run_id: String,
    pub owner_instance_id: String,
    pub boot_epoch: String,
    pub pid: Option<u32>,
    pub process_start_identity: Option<String>,
    pub heartbeat_at: String,
    pub lease_expires_at: String,
}

impl OptimizerRunClaim {
    /// Same two-halves rule as [`TurnClaim::is_live`]: this process holds it,
    /// and the lease has not expired. Do not change `TurnClaim::is_live`.
    pub fn is_live(&self, instance_id: &str, now: DateTime<Utc>) -> bool {
        self.owner_instance_id == instance_id && !self.lease_expired(now)
    }

    pub fn lease_expired(&self, now: DateTime<Utc>) -> bool {
        match DateTime::parse_from_rfc3339(&self.lease_expires_at) {
            Err(_) => true,
            Ok(expires) => now > expires.with_timezone(&Utc),
        }
    }
}

pub fn claim_optimizer_run(
    conn: &Connection,
    run_id: &str,
    instance_id: &str,
    boot_epoch: &str,
    pid: Option<u32>,
    process_start_identity: Option<&str>,
    now: DateTime<Utc>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO optimizer_run_ownership(
            run_id, owner_instance_id, boot_epoch, pid, process_start_identity,
            heartbeat_at, lease_expires_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(run_id) DO UPDATE SET
            owner_instance_id = excluded.owner_instance_id,
            boot_epoch = excluded.boot_epoch,
            pid = excluded.pid,
            process_start_identity = excluded.process_start_identity,
            heartbeat_at = excluded.heartbeat_at,
            lease_expires_at = excluded.lease_expires_at",
        params![
            run_id,
            instance_id,
            boot_epoch,
            pid.map(|pid| pid as i64),
            process_start_identity,
            now.to_rfc3339(),
            (now + LEASE_DURATION).to_rfc3339(),
        ],
    )?;
    Ok(())
}

pub fn heartbeat_optimizer_run(
    conn: &Connection,
    run_id: &str,
    instance_id: &str,
    now: DateTime<Utc>,
) -> Result<bool> {
    let changed = conn.execute(
        "UPDATE optimizer_run_ownership
         SET heartbeat_at = ?1,
             lease_expires_at = ?2
         WHERE run_id = ?3 AND owner_instance_id = ?4",
        params![
            now.to_rfc3339(),
            (now + LEASE_DURATION).to_rfc3339(),
            run_id,
            instance_id,
        ],
    )?;
    Ok(changed > 0)
}

pub fn release_optimizer_run(conn: &Connection, run_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM optimizer_run_ownership WHERE run_id = ?1",
        params![run_id],
    )?;
    Ok(())
}

pub fn load_optimizer_run(conn: &Connection, run_id: &str) -> Result<Option<OptimizerRunClaim>> {
    let claim = conn
        .query_row(
            "SELECT run_id, owner_instance_id, boot_epoch, pid, process_start_identity,
                    heartbeat_at, lease_expires_at
             FROM optimizer_run_ownership WHERE run_id = ?1",
            params![run_id],
            |row| {
                Ok(OptimizerRunClaim {
                    run_id: row.get(0)?,
                    owner_instance_id: row.get(1)?,
                    boot_epoch: row.get(2)?,
                    pid: row
                        .get::<_, Option<i64>>(3)?
                        .and_then(|pid| u32::try_from(pid).ok()),
                    process_start_identity: row.get(4)?,
                    heartbeat_at: row.get(5)?,
                    lease_expires_at: row.get(6)?,
                })
            },
        )
        .optional()?;
    Ok(claim)
}

pub fn optimizer_run_is_live(
    conn: &Connection,
    run_id: &str,
    instance_id: &str,
    now: DateTime<Utc>,
) -> Result<bool> {
    let Some(claim) = load_optimizer_run(conn, run_id)? else {
        return Ok(false);
    };
    Ok(claim.is_live(instance_id, now))
}

/// Dispatch liveness for a `(kind, id)` claim. Turn liveness still goes through
/// [`TurnClaim::is_live`] unchanged.
pub fn claim_is_live(
    conn: &Connection,
    kind: &str,
    id: &str,
    instance_id: &str,
    now: DateTime<Utc>,
) -> Result<bool> {
    match kind {
        KIND_TURN => {
            let Some(claim) = load(conn, id)? else {
                return Ok(false);
            };
            Ok(claim.is_live(instance_id, now))
        }
        KIND_OPTIMIZER_RUN => optimizer_run_is_live(conn, id, instance_id, now),
        _ => Ok(false),
    }
}
