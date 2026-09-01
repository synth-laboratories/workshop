//! Conversation-scoped paid-compute authorization projection.
//!
//! Approval receipts remain the audit record. This table is the live balance
//! used to decide whether a new hard ceiling can be reserved without opening
//! the modal. Totals are never derived from optimizer summaries.

use crate::storage::{append_event, EventAppend, EventSource};
use crate::synth_config::{format_usd_micros, PaidComputeAutoApprovalPolicy};
use anyhow::{anyhow, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

pub(crate) const CONVERSATION_POLICY: &str = "conversation_paid_compute_budget";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReservationGrant {
    pub reserved_usd_micros: u64,
    pub conversation_cap_usd_micros: u64,
    pub settled_spend_usd_micros: u64,
    pub remaining_usd_micros: u64,
}

impl ReservationGrant {
    pub(crate) fn receipt_fields(&self) -> Value {
        json!({
            "policyAuto": true,
            "approvalPolicy": CONVERSATION_POLICY,
            "conversationCapUsdMicros": self.conversation_cap_usd_micros,
            "reservedUsdMicros": self.reserved_usd_micros,
            "settledSpendUsdMicros": self.settled_spend_usd_micros,
            "remainingUsdMicros": self.remaining_usd_micros,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ConversationSnapshot {
    pub conversation_cap_usd_micros: u64,
    pub settled_spend_usd_micros: u64,
    pub reserved_usd_micros: u64,
    pub remaining_usd_micros: u64,
    pub auto_disabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettlementOutcome {
    /// Authoritative exact cost, including a proven zero.
    Exact { cost_usd_micros: u64 },
    /// Telemetry is incomplete; keep the full reservation.
    Unknown,
}

/// Freeze the session's sealed allowance. Existing rows are left unchanged so
/// a later config edit cannot drift an active conversation.
pub(crate) fn seed_conversation_budget(
    conn: &Connection,
    session_id: &str,
    policy: &PaidComputeAutoApprovalPolicy,
) -> Result<()> {
    if !policy.enabled {
        return Ok(());
    }
    let now = Utc::now().to_rfc3339();
    let providers = serde_json::to_string(&policy.providers)?;
    conn.execute(
        "INSERT OR IGNORE INTO paid_compute_conversation_budgets(
            session_id, conversation_cap_usd_micros, max_request_usd_micros,
            providers_json, settled_spend_usd_micros, auto_disabled,
            created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, 0, 0, ?5, ?5)",
        params![
            session_id,
            policy.max_conversation_usd_micros as i64,
            policy.max_request_usd_micros as i64,
            providers,
            now,
        ],
    )?;
    Ok(())
}

pub(crate) fn try_reserve(
    conn: &Connection,
    session_id: &str,
    approval_id: &str,
    preparation_digest: Option<&str>,
    requested_usd_micros: u64,
) -> Result<Option<ReservationGrant>> {
    let Some(budget) = load_budget(conn, session_id)? else {
        return Ok(None);
    };
    if budget.auto_disabled {
        return Ok(None);
    }
    if requested_usd_micros == 0 || requested_usd_micros > budget.max_request_usd_micros {
        return Ok(None);
    }
    let reserved = active_reservations(conn, session_id)?;
    let replacing = match preparation_digest.filter(|digest| !digest.is_empty()) {
        Some(digest) => reserved_micros_for_digest(conn, session_id, digest)?,
        None => 0,
    };
    let used = budget
        .settled_spend_usd_micros
        .saturating_add(reserved.saturating_sub(replacing));
    let Some(remaining_before) = budget.conversation_cap_usd_micros.checked_sub(used) else {
        return Ok(None);
    };
    if requested_usd_micros > remaining_before {
        return Ok(None);
    }
    if let Some(digest) = preparation_digest.filter(|digest| !digest.is_empty()) {
        // A retried start on the same preparation must not stack unused
        // ceilings. Drop the prior reserved row only after this request is
        // known to fit, so a rejected retry cannot wipe an outstanding hold.
        conn.execute(
            "DELETE FROM paid_compute_reservations
             WHERE session_id=?1 AND preparation_digest=?2 AND status='reserved'",
            params![session_id, digest],
        )?;
    }
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO paid_compute_reservations(
            approval_id, session_id, reserved_usd_micros, preparation_digest,
            status, settled_usd_micros, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, 'reserved', NULL, ?5, ?5)",
        params![
            approval_id,
            session_id,
            requested_usd_micros as i64,
            preparation_digest,
            now,
        ],
    )?;
    let remaining = remaining_before - requested_usd_micros;
    Ok(Some(ReservationGrant {
        reserved_usd_micros: requested_usd_micros,
        conversation_cap_usd_micros: budget.conversation_cap_usd_micros,
        settled_spend_usd_micros: budget.settled_spend_usd_micros,
        remaining_usd_micros: remaining,
    }))
}

pub(crate) fn release_reservation(conn: &Connection, approval_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM paid_compute_reservations WHERE approval_id=?1 AND status='reserved'",
        params![approval_id],
    )?;
    Ok(())
}

pub(crate) fn settle(
    conn: &Connection,
    session_id: &str,
    approval_id: &str,
    outcome: SettlementOutcome,
) -> Result<Option<ConversationSnapshot>> {
    let Some(reservation) = load_reservation(conn, approval_id)? else {
        return Ok(None);
    };
    if reservation.session_id != session_id {
        return Err(anyhow!(
            "paid-compute reservation {approval_id} does not belong to session {session_id}"
        ));
    }
    if reservation.status != "reserved" {
        return snapshot(conn, session_id);
    }
    match outcome {
        SettlementOutcome::Unknown => snapshot(conn, session_id),
        SettlementOutcome::Exact { cost_usd_micros } => {
            apply_exact_settlement(conn, &reservation, cost_usd_micros)?;
            snapshot(conn, session_id)
        }
    }
}

pub(crate) fn snapshot(
    conn: &Connection,
    session_id: &str,
) -> Result<Option<ConversationSnapshot>> {
    let Some(budget) = load_budget(conn, session_id)? else {
        return Ok(None);
    };
    let reserved = active_reservations(conn, session_id)?;
    let used = budget.settled_spend_usd_micros.saturating_add(reserved);
    let remaining = budget.conversation_cap_usd_micros.saturating_sub(used);
    Ok(Some(ConversationSnapshot {
        conversation_cap_usd_micros: budget.conversation_cap_usd_micros,
        settled_spend_usd_micros: budget.settled_spend_usd_micros,
        reserved_usd_micros: reserved,
        remaining_usd_micros: remaining,
        auto_disabled: budget.auto_disabled,
    }))
}

pub(crate) fn budget_allows_provider(
    conn: &Connection,
    session_id: &str,
    provider: &str,
) -> Result<bool> {
    let Some(budget) = load_budget(conn, session_id)? else {
        return Ok(false);
    };
    if budget.auto_disabled {
        return Ok(false);
    }
    Ok(budget.providers.iter().any(|allowed| allowed == provider))
}

pub(crate) fn append_settlement_receipt(
    conn: &Connection,
    session_id: &str,
    approval_id: &str,
    outcome: SettlementOutcome,
    snapshot: &ConversationSnapshot,
) -> Result<()> {
    let (settled, retained) = match outcome {
        SettlementOutcome::Exact { cost_usd_micros } => (Some(cost_usd_micros), false),
        SettlementOutcome::Unknown => (None, true),
    };
    append_event(
        conn,
        EventAppend {
            event_id: None,
            session_id: Some(session_id.to_owned()),
            run_id: None,
            source: EventSource::Codex,
            kind: "approval.paid_compute.settled".into(),
            payload: json!({
                "approvalId": approval_id,
                "kind": "paid_compute",
                "approvalPolicy": CONVERSATION_POLICY,
                "settledUsdMicros": settled,
                "reservationRetained": retained,
                "receiptViolation": snapshot.auto_disabled,
                "conversationCapUsdMicros": snapshot.conversation_cap_usd_micros,
                "settledSpendUsdMicros": snapshot.settled_spend_usd_micros,
                "reservedUsdMicros": snapshot.reserved_usd_micros,
                "remainingUsdMicros": snapshot.remaining_usd_micros,
            }),
            remote_sequence: None,
            command_id: None,
            created_at: None,
        },
    )?;
    Ok(())
}

struct BudgetRow {
    conversation_cap_usd_micros: u64,
    max_request_usd_micros: u64,
    providers: Vec<String>,
    settled_spend_usd_micros: u64,
    auto_disabled: bool,
}

struct ReservationRow {
    approval_id: String,
    session_id: String,
    reserved_usd_micros: u64,
    status: String,
}

fn load_budget(conn: &Connection, session_id: &str) -> Result<Option<BudgetRow>> {
    let row = conn
        .query_row(
            "SELECT conversation_cap_usd_micros, max_request_usd_micros, providers_json,
                    settled_spend_usd_micros, auto_disabled
             FROM paid_compute_conversation_budgets WHERE session_id=?1",
            params![session_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .optional()?;
    let Some((cap, max_request, providers_json, settled, disabled)) = row else {
        return Ok(None);
    };
    Ok(Some(BudgetRow {
        conversation_cap_usd_micros: as_u64(cap, "conversation_cap_usd_micros")?,
        max_request_usd_micros: as_u64(max_request, "max_request_usd_micros")?,
        providers: serde_json::from_str(&providers_json)?,
        settled_spend_usd_micros: as_u64(settled, "settled_spend_usd_micros")?,
        auto_disabled: disabled != 0,
    }))
}

fn load_reservation(conn: &Connection, approval_id: &str) -> Result<Option<ReservationRow>> {
    let row = conn
        .query_row(
            "SELECT approval_id, session_id, reserved_usd_micros, status
             FROM paid_compute_reservations WHERE approval_id=?1",
            params![approval_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    let Some((approval_id, session_id, reserved, status)) = row else {
        return Ok(None);
    };
    Ok(Some(ReservationRow {
        approval_id,
        session_id,
        reserved_usd_micros: as_u64(reserved, "reserved_usd_micros")?,
        status,
    }))
}

fn active_reservations(conn: &Connection, session_id: &str) -> Result<u64> {
    let total: i64 = conn.query_row(
        "SELECT COALESCE(SUM(reserved_usd_micros), 0)
         FROM paid_compute_reservations
         WHERE session_id=?1 AND status='reserved'",
        params![session_id],
        |row| row.get(0),
    )?;
    as_u64(total, "active reservations")
}

fn reserved_micros_for_digest(
    conn: &Connection,
    session_id: &str,
    preparation_digest: &str,
) -> Result<u64> {
    let total: i64 = conn.query_row(
        "SELECT COALESCE(SUM(reserved_usd_micros), 0)
         FROM paid_compute_reservations
         WHERE session_id=?1 AND preparation_digest=?2 AND status='reserved'",
        params![session_id, preparation_digest],
        |row| row.get(0),
    )?;
    as_u64(total, "digest reservations")
}

pub(crate) fn reserved_approval_ids_for_digest(
    conn: &Connection,
    session_id: &str,
    preparation_digest: &str,
) -> Result<Vec<String>> {
    let mut statement = conn.prepare(
        "SELECT approval_id FROM paid_compute_reservations
         WHERE session_id=?1 AND preparation_digest=?2 AND status='reserved'
         ORDER BY created_at DESC",
    )?;
    let ids = statement
        .query_map(params![session_id, preparation_digest], |row| row.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(ids)
}

pub(crate) fn list_reserved(conn: &Connection) -> Result<Vec<(String, String, Option<String>)>> {
    let mut statement = conn.prepare(
        "SELECT approval_id, session_id, preparation_digest
         FROM paid_compute_reservations
         WHERE status='reserved'
         ORDER BY created_at ASC",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn apply_exact_settlement(
    conn: &Connection,
    reservation: &ReservationRow,
    cost_usd_micros: u64,
) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    let violation = cost_usd_micros > reservation.reserved_usd_micros;
    let status = if cost_usd_micros == 0 {
        "released"
    } else {
        "settled"
    };
    conn.execute(
        "UPDATE paid_compute_reservations
         SET status=?1, settled_usd_micros=?2, updated_at=?3
         WHERE approval_id=?4 AND status='reserved'",
        params![status, cost_usd_micros as i64, now, reservation.approval_id],
    )?;
    if cost_usd_micros > 0 {
        conn.execute(
            "UPDATE paid_compute_conversation_budgets
             SET settled_spend_usd_micros = settled_spend_usd_micros + ?1,
                 auto_disabled = CASE WHEN ?2 THEN 1 ELSE auto_disabled END,
                 updated_at=?3
             WHERE session_id=?4",
            params![
                cost_usd_micros as i64,
                i64::from(violation),
                now,
                reservation.session_id
            ],
        )?;
    } else if violation {
        conn.execute(
            "UPDATE paid_compute_conversation_budgets
             SET auto_disabled=1, updated_at=?1
             WHERE session_id=?2",
            params![now, reservation.session_id],
        )?;
    }
    Ok(())
}

pub(crate) fn disable_auto(conn: &Connection, session_id: &str) -> Result<()> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE paid_compute_conversation_budgets
         SET auto_disabled=1, updated_at=?1
         WHERE session_id=?2",
        params![now, session_id],
    )?;
    Ok(())
}

pub(crate) fn micros_from_reported_cost(cost_usd: f64) -> Option<u64> {
    if !cost_usd.is_finite() || cost_usd < 0.0 {
        return None;
    }
    let micros = (cost_usd * 1_000_000.0).round();
    if micros > u64::MAX as f64 {
        return None;
    }
    Some(micros as u64)
}

fn as_u64(value: i64, field: &str) -> Result<u64> {
    u64::try_from(value).map_err(|_| anyhow!("paid-compute {field} is negative"))
}

/// Format the auto-approval notice: request cap and conversation remainder.
pub(crate) fn auto_approval_notice(grant: &ReservationGrant) -> String {
    let used = grant
        .conversation_cap_usd_micros
        .saturating_sub(grant.remaining_usd_micros);
    format!(
        "Auto-approved a ${} maximum · ${} of ${} conversation allowance used",
        format_usd_micros(grant.reserved_usd_micros),
        format_usd_micros(used),
        format_usd_micros(grant.conversation_cap_usd_micros),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Database;
    use tempfile::tempdir;

    fn policy(enabled: bool, request: u64, conversation: u64) -> PaidComputeAutoApprovalPolicy {
        PaidComputeAutoApprovalPolicy {
            enabled,
            max_request_usd_micros: request,
            max_conversation_usd_micros: conversation,
            providers: vec!["openrouter".into()],
        }
    }

    fn db() -> (tempfile::TempDir, Database) {
        let dir = tempdir().unwrap();
        let database = Database::open(dir.path().join("core.db")).unwrap();
        (dir, database)
    }

    #[test]
    fn default_disabled_policy_does_not_seed_a_budget() {
        let (_dir, database) = db();
        database
            .with_conn(|conn| {
                seed_conversation_budget(conn, "sess-a", &policy(false, 100_000, 250_000))?;
                assert!(snapshot(conn, "sess-a")?.is_none());
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn reservation_uses_the_hard_ceiling_and_rejects_over_request_or_remainder() {
        let (_dir, database) = db();
        database
            .with_conn(|conn| {
                seed_conversation_budget(conn, "sess-a", &policy(true, 100_000, 250_000))?;
                let first = try_reserve(conn, "sess-a", "approval-1", Some("sha256:a"), 60_000)?
                    .expect("0.06 is eligible");
                assert_eq!(first.reserved_usd_micros, 60_000);
                assert_eq!(first.remaining_usd_micros, 190_000);
                assert!(try_reserve(conn, "sess-a", "approval-over", None, 200_000)?.is_none());
                assert!(try_reserve(conn, "sess-a", "approval-none", None, 0)?.is_none());
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn retried_start_replaces_the_same_digest_reservation() {
        let (_dir, database) = db();
        database
            .with_conn(|conn| {
                seed_conversation_budget(conn, "sess-a", &policy(true, 100_000, 250_000))?;
                try_reserve(conn, "sess-a", "approval-1", Some("sha256:prep"), 60_000)?
                    .expect("first reserve");
                let second = try_reserve(conn, "sess-a", "approval-2", Some("sha256:prep"), 60_000)?
                    .expect("retry replaces rather than stacking");
                assert_eq!(second.reserved_usd_micros, 60_000);
                assert_eq!(second.remaining_usd_micros, 190_000);
                assert_eq!(active_reservations(conn, "sess-a")?, 60_000);
                let ids = reserved_approval_ids_for_digest(conn, "sess-a", "sha256:prep")?;
                assert_eq!(ids, vec!["approval-2".to_string()]);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn concurrent_style_second_reserve_cannot_oversubscribe() {
        let (_dir, database) = db();
        database
            .transaction(|conn| {
                seed_conversation_budget(conn, "sess-a", &policy(true, 100_000, 250_000))?;
                try_reserve(conn, "sess-a", "approval-1", None, 60_000)?.expect("first reserve");
                let second = try_reserve(conn, "sess-a", "approval-2", None, 100_000)?;
                let third = try_reserve(conn, "sess-a", "approval-3", None, 100_000)?;
                assert!(second.is_some(), "0.10 fits in the remaining 0.19");
                assert!(third.is_none(), "the second 0.10 would oversubscribe");
                assert_eq!(active_reservations(conn, "sess-a")?, 160_000);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn unknown_cost_retains_the_reservation_and_exact_cost_replaces_it() {
        let (_dir, database) = db();
        database
            .with_conn(|conn| {
                seed_conversation_budget(conn, "sess-a", &policy(true, 100_000, 250_000))?;
                try_reserve(conn, "sess-a", "approval-1", Some("sha256:a"), 60_000)?;
                settle(conn, "sess-a", "approval-1", SettlementOutcome::Unknown)?.unwrap();
                let held = snapshot(conn, "sess-a")?.unwrap();
                assert_eq!(held.reserved_usd_micros, 60_000);
                assert_eq!(held.settled_spend_usd_micros, 0);
                settle(
                    conn,
                    "sess-a",
                    "approval-1",
                    SettlementOutcome::Exact {
                        cost_usd_micros: 18_000,
                    },
                )?;
                let settled = snapshot(conn, "sess-a")?.unwrap();
                assert_eq!(settled.reserved_usd_micros, 0);
                assert_eq!(settled.settled_spend_usd_micros, 18_000);
                assert_eq!(settled.remaining_usd_micros, 232_000);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn proven_zero_releases_the_reservation() {
        let (_dir, database) = db();
        database
            .with_conn(|conn| {
                seed_conversation_budget(conn, "sess-a", &policy(true, 100_000, 250_000))?;
                try_reserve(conn, "sess-a", "approval-1", None, 60_000)?;
                settle(
                    conn,
                    "sess-a",
                    "approval-1",
                    SettlementOutcome::Exact { cost_usd_micros: 0 },
                )?;
                let snap = snapshot(conn, "sess-a")?.unwrap();
                assert_eq!(snap.reserved_usd_micros, 0);
                assert_eq!(snap.settled_spend_usd_micros, 0);
                assert_eq!(snap.remaining_usd_micros, 250_000);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn overspend_disables_further_automatic_approvals() {
        let (_dir, database) = db();
        database
            .with_conn(|conn| {
                seed_conversation_budget(conn, "sess-a", &policy(true, 100_000, 250_000))?;
                try_reserve(conn, "sess-a", "approval-1", None, 60_000)?;
                settle(
                    conn,
                    "sess-a",
                    "approval-1",
                    SettlementOutcome::Exact {
                        cost_usd_micros: 80_000,
                    },
                )?;
                let snap = snapshot(conn, "sess-a")?.unwrap();
                assert!(snap.auto_disabled);
                assert!(try_reserve(conn, "sess-a", "approval-2", None, 10_000)?.is_none());
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn forked_conversations_do_not_share_totals() {
        let (_dir, database) = db();
        database
            .with_conn(|conn| {
                seed_conversation_budget(conn, "sess-a", &policy(true, 100_000, 250_000))?;
                seed_conversation_budget(conn, "sess-b", &policy(true, 100_000, 250_000))?;
                try_reserve(conn, "sess-a", "approval-1", None, 60_000)?;
                let child = snapshot(conn, "sess-b")?.unwrap();
                assert_eq!(child.reserved_usd_micros, 0);
                assert_eq!(child.remaining_usd_micros, 250_000);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn restart_restores_settled_spend_and_outstanding_reservations() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("core.db");
        {
            let database = Database::open(&path).unwrap();
            database
                .with_conn(|conn| {
                    seed_conversation_budget(conn, "sess-a", &policy(true, 100_000, 250_000))?;
                    try_reserve(conn, "sess-a", "approval-1", None, 60_000)?;
                    settle(
                        conn,
                        "sess-a",
                        "approval-1",
                        SettlementOutcome::Exact {
                            cost_usd_micros: 18_000,
                        },
                    )?;
                    try_reserve(conn, "sess-a", "approval-2", None, 60_000)?;
                    Ok(())
                })
                .unwrap();
        }
        let reopened = Database::open(&path).unwrap();
        reopened
            .with_conn(|conn| {
                let snap = snapshot(conn, "sess-a")?.unwrap();
                assert_eq!(snap.settled_spend_usd_micros, 18_000);
                assert_eq!(snap.reserved_usd_micros, 60_000);
                assert_eq!(snap.remaining_usd_micros, 172_000);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn auto_approval_notice_shows_request_cap_and_used_allowance() {
        let grant = ReservationGrant {
            reserved_usd_micros: 60_000,
            conversation_cap_usd_micros: 250_000,
            settled_spend_usd_micros: 0,
            remaining_usd_micros: 190_000,
        };
        assert_eq!(
            auto_approval_notice(&grant),
            "Auto-approved a $0.06 maximum · $0.06 of $0.25 conversation allowance used"
        );
    }
}
