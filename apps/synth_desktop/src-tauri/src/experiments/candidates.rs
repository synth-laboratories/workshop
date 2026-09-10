//! Durable Candidate rows hanging off an `optimizer_run` member.
//!
//! Workshop projects producer identity. It does not re-apply, re-compile, or
//! restart per seed. Empty `candidates[]` on SFT/CISPO is honest.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

use crate::contract::specta::OpaqueJson;
use crate::lineage::CandidateRecord;

use super::MEMBER_OPTIMIZER;

pub struct CandidateUpsert {
    pub optimizer_run_id: String,
    pub producer_candidate_id: String,
    pub kind: Option<String>,
    pub protocol_id: Option<String>,
    pub status: Option<String>,
    pub parent_ids: Vec<String>,
    pub metrics: Option<serde_json::Value>,
    pub content_digest: Option<String>,
    pub compared_with: Option<Vec<String>>,
    pub promoted_to: Option<String>,
    pub at: String,
}

pub fn upsert(conn: &Connection, request: CandidateUpsert) -> Result<()> {
    let producer = request.producer_candidate_id.trim();
    if producer.is_empty() {
        return Ok(());
    }
    let experiment_id: Option<String> = conn
        .query_row(
            "SELECT group_id FROM experiment_group_members WHERE member_kind=?1 AND member_id=?2",
            params![MEMBER_OPTIMIZER, request.optimizer_run_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(experiment_id) = experiment_id else {
        return Ok(());
    };
    let id = format!("can:{}:{producer}", request.optimizer_run_id);
    let parent_ids_json = serde_json::to_string(&request.parent_ids)?;
    let metrics_json = request
        .metrics
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    let compared_with_json = request
        .compared_with
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;
    conn.execute(
        "INSERT INTO experiment_candidates(
            id, experiment_id, optimizer_run_id, producer_candidate_id,
            kind, protocol_id, status, parent_ids_json, metrics_json,
            content_digest, compared_with_json, promoted_to, created_at, updated_at
         ) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,COALESCE(?11,'[]'),?12,?13,?13)
         ON CONFLICT(optimizer_run_id, producer_candidate_id) DO UPDATE SET
            kind=COALESCE(excluded.kind, experiment_candidates.kind),
            protocol_id=COALESCE(excluded.protocol_id, experiment_candidates.protocol_id),
            status=COALESCE(excluded.status, experiment_candidates.status),
            parent_ids_json=CASE
                WHEN excluded.parent_ids_json != '[]' THEN excluded.parent_ids_json
                ELSE experiment_candidates.parent_ids_json
            END,
            metrics_json=COALESCE(excluded.metrics_json, experiment_candidates.metrics_json),
            content_digest=COALESCE(excluded.content_digest, experiment_candidates.content_digest),
            compared_with_json=CASE
                WHEN ?11 IS NOT NULL THEN excluded.compared_with_json
                ELSE experiment_candidates.compared_with_json
            END,
            promoted_to=COALESCE(excluded.promoted_to, experiment_candidates.promoted_to),
            updated_at=excluded.updated_at",
        params![
            id,
            experiment_id,
            request.optimizer_run_id,
            producer,
            request.kind,
            request.protocol_id,
            request.status,
            parent_ids_json,
            metrics_json,
            request.content_digest,
            compared_with_json,
            request.promoted_to,
            request.at,
        ],
    )?;
    Ok(())
}

pub fn load_for_experiment(conn: &Connection, experiment_id: &str) -> Result<Vec<CandidateRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, experiment_id, optimizer_run_id, producer_candidate_id,
                kind, protocol_id, status, parent_ids_json, metrics_json,
                content_digest, compared_with_json, promoted_to, created_at, updated_at
           FROM experiment_candidates
          WHERE experiment_id=?1
          ORDER BY created_at, producer_candidate_id, id",
    )?;
    let rows = stmt
        .query_map(params![experiment_id], candidate_from_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn load_by_id(conn: &Connection, id: &str) -> Result<Option<CandidateRecord>> {
    conn.query_row(
        "SELECT id, experiment_id, optimizer_run_id, producer_candidate_id,
                kind, protocol_id, status, parent_ids_json, metrics_json,
                content_digest, compared_with_json, promoted_to, created_at, updated_at
           FROM experiment_candidates WHERE id=?1",
        [id],
        candidate_from_row,
    )
    .optional()
    .map_err(Into::into)
}

pub fn append_compared_with(
    conn: &Connection,
    candidate_id: &str,
    other_id: &str,
    at: &str,
) -> Result<bool> {
    let Some(record) = load_by_id(conn, candidate_id)? else {
        anyhow::bail!("unknown candidate {candidate_id}");
    };
    if record.compared_with.iter().any(|id| id == other_id) {
        return Ok(false);
    }
    let mut next = record.compared_with;
    next.push(other_id.to_owned());
    conn.execute(
        "UPDATE experiment_candidates SET compared_with_json=?2, updated_at=?3 WHERE id=?1",
        params![candidate_id, serde_json::to_string(&next)?, at],
    )?;
    Ok(true)
}

pub fn set_promoted_to(
    conn: &Connection,
    source_id: &str,
    target_id: &str,
    at: &str,
) -> Result<bool> {
    let Some(record) = load_by_id(conn, source_id)? else {
        anyhow::bail!("unknown candidate {source_id}");
    };
    if record.promoted_to.as_deref() == Some(target_id) {
        return Ok(false);
    }
    let next_status = match record.status.as_deref() {
        None | Some("") | Some("accepted") => Some("promoted"),
        _ => record.status.as_deref(),
    };
    conn.execute(
        "UPDATE experiment_candidates SET promoted_to=?2, status=COALESCE(?3, status), updated_at=?4 WHERE id=?1",
        params![source_id, target_id, next_status, at],
    )?;
    Ok(true)
}

fn candidate_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<CandidateRecord> {
    Ok(CandidateRecord {
        id: row.get(0)?,
        experiment_id: row.get(1)?,
        optimizer_run_id: row.get(2)?,
        producer_candidate_id: row.get(3)?,
        kind: row.get(4)?,
        protocol_id: row.get(5)?,
        status: row.get(6)?,
        parent_ids: serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_default(),
        metrics: row
            .get::<_, Option<String>>(8)?
            .and_then(|value| serde_json::from_str(&value).ok())
            .map(OpaqueJson),
        content_digest: row.get(9)?,
        compared_with: serde_json::from_str(&row.get::<_, String>(10)?).unwrap_or_default(),
        promoted_to: row.get(11)?,
        created_at: row.get(12)?,
        updated_at: row.get(13)?,
    })
}
