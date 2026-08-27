//! SQLite persistence for kernel projections. CoreRuntime is the writer.

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};

use super::admission::{AdmissionCommit, RunDraft};
use super::algorithm::AlgorithmProjection;
use super::commit::RunKernelState;
use super::evidence::{EvidenceRef, EvidenceState, SealedTerminal};
use super::types::{
    classify_legacy_status, AdmissionState, AlgorithmKind, EvidenceCompleteness,
    ExecutionPlacement, RunCondition, RunLifecycle, RunPhase,
};
use super::work::WorkItem;
use super::KERNEL_SCHEMA_VERSION;

pub fn upsert_run_columns(conn: &Connection, state: &RunKernelState) -> Result<()> {
    conn.execute(
        "UPDATE optimizer_runs
         SET lifecycle = ?2,
             phase = ?3,
             condition = ?4,
             placement = ?5,
             aggregate_sequence = ?6,
             projection_revision = ?7
         WHERE id = ?1",
        params![
            state.run_id,
            state.lifecycle.as_str(),
            state.phase.map(|phase| phase.as_str()),
            state.condition.as_str(),
            state.placement.as_str(),
            state.aggregate_sequence as i64,
            state.projection_revision as i64,
        ],
    )
    .context("update optimizer run kernel columns")?;
    Ok(())
}

pub fn upsert_projection(conn: &Connection, state: &RunKernelState) -> Result<()> {
    let payload =
        serde_json::to_string(&state.projection).context("serialize algorithm projection")?;
    conn.execute(
        "INSERT INTO optimizer_algorithm_projections(
            optimizer_run_id, algorithm, reducer_version, as_of_sequence, projection_json, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'))
         ON CONFLICT(optimizer_run_id) DO UPDATE SET
            algorithm=excluded.algorithm,
            reducer_version=excluded.reducer_version,
            as_of_sequence=excluded.as_of_sequence,
            projection_json=excluded.projection_json,
            updated_at=excluded.updated_at",
        params![
            state.run_id,
            state.algorithm.wire_id(),
            state.algorithm.reducer_version(),
            state.aggregate_sequence as i64,
            payload,
        ],
    )
    .context("upsert optimizer algorithm projection")?;
    upsert_run_columns(conn, state)?;
    replace_work_items(conn, &state.run_id, state.projection.work_items())?;
    Ok(())
}

fn replace_work_items(conn: &Connection, run_id: &str, items: &[WorkItem]) -> Result<()> {
    conn.execute(
        "DELETE FROM optimizer_work_items WHERE optimizer_run_id = ?1",
        params![run_id],
    )?;
    for item in items {
        conn.execute(
            "INSERT INTO optimizer_work_items(
                work_item_id, optimizer_run_id, kind, lifecycle, terminal, external_ref, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'), datetime('now'))",
            params![
                item.work_item_id,
                run_id,
                item.kind.as_str(),
                item.lifecycle.as_str(),
                item.terminal.map(|kind| kind.as_str()),
                item.external_ref.as_deref(),
            ],
        )
        .with_context(|| format!("insert work item {}", item.work_item_id))?;
    }
    Ok(())
}

pub fn load_projection_json(conn: &Connection, run_id: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT projection_json FROM optimizer_algorithm_projections WHERE optimizer_run_id = ?1",
        params![run_id],
        |row| row.get(0),
    )
    .optional()
    .context("load optimizer algorithm projection")
}

/// Load the durable kernel projection without replaying producer events.
///
/// The algorithm payload and its cursor are one projection row; common
/// lifecycle fields live on `optimizer_runs`, and the immutable digest lives
/// on `optimizer_run_specs`. Missing pieces are an error rather than an empty
/// or zero-valued view.
pub fn load_state(conn: &Connection, run_id: &str) -> Result<Option<RunKernelState>> {
    let row = conn
        .query_row(
            "SELECT r.algorithm_id,
                    r.status,
                    r.lifecycle,
                    r.phase,
                    r.condition,
                    r.placement,
                    r.aggregate_sequence,
                    r.projection_revision,
                    r.finished_at,
                    r.updated_at,
                    terminal.terminal_cursor,
                    terminal.sealed_at,
                    r.terminal_failure_id,
                    s.spec_digest,
                    p.algorithm,
                    p.as_of_sequence,
                    p.projection_json
             FROM optimizer_runs r
             JOIN optimizer_algorithm_projections p
               ON p.optimizer_run_id = r.id
             LEFT JOIN optimizer_run_specs s
               ON s.optimizer_run_id = r.id
             LEFT JOIN optimizer_terminal_manifests terminal
               ON terminal.optimizer_run_id = r.id
             WHERE r.id = ?1",
            [run_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, Option<String>>(8)?,
                    row.get::<_, String>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                    row.get::<_, Option<String>>(11)?,
                    row.get::<_, Option<String>>(12)?,
                    row.get::<_, Option<String>>(13)?,
                    row.get::<_, String>(14)?,
                    row.get::<_, i64>(15)?,
                    row.get::<_, String>(16)?,
                ))
            },
        )
        .optional()
        .context("load durable optimizer kernel state")?;
    let Some((
        algorithm_id,
        legacy_status,
        lifecycle,
        phase,
        condition,
        placement,
        aggregate_sequence,
        projection_revision,
        finished_at,
        updated_at,
        terminal_sequence,
        terminal_sealed_at,
        failure_ref,
        spec_digest,
        projection_algorithm,
        projection_sequence,
        projection_json,
    )) = row
    else {
        return Ok(None);
    };

    let algorithm =
        AlgorithmKind::parse_wire(&algorithm_id).map_err(|error| anyhow::anyhow!("{error}"))?;
    let projected_algorithm = AlgorithmKind::parse_wire(&projection_algorithm)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    if projected_algorithm != algorithm {
        anyhow::bail!(
            "optimizer run {run_id} algorithm {} disagrees with projection {}",
            algorithm.wire_id(),
            projected_algorithm.wire_id()
        );
    }
    let projection: AlgorithmProjection = serde_json::from_str(&projection_json)
        .with_context(|| format!("decode kernel projection for {run_id}"))?;
    if projection.kind() != algorithm {
        anyhow::bail!(
            "optimizer run {run_id} projection payload belongs to {} rather than {}",
            projection.kind().wire_id(),
            algorithm.wire_id()
        );
    }
    let legacy = classify_legacy_status(&legacy_status).ok_or_else(|| {
        anyhow::anyhow!("optimizer run {run_id} has unknown stored status {legacy_status:?}")
    })?;
    let lifecycle = lifecycle
        .as_deref()
        .map(RunLifecycle::parse)
        .transpose()
        .map_err(|error| anyhow::anyhow!("{error}"))?
        .unwrap_or(legacy.0);
    let phase = phase
        .as_deref()
        .map(RunPhase::parse)
        .transpose()
        .map_err(|error| anyhow::anyhow!("{error}"))?
        .or(legacy.1);
    let condition = condition
        .as_deref()
        .map(RunCondition::parse)
        .transpose()
        .map_err(|error| anyhow::anyhow!("{error}"))?
        .unwrap_or(legacy.2);
    let placement = placement
        .as_deref()
        .map(ExecutionPlacement::parse)
        .transpose()
        .map_err(|error| anyhow::anyhow!("{error}"))?
        .ok_or_else(|| anyhow::anyhow!("optimizer run {run_id} is missing execution placement"))?;
    let aggregate_sequence = aggregate_sequence
        .ok_or_else(|| anyhow::anyhow!("optimizer run {run_id} is missing aggregate sequence"))?;
    if aggregate_sequence != projection_sequence {
        anyhow::bail!(
            "optimizer run {run_id} cursor {aggregate_sequence} disagrees with projection {projection_sequence}"
        );
    }
    let projection_revision = projection_revision
        .ok_or_else(|| anyhow::anyhow!("optimizer run {run_id} is missing projection revision"))?;
    let spec_digest = spec_digest
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("optimizer run {run_id} is missing its admitted spec"))?;
    let sealed_evidence = projection.evidence_state();
    let current_evidence = load_current_evidence(conn, run_id, &sealed_evidence)?;
    let terminal = if lifecycle.is_terminal() {
        let (kind, reason) = legacy.3.ok_or_else(|| {
            anyhow::anyhow!(
                "optimizer run {run_id} is terminal but status {legacy_status:?} has no terminal outcome"
            )
        })?;
        Some(SealedTerminal {
            kind,
            reason,
            final_sequence: terminal_sequence.unwrap_or(aggregate_sequence) as u64,
            evidence: sealed_evidence,
            failure_ref: failure_ref.clone(),
            sealed_at: terminal_sealed_at.or(finished_at).unwrap_or(updated_at),
        })
    } else {
        None
    };
    Ok(Some(RunKernelState {
        schema_version: KERNEL_SCHEMA_VERSION.into(),
        run_id: run_id.to_string(),
        algorithm,
        lifecycle,
        phase,
        condition,
        placement,
        aggregate_sequence: u64::try_from(aggregate_sequence)
            .context("optimizer aggregate sequence is negative")?,
        projection_revision: u64::try_from(projection_revision)
            .context("optimizer projection revision is negative")?,
        spec_digest,
        terminal,
        failure_ref,
        current_evidence,
        projection,
    }))
}

fn load_current_evidence(
    conn: &Connection,
    run_id: &str,
    base: &EvidenceState,
) -> Result<Option<EvidenceState>> {
    let mut evidence = base.clone();
    let mut amended = false;
    let mut statement = conn.prepare(
        "SELECT evidence_json
         FROM optimizer_evidence_amendments
         WHERE optimizer_run_id = ?1
         ORDER BY recorded_at, amendment_id",
    )?;
    let rows = statement.query_map([run_id], |row| row.get::<_, String>(0))?;
    for row in rows {
        amended = true;
        let event: serde_json::Value =
            serde_json::from_str(&row?).context("decode optimizer evidence amendment")?;
        if let Some(degradation) = event.pointer("/delta/degradation") {
            evidence.completeness = EvidenceCompleteness::Unusable;
            evidence.reason = degradation
                .get("reason")
                .and_then(|value| value.as_str())
                .map(str::to_string)
                .or_else(|| Some("late evidence degradation".into()));
        }
    }

    let mut statement = conn.prepare(
        "SELECT kind, ref_id, digest
         FROM optimizer_evidence_refs
         WHERE optimizer_run_id = ?1
         ORDER BY recorded_at, kind, ref_id",
    )?;
    let rows = statement.query_map([run_id], |row| {
        Ok(EvidenceRef {
            kind: row.get(0)?,
            id: row.get(1)?,
            digest: row.get(2)?,
        })
    })?;
    for row in rows {
        amended = true;
        let reference = row?;
        if !evidence
            .refs
            .iter()
            .any(|existing| existing.kind == reference.kind && existing.id == reference.id)
        {
            evidence.refs.push(reference);
        }
    }
    if !evidence.refs.is_empty() && evidence.completeness == EvidenceCompleteness::Absent {
        evidence.completeness = EvidenceCompleteness::Partial;
        evidence.reason = None;
    }
    Ok(amended.then_some(evidence))
}

pub fn insert_draft(conn: &Connection, draft: &RunDraft) -> Result<()> {
    conn.execute(
        "INSERT INTO optimizer_run_drafts(
            id, algorithm, spec_json, spec_digest, admission_state, authorization_ref, created_at, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(id) DO UPDATE SET
            algorithm=excluded.algorithm,
            spec_json=excluded.spec_json,
            spec_digest=excluded.spec_digest,
            admission_state=excluded.admission_state,
            authorization_ref=excluded.authorization_ref,
            updated_at=excluded.updated_at",
        params![
            draft.draft_id,
            draft.algorithm.wire_id(),
            draft.spec_json,
            draft.spec_digest,
            draft.admission.as_str(),
            draft.authorization_ref,
            draft.created_at,
            draft.updated_at,
        ],
    )
    .context("upsert optimizer run draft")?;
    Ok(())
}

pub fn load_draft(conn: &Connection, draft_id: &str) -> Result<Option<RunDraft>> {
    let row = conn
        .query_row(
            "SELECT algorithm, spec_json, spec_digest, admission_state, authorization_ref, created_at, updated_at
             FROM optimizer_run_drafts WHERE id = ?1",
            [draft_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                ))
            },
        )
        .optional()
        .context("load optimizer run draft")?;
    let Some((
        algorithm,
        spec_json,
        spec_digest,
        admission,
        authorization_ref,
        created_at,
        updated_at,
    )) = row
    else {
        return Ok(None);
    };
    Ok(Some(RunDraft {
        draft_id: draft_id.to_string(),
        algorithm: AlgorithmKind::parse_wire(&algorithm)
            .map_err(|error| anyhow::anyhow!("{error}"))?,
        spec_digest,
        spec_json,
        admission: AdmissionState::parse(&admission).map_err(|error| anyhow::anyhow!("{error}"))?,
        authorization_ref,
        created_at,
        updated_at,
    }))
}

pub fn consume_draft(conn: &Connection, draft_id: &str, at: &str) -> Result<RunDraft> {
    let mut draft = load_draft(conn, draft_id)?
        .ok_or_else(|| anyhow::anyhow!("optimizer run draft `{draft_id}` does not exist"))?;
    draft
        .transition(AdmissionState::Consumed, at)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    insert_draft(conn, &draft)?;
    Ok(draft)
}

pub fn insert_spec(conn: &Connection, commit: &AdmissionCommit) -> Result<()> {
    let authorization_json = serde_json::to_string(&serde_json::json!({
        "authorizationRef": commit.authorization_ref,
        "draftId": commit.draft_id,
    }))
    .context("serialize admission authorization")?;
    conn.execute(
        "INSERT INTO optimizer_run_specs(
            optimizer_run_id, spec_json, spec_digest, authorization_json, admitted_at
         ) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(optimizer_run_id) DO UPDATE SET
            spec_json=excluded.spec_json,
            spec_digest=excluded.spec_digest,
            authorization_json=excluded.authorization_json,
            admitted_at=excluded.admitted_at",
        params![
            commit.run_id,
            commit.spec_json,
            commit.spec_digest,
            authorization_json,
            commit.admitted_at,
        ],
    )
    .context("upsert optimizer run spec")?;
    Ok(())
}
