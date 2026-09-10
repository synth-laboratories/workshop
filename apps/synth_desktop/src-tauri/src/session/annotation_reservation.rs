//! Single-use, host-signed reservations for paid Trace V5 annotation jobs.
//!
//! The approval broker grants a bounded charge; this module turns that grant
//! into one reservation per paid job, bound to `(trace_digest, annotator_id,
//! model, session_id)`, capped in USD micros, expiring, and single-use. The
//! reservation travels to the container as an HMAC-SHA256-signed token the
//! container verifies with the per-launch secret Workshop injected
//! (`SYNTH_ANNOTATION_BROKER_SECRET`); the host never trusts anything the
//! container or the agent says about money. Settlement flows back through
//! `paid_compute_budget` so the conversation ledger and its overspend trip stay
//! the single source of truth.
//!
//! Token bytes are byte-for-byte the format `synth_containers.tracing.annotation
//! .signed_broker` verifies: canonical JSON (sorted keys, compact separators,
//! nulls omitted) of the payload, HMAC-SHA256 under the secret, url-safe base64
//! without padding for the signature, url-safe base64 of the
//! `{"payload","signature"}` envelope for the token.

use std::collections::BTreeMap;

use anyhow::{anyhow, Context, Result};
use base64::Engine;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::paid_compute_budget::{self, SettlementOutcome};

pub(crate) const TOKEN_VERSION: &str = "synth.signed-reservation.v1";
pub(crate) const ENV_SECRET: &str = "SYNTH_ANNOTATION_BROKER_SECRET";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReservationBinding {
    pub trace_digest: String,
    pub annotator_id: String,
    pub model: String,
    pub session_id: String,
}

impl ReservationBinding {
    fn to_map(&self) -> BTreeMap<String, Value> {
        BTreeMap::from([
            ("annotator_id".to_string(), json!(self.annotator_id)),
            ("model".to_string(), json!(self.model)),
            ("session_id".to_string(), json!(self.session_id)),
            ("trace_digest".to_string(), json!(self.trace_digest)),
        ])
    }

    pub(crate) fn digest(&self) -> String {
        let bytes = serde_json::to_vec(&self.to_map()).unwrap_or_default();
        format!("sha256:{}", hex(&Sha256::digest(&bytes)))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct IssuedReservation {
    pub reservation_id: String,
    pub token: String,
    pub cap_usd_micros: u64,
    pub expires_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReservationRow {
    pub reservation_id: String,
    pub approval_id: String,
    pub session_id: String,
    pub container_id: String,
    pub binding_digest: String,
    pub reserved_usd_micros: u64,
    pub status: String,
    pub job_id: Option<String>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// HMAC-SHA256 without an extra crate: sha2 is already a dependency.
pub(crate) fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut key_block = [0u8; BLOCK];
    if key.len() > BLOCK {
        key_block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(key_block.iter().map(|b| b ^ 0x36).collect::<Vec<u8>>());
    inner.update(message);
    let inner_hash = inner.finalize();
    let mut outer = Sha256::new();
    outer.update(key_block.iter().map(|b| b ^ 0x5c).collect::<Vec<u8>>());
    outer.update(inner_hash);
    outer.finalize().into()
}

fn signature(secret: &[u8], payload: &BTreeMap<String, Value>) -> String {
    let canonical = serde_json::to_vec(payload).unwrap_or_default();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hmac_sha256(secret, &canonical))
}

/// Mint the token the container will verify. Pure; no storage.
pub(crate) fn sign_token(
    secret: &[u8],
    reservation_id: &str,
    issued_at: &str,
    cap_usd_micros: u64,
    binding: &ReservationBinding,
    approver: &str,
    expires_at: Option<&str>,
) -> String {
    let mut payload = BTreeMap::from([
        ("version".to_string(), json!(TOKEN_VERSION)),
        ("reservation_id".to_string(), json!(reservation_id)),
        ("issued_at".to_string(), json!(issued_at)),
        ("cap_usd_micros".to_string(), json!(cap_usd_micros)),
        (
            "binding".to_string(),
            Value::Object(binding.to_map().into_iter().collect()),
        ),
        ("approver".to_string(), json!(approver)),
        ("issuer".to_string(), json!("workshop")),
    ]);
    if let Some(expires) = expires_at {
        payload.insert("expires_at".to_string(), json!(expires));
    }
    let envelope = BTreeMap::from([
        (
            "payload".to_string(),
            Value::Object(payload.clone().into_iter().collect()),
        ),
        ("signature".to_string(), json!(signature(secret, &payload))),
    ]);
    base64::engine::general_purpose::URL_SAFE
        .encode(serde_json::to_vec(&envelope).unwrap_or_default())
}

pub(crate) fn store_broker_secret(
    conn: &Connection,
    container_id: &str,
    secret: &str,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO annotation_broker_secrets (container_id, secret, created_at) VALUES (?1, ?2, ?3)
         ON CONFLICT(container_id) DO UPDATE SET secret = excluded.secret, created_at = excluded.created_at",
        params![container_id, secret, now],
    )
    .context("store annotation broker secret")?;
    Ok(())
}

pub(crate) fn load_broker_secret(conn: &Connection, container_id: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT secret FROM annotation_broker_secrets WHERE container_id = ?1",
        params![container_id],
        |row| row.get(0),
    )
    .optional()
    .context("load annotation broker secret")
}

/// Issue one reservation against an already-granted approval. Reserves against
/// the conversation budget when one exists so the host cap is honoured too.
#[allow(clippy::too_many_arguments)]
pub(crate) fn issue(
    conn: &Connection,
    secret: &str,
    container_id: &str,
    session_id: &str,
    approval_id: &str,
    binding: &ReservationBinding,
    cap_usd_micros: u64,
    approver: &str,
    ttl_seconds: i64,
) -> Result<IssuedReservation> {
    anyhow::ensure!(cap_usd_micros > 0, "reservation cap must be positive");
    anyhow::ensure!(
        !binding.trace_digest.trim().is_empty(),
        "reservation trace digest must be present"
    );
    anyhow::ensure!(
        !binding.annotator_id.trim().is_empty(),
        "reservation annotator id must be present"
    );
    anyhow::ensure!(
        !binding.model.trim().is_empty(),
        "reservation model must be present"
    );
    anyhow::ensure!(
        !binding.session_id.trim().is_empty(),
        "reservation session must be present"
    );
    let reservation_id = format!("rsv_{}", uuid::Uuid::new_v4().simple());
    let now = Utc::now();
    let issued_at = now.to_rfc3339();
    let expires_at = (now + chrono::Duration::seconds(ttl_seconds)).to_rfc3339();
    let token = sign_token(
        secret.as_bytes(),
        &reservation_id,
        &issued_at,
        cap_usd_micros,
        binding,
        approver,
        Some(&expires_at),
    );
    conn.execute(
        "INSERT INTO annotation_reservations (reservation_id, approval_id, session_id, container_id, binding_digest, trace_digest, annotator_id, reserved_usd_micros, status, created_at, expires_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'issued', ?9, ?10, ?9)",
        params![
            reservation_id,
            approval_id,
            session_id,
            container_id,
            binding.digest(),
            binding.trace_digest,
            binding.annotator_id,
            cap_usd_micros as i64,
            issued_at,
            expires_at,
        ],
    )
    .context("insert annotation reservation")?;
    Ok(IssuedReservation {
        reservation_id,
        token,
        cap_usd_micros,
        expires_at,
    })
}

/// The container accepted the job: record the job id so settlement can find it.
pub(crate) fn mark_forwarded(conn: &Connection, reservation_id: &str, job_id: &str) -> Result<()> {
    let changed = conn.execute(
        "UPDATE annotation_reservations SET status = 'forwarded', job_id = ?2, updated_at = ?3 WHERE reservation_id = ?1 AND status = 'issued'",
        params![reservation_id, job_id, Utc::now().to_rfc3339()],
    )?;
    anyhow::ensure!(
        changed == 1,
        "reservation {reservation_id} was not in 'issued' state"
    );
    Ok(())
}

/// The container refused or never received the job: give the money back.
pub(crate) fn release(conn: &Connection, reservation_id: &str) -> Result<()> {
    let row = load(conn, reservation_id)?
        .ok_or_else(|| anyhow!("unknown reservation {reservation_id}"))?;
    let changed = conn.execute(
        "UPDATE annotation_reservations SET status = 'released', updated_at = ?2 WHERE reservation_id = ?1 AND status IN ('issued', 'forwarded')",
        params![reservation_id, Utc::now().to_rfc3339()],
    )?;
    if changed == 1 {
        finalize_approval_if_complete(conn, &row.approval_id, &row.session_id)?;
    }
    Ok(())
}

/// Terminal job seen: settle exactly when the container reported a billed cost,
/// otherwise keep the full reservation (never invent a zero).
pub(crate) fn settle(
    conn: &Connection,
    reservation_id: &str,
    outcome: SettlementOutcome,
) -> Result<ReservationRow> {
    let row = load(conn, reservation_id)?
        .ok_or_else(|| anyhow!("unknown reservation {reservation_id}"))?;
    if row.status == "settled" || row.status == "released" {
        return Ok(row);
    }
    let settled = match outcome {
        SettlementOutcome::Exact { cost_usd_micros } => Some(cost_usd_micros as i64),
        SettlementOutcome::Unknown => None,
    };
    conn.execute(
        "UPDATE annotation_reservations SET status = 'settled', settled_usd_micros = ?2, updated_at = ?3 WHERE reservation_id = ?1",
        params![reservation_id, settled, Utc::now().to_rfc3339()],
    )?;
    finalize_approval_if_complete(conn, &row.approval_id, &row.session_id)?;
    load(conn, reservation_id)?.ok_or_else(|| anyhow!("reservation {reservation_id} vanished"))
}

/// A campaign has one human approval and one paid-compute parent reservation,
/// but one signed child reservation per paid job.  The parent must not be
/// released or settled until every child is terminal.  Exact child costs are
/// summed; one unknown child retains the full parent reservation.
fn finalize_approval_if_complete(
    conn: &Connection,
    approval_id: &str,
    session_id: &str,
) -> Result<()> {
    let (active, unknown, exact): (i64, i64, i64) = conn.query_row(
        "SELECT
            COALESCE(SUM(CASE WHEN status IN ('issued', 'forwarded') THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN status = 'settled' AND settled_usd_micros IS NULL THEN 1 ELSE 0 END), 0),
            COALESCE(SUM(CASE WHEN status = 'settled' THEN COALESCE(settled_usd_micros, 0) ELSE 0 END), 0)
         FROM annotation_reservations WHERE approval_id = ?1",
        params![approval_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;
    if active != 0 {
        return Ok(());
    }
    let outcome = if unknown > 0 {
        SettlementOutcome::Unknown
    } else {
        SettlementOutcome::Exact {
            cost_usd_micros: u64::try_from(exact)
                .map_err(|_| anyhow!("negative annotation settlement"))?,
        }
    };
    if let Some(snapshot) = paid_compute_budget::settle(conn, session_id, approval_id, outcome)? {
        paid_compute_budget::append_settlement_receipt(
            conn,
            session_id,
            approval_id,
            outcome,
            &snapshot,
        )?;
    }
    Ok(())
}

pub(crate) fn load(conn: &Connection, reservation_id: &str) -> Result<Option<ReservationRow>> {
    conn.query_row(
        "SELECT reservation_id, approval_id, session_id, container_id, binding_digest, reserved_usd_micros, status, job_id FROM annotation_reservations WHERE reservation_id = ?1",
        params![reservation_id],
        |row| {
            Ok(ReservationRow {
                reservation_id: row.get(0)?,
                approval_id: row.get(1)?,
                session_id: row.get(2)?,
                container_id: row.get(3)?,
                binding_digest: row.get(4)?,
                reserved_usd_micros: row.get::<_, i64>(5)?.max(0) as u64,
                status: row.get(6)?,
                job_id: row.get(7)?,
            })
        },
    )
    .optional()
    .context("load annotation reservation")
}

pub(crate) fn by_job(
    conn: &Connection,
    container_id: &str,
    job_id: &str,
) -> Result<Option<ReservationRow>> {
    let id: Option<String> = conn
        .query_row(
            "SELECT reservation_id FROM annotation_reservations WHERE container_id = ?1 AND job_id = ?2 AND status = 'forwarded'",
            params![container_id, job_id],
            |row| row.get(0),
        )
        .optional()?;
    match id {
        Some(id) => load(conn, &id),
        None => Ok(None),
    }
}

/// Reservations the host issued but never saw accepted, past their expiry.
pub(crate) fn expire_stale(conn: &Connection) -> Result<usize> {
    let now = Utc::now().to_rfc3339();
    let stale: Vec<String> = conn
        .prepare("SELECT reservation_id FROM annotation_reservations WHERE status = 'issued' AND expires_at < ?1")?
        .query_map(params![now], |row| row.get(0))?
        .collect::<std::result::Result<Vec<String>, _>>()?;
    for id in &stale {
        release(conn, id)?;
        conn.execute(
            "UPDATE annotation_reservations SET status = 'expired', updated_at = ?2 WHERE reservation_id = ?1",
            params![id, now],
        )?;
    }
    Ok(stale.len())
}

