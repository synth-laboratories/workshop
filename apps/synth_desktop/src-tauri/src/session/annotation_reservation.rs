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

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> ReservationBinding {
        ReservationBinding {
            trace_digest: format!("sha256:{}", "a".repeat(64)),
            annotator_id: "craftax.belief".into(),
            model: "gpt-5.6-luna".into(),
            session_id: "sess-1".into(),
        }
    }

    /// Vector produced by `synth_containers.tracing.annotation.signed_broker` with
    /// secret b"parity"; both sides must sign identical canonical bytes.
    #[test]
    fn signature_matches_the_python_verifier() {
        let mut payload = BTreeMap::from([
            ("version".to_string(), json!(TOKEN_VERSION)),
            ("reservation_id".to_string(), json!("rsv_parity0001")),
            (
                "issued_at".to_string(),
                json!("2026-08-31T00:00:00.000000Z"),
            ),
            ("cap_usd_micros".to_string(), json!(250000)),
            (
                "binding".to_string(),
                Value::Object(binding().to_map().into_iter().collect()),
            ),
            ("approver".to_string(), json!("josh")),
            ("issuer".to_string(), json!("workshop")),
        ]);
        let canonical = String::from_utf8(serde_json::to_vec(&payload).unwrap()).unwrap();
        assert_eq!(
            canonical,
            r#"{"approver":"josh","binding":{"annotator_id":"craftax.belief","model":"gpt-5.6-luna","session_id":"sess-1","trace_digest":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"cap_usd_micros":250000,"issued_at":"2026-08-31T00:00:00.000000Z","issuer":"workshop","reservation_id":"rsv_parity0001","version":"synth.signed-reservation.v1"}"#
        );
        assert_eq!(
            signature(b"parity", &payload),
            "dVYJMvofxxugpWgI6i9xwfPmoU19wWnyWXVQ2o9kgTI"
        );
        payload.insert("expires_at".to_string(), json!("2030-01-01T00:00:00Z"));
        assert_ne!(
            signature(b"parity", &payload),
            "dVYJMvofxxugpWgI6i9xwfPmoU19wWnyWXVQ2o9kgTI"
        );
    }

    #[test]
    fn token_round_trips_and_is_bound() {
        let token = sign_token(
            b"s",
            "rsv_x",
            "2026-08-31T00:00:00Z",
            100,
            &binding(),
            "",
            Some("2030-01-01T00:00:00Z"),
        );
        let decoded = base64::engine::general_purpose::URL_SAFE
            .decode(token)
            .unwrap();
        let envelope: Value = serde_json::from_slice(&decoded).unwrap();
        assert_eq!(envelope["payload"]["reservation_id"], "rsv_x");
        assert_eq!(envelope["payload"]["binding"]["session_id"], "sess-1");
        assert!(envelope["signature"].as_str().unwrap().len() > 30);
    }

    fn memory() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::storage::migrations::apply_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn issue_forward_settle_and_release_are_single_use() {
        let conn = memory();
        store_broker_secret(&conn, "ctr", "secret").unwrap();
        assert_eq!(
            load_broker_secret(&conn, "ctr").unwrap().as_deref(),
            Some("secret")
        );
        assert!(load_broker_secret(&conn, "other").unwrap().is_none());
        let issued = issue(
            &conn,
            "secret",
            "ctr",
            "sess-1",
            "approval-1",
            &binding(),
            250_000,
            "josh",
            600,
        )
        .unwrap();
        assert!(load(&conn, &issued.reservation_id).unwrap().unwrap().status == "issued");
        mark_forwarded(&conn, &issued.reservation_id, "job-1").unwrap();
        assert!(mark_forwarded(&conn, &issued.reservation_id, "job-2").is_err());
        assert_eq!(
            by_job(&conn, "ctr", "job-1")
                .unwrap()
                .unwrap()
                .reservation_id,
            issued.reservation_id
        );
        let settled = settle(
            &conn,
            &issued.reservation_id,
            SettlementOutcome::Exact {
                cost_usd_micros: 4000,
            },
        )
        .unwrap();
        assert_eq!(settled.status, "settled");
        // idempotent
        assert_eq!(
            settle(&conn, &issued.reservation_id, SettlementOutcome::Unknown)
                .unwrap()
                .status,
            "settled"
        );
        let second = issue(
            &conn,
            "secret",
            "ctr",
            "sess-1",
            "approval-2",
            &binding(),
            100,
            "josh",
            -1,
        )
        .unwrap();
        assert_eq!(expire_stale(&conn).unwrap(), 1);
        assert_eq!(
            load(&conn, &second.reservation_id).unwrap().unwrap().status,
            "expired"
        );
    }

    #[test]
    fn campaign_children_settle_the_parent_once_with_the_aggregate_cost() {
        let conn = memory();
        let policy = crate::synth_config::PaidComputeAutoApprovalPolicy {
            enabled: true,
            max_request_usd_micros: 500_000,
            max_conversation_usd_micros: 1_000_000,
            providers: vec!["openrouter".into()],
        };
        paid_compute_budget::seed_conversation_budget(&conn, "sess-1", &policy).unwrap();
        paid_compute_budget::try_reserve(
            &conn,
            "sess-1",
            "approval-campaign",
            Some("campaign"),
            300_000,
        )
        .unwrap()
        .unwrap();
        let first = issue(
            &conn,
            "secret",
            "ctr",
            "sess-1",
            "approval-campaign",
            &binding(),
            100_000,
            "workshop",
            600,
        )
        .unwrap();
        let mut second_binding = binding();
        second_binding.annotator_id = "craftax.plan".into();
        let second = issue(
            &conn,
            "secret",
            "ctr",
            "sess-1",
            "approval-campaign",
            &second_binding,
            200_000,
            "workshop",
            600,
        )
        .unwrap();
        mark_forwarded(&conn, &first.reservation_id, "job-1").unwrap();
        mark_forwarded(&conn, &second.reservation_id, "job-2").unwrap();

        settle(
            &conn,
            &first.reservation_id,
            SettlementOutcome::Exact {
                cost_usd_micros: 30_000,
            },
        )
        .unwrap();
        let midway = paid_compute_budget::snapshot(&conn, "sess-1")
            .unwrap()
            .unwrap();
        assert_eq!(
            midway.reserved_usd_micros, 300_000,
            "one child cannot release the campaign parent"
        );
        assert_eq!(midway.settled_spend_usd_micros, 0);

        settle(
            &conn,
            &second.reservation_id,
            SettlementOutcome::Exact {
                cost_usd_micros: 40_000,
            },
        )
        .unwrap();
        let terminal = paid_compute_budget::snapshot(&conn, "sess-1")
            .unwrap()
            .unwrap();
        assert_eq!(terminal.reserved_usd_micros, 0);
        assert_eq!(terminal.settled_spend_usd_micros, 70_000);
    }
}
