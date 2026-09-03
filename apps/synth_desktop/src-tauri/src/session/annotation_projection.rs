//! Local durable projections of container-authoritative Trace V5 annotations.
//!
//! Workshop caches bounded summaries so `analysis.annotation_workbench.v1` can
//! reload without hydrating a trace journal or contacting the container. The
//! visual is a projection: evidence-head and verifier-result digests remain the
//! identities, and missing verifier evidence is stored as unavailable rather
//! than a zero score.

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};

pub const WORKBENCH_SCHEMA: &str = "synth.annotation-workbench.v1";

#[derive(Clone, Debug)]
pub struct EvidenceHeadRow {
    pub digest: String,
    pub bundle_id: Option<String>,
    pub trace_digest: String,
    pub campaign_id: Option<String>,
    pub annotation_count: i64,
    pub verifier_result_count: i64,
    pub summary: Value,
}

#[derive(Clone, Debug)]
pub struct RubricResultRow {
    pub digest: String,
    pub available: bool,
    pub unavailable_reason: Option<String>,
    pub summary: Value,
}

pub fn upsert_campaign(
    conn: &Connection,
    campaign_id: &str,
    status: &str,
    metadata: &Value,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let traces = serde_json::to_string(
        metadata.get("traces").unwrap_or(&json!([])),
    )?;
    let annotators = serde_json::to_string(
        metadata.get("annotators").unwrap_or(&json!([])),
    )?;
    let coverage = serde_json::to_string(
        metadata.get("coverage").unwrap_or(&json!({})),
    )?;
    let cost = serde_json::to_string(metadata.get("cost").unwrap_or(&json!({})))?;
    let metadata_json = serde_json::to_string(metadata)?;
    conn.execute(
        "INSERT INTO annotation_campaigns(
            campaign_id, container_id, eval_run_id, session_id, label, domain,
            status, traces_json, annotators_json, coverage_json, cost_json,
            metadata_json, created_at, updated_at
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)
         ON CONFLICT(campaign_id) DO UPDATE SET
            container_id=excluded.container_id,
            eval_run_id=excluded.eval_run_id,
            session_id=excluded.session_id,
            label=excluded.label,
            domain=excluded.domain,
            status=excluded.status,
            traces_json=excluded.traces_json,
            annotators_json=excluded.annotators_json,
            coverage_json=excluded.coverage_json,
            cost_json=excluded.cost_json,
            metadata_json=excluded.metadata_json,
            updated_at=excluded.updated_at",
        params![
            campaign_id,
            metadata.get("containerId").and_then(Value::as_str),
            metadata.get("evalRunId").and_then(Value::as_str),
            metadata.get("sessionId").and_then(Value::as_str),
            metadata.get("label").and_then(Value::as_str),
            metadata.get("domain").and_then(Value::as_str),
            status,
            traces,
            annotators,
            coverage,
            cost,
            metadata_json,
            now,
        ],
    )?;
    Ok(())
}

pub fn upsert_evidence_head(
    conn: &Connection,
    digest: &str,
    trace_digest: &str,
    summary: &Value,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let annotation_count = summary
        .get("findings")
        .and_then(Value::as_array)
        .map(|rows| rows.len() as i64)
        .or_else(|| {
            summary
                .pointer("/evidenceHead/annotationCount")
                .and_then(Value::as_i64)
        })
        .unwrap_or(0);
    let summary_json = serde_json::to_string(summary)?;
    let verifier_result_count = summary
        .pointer("/evidenceHead/verifierResultCount")
        .and_then(Value::as_i64)
        .or_else(|| {
            summary
                .pointer("/rubric/available")
                .and_then(Value::as_bool)
                .map(|available| i64::from(available))
        })
        .unwrap_or(0);
    conn.execute(
        "INSERT INTO annotation_evidence_heads(
            digest, bundle_id, trace_digest, campaign_id, annotation_count,
            verifier_result_count, summary_json, created_at, updated_at
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
         ON CONFLICT(digest) DO UPDATE SET
            bundle_id=excluded.bundle_id,
            trace_digest=excluded.trace_digest,
            campaign_id=excluded.campaign_id,
            annotation_count=excluded.annotation_count,
            verifier_result_count=excluded.verifier_result_count,
            summary_json=excluded.summary_json,
            updated_at=excluded.updated_at",
        params![
            digest,
            summary.pointer("/evidenceHead/bundleId").and_then(Value::as_str),
            trace_digest,
            summary.pointer("/campaign/id").and_then(Value::as_str),
            annotation_count,
            verifier_result_count,
            summary_json,
            now,
        ],
    )?;
    Ok(())
}

pub fn upsert_unavailable_rubric(
    conn: &Connection,
    digest: &str,
    reason: &str,
    evidence_head_digest: Option<&str>,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let summary = json!({
        "available": false,
        "reason": reason,
        "digest": digest,
        "criteria": []
    });
    let summary_json = serde_json::to_string(&summary)?;
    conn.execute(
        "INSERT INTO rubric_results(
            digest, verifier_result_id, evidence_head_digest, rubric_id,
            rubric_digest, available, unavailable_reason, summary_json,
            created_at, updated_at
         ) VALUES(?1, NULL, ?2, NULL, NULL, 0, ?3, ?4, ?5, ?5)
         ON CONFLICT(digest) DO UPDATE SET
            evidence_head_digest=excluded.evidence_head_digest,
            available=0,
            unavailable_reason=excluded.unavailable_reason,
            summary_json=excluded.summary_json,
            updated_at=excluded.updated_at",
        params![digest, evidence_head_digest, reason, summary_json, now],
    )?;
    Ok(())
}

pub fn upsert_available_rubric(
    conn: &Connection,
    digest: &str,
    summary: &Value,
    evidence_head_digest: Option<&str>,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let summary_json = serde_json::to_string(summary)?;
    conn.execute(
        "INSERT INTO rubric_results(
            digest, verifier_result_id, evidence_head_digest, rubric_id,
            rubric_digest, available, unavailable_reason, summary_json,
            created_at, updated_at
         ) VALUES(?1, ?2, ?3, ?4, ?5, 1, NULL, ?6, ?7, ?7)
         ON CONFLICT(digest) DO UPDATE SET
            verifier_result_id=excluded.verifier_result_id,
            evidence_head_digest=excluded.evidence_head_digest,
            rubric_id=excluded.rubric_id,
            rubric_digest=excluded.rubric_digest,
            available=1,
            unavailable_reason=NULL,
            summary_json=excluded.summary_json,
            updated_at=excluded.updated_at",
        params![
            digest,
            summary.get("verifierResultId").and_then(Value::as_str),
            evidence_head_digest,
            summary.get("rubricId").and_then(Value::as_str),
            summary.get("rubricDigest").and_then(Value::as_str),
            summary_json,
            now,
        ],
    )?;
    Ok(())
}

pub fn get_evidence_head(conn: &Connection, digest: &str) -> Result<Option<EvidenceHeadRow>> {
    conn.query_row(
        "SELECT digest, bundle_id, trace_digest, campaign_id, annotation_count,
                verifier_result_count, summary_json
         FROM annotation_evidence_heads WHERE digest = ?1",
        [digest],
        |row| {
            let summary_json: String = row.get(6)?;
            Ok(EvidenceHeadRow {
                digest: row.get(0)?,
                bundle_id: row.get(1)?,
                trace_digest: row.get(2)?,
                campaign_id: row.get(3)?,
                annotation_count: row.get(4)?,
                verifier_result_count: row.get(5)?,
                summary: serde_json::from_str(&summary_json).unwrap_or(Value::Null),
            })
        },
    )
    .optional()
    .context("load annotation evidence head")
}

pub fn get_rubric_result(conn: &Connection, digest: &str) -> Result<Option<RubricResultRow>> {
    conn.query_row(
        "SELECT digest, available, unavailable_reason, summary_json
         FROM rubric_results WHERE digest = ?1",
        [digest],
        |row| {
            let summary_json: String = row.get(3)?;
            let available: i64 = row.get(1)?;
            Ok(RubricResultRow {
                digest: row.get(0)?,
                available: available != 0,
                unavailable_reason: row.get(2)?,
                summary: serde_json::from_str(&summary_json).unwrap_or(Value::Null),
            })
        },
    )
    .optional()
    .context("load rubric result")
}

pub fn projection_payload(conn: &Connection, kind: &str, digest: &str) -> Result<Value> {
    match kind {
        "annotation_evidence_head" => {
            let row = get_evidence_head(conn, digest)?.ok_or_else(|| {
                anyhow::anyhow!("annotation evidence head `{digest}` is not cached locally")
            })?;
            Ok(json!({
                "kind": kind,
                "digest": row.digest,
                "payload": row.summary,
            }))
        }
        "verifier_result_v2" => {
            let row = get_rubric_result(conn, digest)?.ok_or_else(|| {
                anyhow::anyhow!("verifier result `{digest}` is not cached locally")
            })?;
            anyhow::ensure!(
                row.available || row.unavailable_reason.is_some(),
                "verifier result `{digest}` has no availability claim"
            );
            Ok(json!({
                "kind": kind,
                "digest": row.digest,
                "payload": row.summary,
            }))
        }
        other => anyhow::bail!("unsupported analysis projection kind `{other}`"),
    }
}

const TERMINAL_JOB_STATES: &[&str] = &["sealed", "abstained", "failed", "cancelled"];

#[derive(Clone, Debug)]
pub struct JobRow {
    pub job_id: String,
    pub campaign_id: Option<String>,
    pub container_id: Option<String>,
    pub trace_id: Option<String>,
    pub trace_digest: String,
    pub annotator_id: String,
    pub state: String,
    pub evidence_head_digest: Option<String>,
    pub applied_count: Option<i64>,
    pub abstained_count: Option<i64>,
    pub rejected_count: Option<i64>,
    pub failure_reason: Option<String>,
    pub projected: bool,
}

#[derive(Clone, Debug)]
pub struct JobApplyOutcome {
    pub job_id: String,
    pub campaign_id: Option<String>,
    pub state: String,
    pub terminal: bool,
    pub trace_id: Option<String>,
    pub trace_digest: String,
    pub bundle_digest: Option<String>,
    pub already_projected: bool,
}

#[derive(Clone, Debug)]
pub struct ProjectedHead {
    pub digest: String,
    pub trace_id: String,
    pub trace_digest: String,
    pub container_id: Option<String>,
    pub campaign_id: Option<String>,
    pub campaign_status: String,
    pub session_id: Option<String>,
    pub eval_run_id: Option<String>,
    pub rubric_digest: Option<String>,
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn text<'a>(value: &'a Value, snake: &str, camel: &str) -> Option<&'a str> {
    value
        .get(snake)
        .or_else(|| value.get(camel))
        .and_then(Value::as_str)
        .filter(|item| !item.is_empty())
}

fn i64_field(value: &Value, snake: &str, camel: &str) -> Option<i64> {
    value
        .get(snake)
        .or_else(|| value.get(camel))
        .and_then(|item| {
            item.as_i64().or_else(|| item.as_u64().map(|n| n as i64))
        })
}

fn job_object(payload: &Value) -> &Value {
    payload.get("job").unwrap_or(payload)
}

fn is_terminal_state(state: &str) -> bool {
    TERMINAL_JOB_STATES.contains(&state)
}

fn finding_status(raw: Option<&str>) -> &'static str {
    match raw {
        Some("abstained") => "abstained",
        Some("rejected") => "rejected",
        _ => "applied",
    }
}

fn domain_from_annotator(annotator_id: &str) -> Option<&str> {
    annotator_id.split('.').next().filter(|part| !part.is_empty())
}

pub fn campaign_status_from_jobs(states: &[String]) -> &'static str {
    if states.is_empty() {
        return "submitted";
    }
    if states.iter().any(|state| !is_terminal_state(state)) {
        return "running";
    }
    let n = states.len();
    let sealed = states.iter().filter(|state| *state == "sealed").count();
    let failed = states.iter().filter(|state| *state == "failed").count();
    let cancelled = states.iter().filter(|state| *state == "cancelled").count();
    let abstained = states.iter().filter(|state| *state == "abstained").count();
    if sealed == n {
        "sealed"
    } else if failed == n {
        "failed"
    } else if cancelled == n {
        "cancelled"
    } else if abstained == n {
        "abstained"
    } else {
        "partially_sealed"
    }
}

pub fn apply_job_snapshot(
    conn: &Connection,
    campaign_id: Option<&str>,
    container_id: &str,
    payload: &Value,
) -> Result<JobApplyOutcome> {
    let job = job_object(payload);
    let job_id = text(job, "job_id", "jobId")
        .context("annotation job payload is missing job_id")?
        .to_string();
    let request = job.get("request").cloned().unwrap_or(Value::Null);
    let trace_digest = text(job, "source_trace_digest", "sourceTraceDigest")
        .or_else(|| text(&request, "source_trace_digest", "sourceTraceDigest"))
        .or_else(|| text(job, "trace_digest", "traceDigest"))
        .unwrap_or("")
        .to_string();
    anyhow::ensure!(
        !trace_digest.is_empty(),
        "annotation job `{job_id}` is missing a source trace digest"
    );
    let trace_id = text(job, "source_trace_id", "sourceTraceId")
        .or_else(|| text(&request, "source_trace_id", "sourceTraceId"))
        .or_else(|| text(job, "trace_id", "traceId"))
        .map(str::to_owned);
    let annotator_id = text(job, "annotator_id", "annotatorId")
        .or_else(|| text(&request, "annotator_id", "annotatorId"))
        .unwrap_or("unknown")
        .to_string();
    let annotator_digest = text(&request, "annotator_digest", "annotatorDigest");
    let repeat_index = i64_field(&request, "repeat_index", "repeatIndex").unwrap_or(0);
    let state = text(job, "state", "state").unwrap_or("prepared").to_string();
    let terminal = payload
        .get("terminal")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| is_terminal_state(&state));
    let bundle_digest = text(job, "bundle_digest", "bundleDigest").map(str::to_owned);
    let reservation_id = text(job, "reservation_id", "reservationId");
    let applied_count = i64_field(job, "applied_count", "appliedCount");
    let abstained_count = i64_field(job, "abstained_count", "abstainedCount");
    let rejected_count = i64_field(job, "rejected_count", "rejectedCount");
    let failure_reason = job
        .get("error")
        .and_then(|error| {
            text(error, "message", "message").or_else(|| text(error, "code", "code"))
        })
        .map(str::to_owned);
    let cost_usd_micros = job
        .pointer("/usage/cost_usd")
        .or_else(|| job.pointer("/usage/costUsd"))
        .and_then(Value::as_f64)
        .map(|usd| (usd * 1_000_000.0).round() as i64);
    let previous = load_job(conn, &job_id)?;
    let already_projected = previous
        .as_ref()
        .map(|row| {
            row.projected
                && row.state == state
                && row.evidence_head_digest.as_deref() == bundle_digest.as_deref()
        })
        .unwrap_or(false);
    let mut stored = payload.clone();
    if already_projected {
        if let Some(object) = stored.as_object_mut() {
            object.insert("projected".into(), json!(true));
        }
    }
    if let Some(campaign_id) = campaign_id {
        ensure_campaign_stub(conn, campaign_id, container_id)?;
    }
    let now = now_rfc3339();
    let payload_json = serde_json::to_string(&stored)?;
    conn.execute(
        "INSERT INTO annotation_jobs(
            job_id, campaign_id, container_id, trace_id, trace_digest, annotator_id,
            annotator_digest, repeat_index, state, reservation_id, evidence_head_digest,
            verifier_result_digest, applied_count, abstained_count, rejected_count,
            cost_usd_micros, failure_reason, payload_json, created_at, updated_at
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?18)
         ON CONFLICT(job_id) DO UPDATE SET
            campaign_id=COALESCE(excluded.campaign_id, annotation_jobs.campaign_id),
            container_id=excluded.container_id,
            trace_id=COALESCE(excluded.trace_id, annotation_jobs.trace_id),
            trace_digest=excluded.trace_digest,
            annotator_id=excluded.annotator_id,
            annotator_digest=COALESCE(excluded.annotator_digest, annotation_jobs.annotator_digest),
            repeat_index=excluded.repeat_index,
            state=excluded.state,
            reservation_id=COALESCE(excluded.reservation_id, annotation_jobs.reservation_id),
            applied_count=excluded.applied_count,
            abstained_count=excluded.abstained_count,
            rejected_count=excluded.rejected_count,
            cost_usd_micros=COALESCE(excluded.cost_usd_micros, annotation_jobs.cost_usd_micros),
            failure_reason=excluded.failure_reason,
            payload_json=excluded.payload_json,
            updated_at=excluded.updated_at",
        params![
            job_id,
            campaign_id,
            container_id,
            trace_id,
            trace_digest,
            annotator_id,
            annotator_digest,
            repeat_index,
            state,
            reservation_id,
            if already_projected {
                bundle_digest.as_deref()
            } else {
                None
            },
            applied_count,
            abstained_count,
            rejected_count,
            cost_usd_micros,
            failure_reason,
            payload_json,
            now,
        ],
    )?;
    Ok(JobApplyOutcome {
        job_id,
        campaign_id: campaign_id.map(str::to_owned),
        state,
        terminal,
        trace_id,
        trace_digest,
        bundle_digest,
        already_projected,
    })
}

fn ensure_campaign_stub(conn: &Connection, campaign_id: &str, container_id: &str) -> Result<()> {
    let now = now_rfc3339();
    conn.execute(
        "INSERT OR IGNORE INTO annotation_campaigns(
            campaign_id, container_id, status, traces_json, annotators_json,
            coverage_json, cost_json, metadata_json, created_at, updated_at
         ) VALUES(?1, ?2, 'running', '[]', '[]', '{}', '{}', '{}', ?3, ?3)",
        params![campaign_id, container_id, now],
    )?;
    Ok(())
}

/// Register a deterministic local campaign for a sealed annotation head that
/// was produced by the container outside Workshop's campaign ledger (for
/// example, post-rollout annotation during recovery). This is projection
/// metadata only; it never starts or mutates annotation work.
pub fn ensure_import_campaign(
    conn: &Connection,
    campaign_id: &str,
    container_id: &str,
    trace_id: &str,
    session_id: Option<&str>,
) -> Result<()> {
    ensure_campaign_stub(conn, campaign_id, container_id)?;
    let now = now_rfc3339();
    conn.execute(
        "UPDATE annotation_campaigns SET
            session_id=COALESCE(session_id, ?2),
            label='Imported sealed annotations',
            status='sealed',
            traces_json=?3,
            metadata_json=?4,
            updated_at=?5
         WHERE campaign_id=?1",
        params![
            campaign_id,
            session_id,
            serde_json::to_string(&json!([{ "trace_id": trace_id }]))?,
            serde_json::to_string(&json!({
                "containerId": container_id,
                "traceId": trace_id,
                "sessionId": session_id,
                "source": "sealed_container_evidence_head"
            }))?,
            now,
        ],
    )?;
    Ok(())
}

pub fn mark_job_projected(
    conn: &Connection,
    job_id: &str,
    evidence_head_digest: Option<&str>,
) -> Result<()> {
    let mut payload: Value = conn
        .query_row(
            "SELECT payload_json FROM annotation_jobs WHERE job_id=?1",
            [job_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or(json!({}));
    if let Some(object) = payload.as_object_mut() {
        object.insert("projected".into(), json!(true));
    }
    conn.execute(
        "UPDATE annotation_jobs
         SET payload_json=?1, evidence_head_digest=COALESCE(?2, evidence_head_digest), updated_at=?3
         WHERE job_id=?4",
        params![
            serde_json::to_string(&payload)?,
            evidence_head_digest,
            now_rfc3339(),
            job_id
        ],
    )?;
    Ok(())
}

fn load_job(conn: &Connection, job_id: &str) -> Result<Option<JobRow>> {
    conn.query_row(
        "SELECT job_id, campaign_id, container_id, trace_id, trace_digest, annotator_id,
                state, evidence_head_digest, applied_count, abstained_count, rejected_count,
                failure_reason, payload_json
         FROM annotation_jobs WHERE job_id=?1",
        [job_id],
        job_from_row,
    )
    .optional()
    .context("load annotation job")
}

fn job_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<JobRow> {
    let payload_json: String = row.get(12)?;
    let payload: Value = serde_json::from_str(&payload_json).unwrap_or(Value::Null);
    Ok(JobRow {
        job_id: row.get(0)?,
        campaign_id: row.get(1)?,
        container_id: row.get(2)?,
        trace_id: row.get(3)?,
        trace_digest: row.get(4)?,
        annotator_id: row.get(5)?,
        state: row.get(6)?,
        evidence_head_digest: row.get(7)?,
        applied_count: row.get(8)?,
        abstained_count: row.get(9)?,
        rejected_count: row.get(10)?,
        failure_reason: row.get(11)?,
        projected: payload.get("projected").and_then(Value::as_bool).unwrap_or(false),
    })
}

pub fn list_jobs_for_campaign(conn: &Connection, campaign_id: &str) -> Result<Vec<JobRow>> {
    let mut statement = conn.prepare(
        "SELECT job_id, campaign_id, container_id, trace_id, trace_digest, annotator_id,
                state, evidence_head_digest, applied_count, abstained_count, rejected_count,
                failure_reason, payload_json
         FROM annotation_jobs WHERE campaign_id=?1 ORDER BY job_id",
    )?;
    let rows = statement
        .query_map([campaign_id], job_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn list_jobs_needing_reconcile(conn: &Connection) -> Result<Vec<JobRow>> {
    let mut statement = conn.prepare(
        "SELECT job_id, campaign_id, container_id, trace_id, trace_digest, annotator_id,
                state, evidence_head_digest, applied_count, abstained_count, rejected_count,
                failure_reason, payload_json
         FROM annotation_jobs
         WHERE container_id IS NOT NULL
           AND (
                state NOT IN ('sealed','abstained','failed','cancelled')
                OR COALESCE(json_extract(payload_json, '$.projected'), 0) != 1
           )
         ORDER BY updated_at, job_id",
    )?;
    let rows = statement
        .query_map([], job_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn campaign_row(
    conn: &Connection,
    campaign_id: &str,
) -> Result<Option<(Option<String>, Option<String>, Option<String>, String)>> {
    conn.query_row(
        "SELECT session_id, eval_run_id, label, status
         FROM annotation_campaigns WHERE campaign_id=?1",
        [campaign_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )
    .optional()
    .context("load annotation campaign")
}

pub fn refresh_campaign_coverage(conn: &Connection, campaign_id: &str) -> Result<String> {
    let jobs = list_jobs_for_campaign(conn, campaign_id)?;
    let states: Vec<String> = jobs.iter().map(|job| job.state.clone()).collect();
    let status = campaign_status_from_jobs(&states);
    let rejected: i64 = jobs.iter().filter_map(|job| job.rejected_count).sum();
    let applied: i64 = jobs.iter().filter_map(|job| job.applied_count).sum();
    let abstained_findings: i64 = jobs.iter().filter_map(|job| job.abstained_count).sum();
    let coverage = json!({
        "jobs": jobs.len(),
        "sealed": jobs.iter().filter(|job| job.state == "sealed").count(),
        "abstained": jobs.iter().filter(|job| job.state == "abstained").count(),
        "failed": jobs.iter().filter(|job| job.state == "failed").count(),
        "cancelled": jobs.iter().filter(|job| job.state == "cancelled").count(),
        "applied": applied,
        "abstainedFindings": abstained_findings,
        "rejected": rejected,
    });
    conn.execute(
        "UPDATE annotation_campaigns
         SET status=?1, coverage_json=?2, updated_at=?3
         WHERE campaign_id=?4",
        params![status, serde_json::to_string(&coverage)?, now_rfc3339(), campaign_id],
    )?;
    Ok(status.to_string())
}

/// Seed local campaign/job rows from a submitted annotation stage report.
/// Idempotent: existing polled state is never overwritten.
pub fn seed_from_stage_payload(
    conn: &Connection,
    eval_run_id: &str,
    label: Option<&str>,
    stage: &Value,
) -> Result<usize> {
    let Some(campaign_id) = text(stage, "campaign_id", "campaignId") else {
        return Ok(0);
    };
    let status = text(stage, "status", "status").unwrap_or("submitted");
    if status != "submitted" {
        return Ok(0);
    }
    let Some(container_id) = text(stage, "container_id", "containerId") else {
        return Ok(0);
    };
    let jobs = stage
        .get("jobs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut traces: Vec<Value> = Vec::new();
    let mut annotators: Vec<String> = Vec::new();
    for job in &jobs {
        let digest = text(job, "trace_digest", "traceDigest");
        let trace_id = text(job, "trace_id", "traceId");
        if let Some(digest) = digest {
            if !traces.iter().any(|row| row.get("digest").and_then(Value::as_str) == Some(digest)) {
                traces.push(json!({ "id": trace_id, "digest": digest }));
            }
        }
        if let Some(annotator) = text(job, "annotator_id", "annotatorId") {
            if !annotators.iter().any(|item| item == annotator) {
                annotators.push(annotator.to_string());
            }
        }
    }
    let session_id: Option<String> = conn
        .query_row(
            "SELECT session_ref FROM optimizer_runs WHERE id=?1",
            [eval_run_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    let label = label
        .or_else(|| text(stage, "label", "label"))
        .unwrap_or("post_rollout");
    let domain = annotators
        .first()
        .and_then(|id| domain_from_annotator(id))
        .map(str::to_owned);
    let metadata = json!({
        "containerId": container_id,
        "evalRunId": eval_run_id,
        "sessionId": session_id,
        "label": label,
        "domain": domain,
        "traces": traces,
        "annotators": annotators,
        "coverage": { "jobs": jobs.len(), "sealed": 0, "abstained": 0, "failed": 0 }
    });
    let now = now_rfc3339();
    conn.execute(
        "INSERT OR IGNORE INTO annotation_campaigns(
            campaign_id, container_id, eval_run_id, session_id, label, domain,
            status, traces_json, annotators_json, coverage_json, cost_json,
            metadata_json, created_at, updated_at
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, 'submitted', ?7, ?8, ?9, '{}', ?10, ?11, ?11)",
        params![
            campaign_id,
            container_id,
            eval_run_id,
            session_id,
            label,
            domain,
            serde_json::to_string(&traces)?,
            serde_json::to_string(&annotators)?,
            serde_json::to_string(&json!({ "jobs": jobs.len(), "sealed": 0 }))?,
            serde_json::to_string(&metadata)?,
            now,
        ],
    )?;
    let mut inserted = 0usize;
    for job in &jobs {
        let Some(job_id) = text(job, "job_id", "jobId") else {
            continue;
        };
        let Some(trace_digest) = text(job, "trace_digest", "traceDigest") else {
            continue;
        };
        let Some(annotator_id) = text(job, "annotator_id", "annotatorId") else {
            continue;
        };
        let changed = conn.execute(
            "INSERT OR IGNORE INTO annotation_jobs(
                job_id, campaign_id, container_id, trace_id, trace_digest, annotator_id,
                repeat_index, state, reservation_id, payload_json, created_at, updated_at
             ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, 'prepared', ?8, '{}', ?9, ?9)",
            params![
                job_id,
                campaign_id,
                container_id,
                text(job, "trace_id", "traceId"),
                trace_digest,
                annotator_id,
                i64_field(job, "repeat_index", "repeatIndex").unwrap_or(0),
                text(job, "reservation_id", "reservationId"),
                now,
            ],
        )?;
        inserted += changed as usize;
    }
    Ok(inserted)
}

pub fn seed_from_amendments(conn: &Connection) -> Result<usize> {
    let mut statement = conn.prepare(
        "SELECT optimizer_run_id, evidence_json
         FROM optimizer_evidence_amendments
         WHERE evidence_json LIKE '%annotationStage%'",
    )?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut seeded = 0usize;
    for (run_id, evidence_json) in rows {
        let Ok(event) = serde_json::from_str::<Value>(&evidence_json) else {
            continue;
        };
        let Some(stage) = event
            .pointer("/delta/annotationStage")
            .or_else(|| event.pointer("/delta/annotation_stage"))
            .cloned()
        else {
            continue;
        };
        let label = event
            .pointer("/raw/spec/label")
            .and_then(Value::as_str);
        seeded += seed_from_stage_payload(conn, &run_id, label, &stage)?;
    }
    Ok(seeded)
}

pub fn build_workbench_summary(
    campaign_id: &str,
    campaign_status: &str,
    campaign_label: Option<&str>,
    campaign_domain: Option<&str>,
    trace_id: &str,
    trace_digest: &str,
    annotations_payload: &Value,
    bundles_payload: &Value,
    jobs: &[JobRow],
) -> Value {
    let annotations = annotations_payload
        .get("annotations")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let head = head_bundle(bundles_payload, annotations_payload);
    let bundle_digest = head
        .get("bundle_digest")
        .or_else(|| head.get("bundleDigest"))
        .or_else(|| annotations_payload.get("bundle_digest"))
        .or_else(|| annotations_payload.get("bundleDigest"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let bundle_id = head
        .get("bundle_id")
        .or_else(|| head.get("bundleId"))
        .and_then(Value::as_str);
    let verifier_result_count = head
        .get("verifier_result_count")
        .or_else(|| head.get("verifierResultCount"))
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let findings: Vec<Value> = annotations.iter().filter_map(finding_from_annotation).collect();
    let mut taxonomy = std::collections::BTreeMap::<String, i64>::new();
    for finding in &findings {
        if let Some(label) = finding.get("label").and_then(Value::as_str) {
            *taxonomy.entry(label.to_string()).or_insert(0) += 1;
        }
    }
    let milestones: Vec<Value> = findings
        .iter()
        .filter(|finding| {
            finding
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind.contains("milestone"))
                || finding
                    .get("label")
                    .and_then(Value::as_str)
                    .is_some_and(|label| label.starts_with("milestone."))
        })
        .map(|finding| {
            let label = finding.get("label").and_then(Value::as_str).unwrap_or("milestone");
            let verified = label.contains("engine_verified")
                || finding
                    .pointer("/payload/engine_verified")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
            json!({
                "id": finding.get("id").cloned().unwrap_or(json!(label)),
                "label": label,
                "state": if verified { "verified" } else { "attempted" },
                "engineVerified": verified,
            })
        })
        .collect();
    let mut spans = Vec::new();
    let mut seen_spans = std::collections::BTreeSet::new();
    for finding in &findings {
        let Some(target) = finding.get("target") else {
            continue;
        };
        let id = target
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if id.is_empty() || !seen_spans.insert(id.clone()) {
            continue;
        }
        spans.push(json!({
            "id": id,
            "title": finding.get("summary").cloned().unwrap_or(json!(id)),
            "kind": target.get("kind").cloned().unwrap_or(json!("span")),
        }));
    }
    let job_rows: Vec<Value> = jobs
        .iter()
        .map(|job| {
            let mut row = json!({
                "id": job.job_id,
                "annotatorId": job.annotator_id,
                "state": job.state,
            });
            if let Some(reason) = &job.failure_reason {
                row["reason"] = json!(reason);
            }
            row
        })
        .collect();
    let abstentions: Vec<Value> = jobs
        .iter()
        .filter(|job| job.state == "abstained")
        .map(|job| {
            json!({
                "jobId": job.job_id,
                "reason": job.failure_reason.clone().unwrap_or_else(|| "abstained".into()),
            })
        })
        .collect();
    let rejected: Vec<Value> = findings
        .iter()
        .filter(|finding| finding.get("status").and_then(Value::as_str) == Some("rejected"))
        .map(|finding| {
            json!({
                "findingId": finding.get("id"),
                "reason": finding.get("summary").cloned().unwrap_or(json!("rejected")),
            })
        })
        .collect();
    let rubric = rubric_from_verifier_results(head.get("verifier_results").or_else(|| head.get("verifierResults")));
    json!({
        "schemaVersion": WORKBENCH_SCHEMA,
        "campaign": {
            "id": campaign_id,
            "status": campaign_status,
            "label": campaign_label,
            "domain": campaign_domain,
        },
        "coverage": {
            "jobs": jobs.len(),
            "sealed": jobs.iter().filter(|job| job.state == "sealed").count(),
            "abstained": jobs.iter().filter(|job| job.state == "abstained").count(),
            "failed": jobs.iter().filter(|job| job.state == "failed").count(),
            "applied": jobs.iter().filter_map(|job| job.applied_count).sum::<i64>(),
            "abstainedFindings": jobs.iter().filter_map(|job| job.abstained_count).sum::<i64>(),
            "rejected": jobs.iter().filter_map(|job| job.rejected_count).sum::<i64>(),
        },
        "validation": {
            "selectorsResolved": findings.len(),
            "unresolvedSelectors": 0,
            "validationFailures": 0
        },
        "traces": [{ "id": trace_id, "digest": trace_digest }],
        "evidenceHead": {
            "bundleId": bundle_id,
            "digest": bundle_digest,
            "annotationCount": findings.len(),
            "verifierResultCount": verifier_result_count,
        },
        "rubric": rubric,
        "findings": findings,
        "taxonomy": taxonomy
            .into_iter()
            .map(|(label, count)| json!({ "label": label, "count": count }))
            .collect::<Vec<_>>(),
        "milestones": milestones,
        "spans": spans,
        "jobs": job_rows,
        "audit": {
            "abstentions": abstentions,
            "rejected": rejected,
            "unresolvedSelectors": [],
            "consensus": []
        }
    })
}

fn head_bundle(bundles_payload: &Value, annotations_payload: &Value) -> Value {
    let bundles = bundles_payload
        .get("bundles")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    bundles
        .iter()
        .find(|bundle| bundle.get("is_head").or_else(|| bundle.get("isHead")).and_then(Value::as_bool) == Some(true))
        .cloned()
        .or_else(|| bundles.last().cloned())
        .unwrap_or_else(|| json!({
            "bundle_digest": annotations_payload.get("bundle_digest").cloned().unwrap_or(Value::Null),
            "verifier_result_count": 0,
            "verifier_results": [],
        }))
}

fn rubric_from_verifier_results(raw: Option<&Value>) -> Value {
    let results = raw.and_then(Value::as_array).cloned().unwrap_or_default();
    let selected = results.iter().find(|item| {
        item.get("criterion_results")
            .or_else(|| item.get("criterionResults"))
            .or_else(|| item.get("judgments"))
            .and_then(Value::as_array)
            .is_some_and(|rows| !rows.is_empty())
    });
    let Some(result) = selected else {
        return json!({
            "available": false,
            "reason": if results.is_empty() {
                "verifier_result_missing"
            } else {
                "verifier_result_empty"
            },
            "digest": Value::Null,
            "criteria": []
        });
    };
    let digest = text(result, "content_digest", "contentDigest").unwrap_or("");
    let criteria = result
        .get("criterion_results")
        .or_else(|| result.get("criterionResults"))
        .or_else(|| result.get("judgments"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    json!({
        "available": true,
        "digest": digest,
        "verifierResultId": text(result, "verifier_result_id", "verifierResultId"),
        "rubricId": text(result, "rubric_id", "rubricId"),
        "rubricDigest": text(result, "rubric_digest", "rubricDigest"),
        "score": result.get("score").cloned().unwrap_or(Value::Null),
        "passed": result.get("passed").cloned().unwrap_or(Value::Null),
        "verdict": result.get("verdict").cloned().unwrap_or(json!("")),
        "verificationStatus": text(result, "verification_status", "verificationStatus"),
        "passThreshold": result.get("pass_threshold").or_else(|| result.get("passThreshold")).cloned(),
        "criteria": criteria,
    })
}

fn finding_from_annotation(annotation: &Value) -> Option<Value> {
    let id = text(annotation, "annotation_id", "annotationId")?;
    let annotator_id = text(annotation, "annotator_id", "annotatorId").unwrap_or("unknown");
    let annotation_type = text(annotation, "annotation_type", "annotationType").unwrap_or("");
    let labels = annotation
        .get("labels")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let label = labels
        .first()
        .and_then(Value::as_str)
        .unwrap_or(annotation_type);
    let target = annotation.get("target").cloned().unwrap_or(Value::Null);
    let entity_id = text(&target, "entity_id", "entityId")
        .or_else(|| text(&target, "id", "id"))
        .unwrap_or("");
    let kind = text(&target, "kind", "kind").unwrap_or("span");
    let summary = text(annotation, "rationale", "rationale")
        .or_else(|| {
            annotation
                .get("payload")
                .and_then(|payload| text(payload, "summary", "summary"))
        })
        .unwrap_or("");
    Some(json!({
        "id": id,
        "annotatorId": annotator_id,
        "type": annotation_type,
        "label": label,
        "severity": annotation
            .get("payload")
            .and_then(|payload| text(payload, "severity", "severity"))
            .unwrap_or("info"),
        "status": finding_status(text(annotation, "status", "status")),
        "target": {
            "kind": kind,
            "id": entity_id,
            "selector": format!("{kind}:{entity_id}"),
        },
        "summary": summary,
        "payload": annotation.get("payload").cloned().unwrap_or(json!({})),
    }))
}

pub fn replace_findings(conn: &Connection, evidence_head_digest: &str, summary: &Value) -> Result<usize> {
    conn.execute(
        "DELETE FROM annotation_findings WHERE evidence_head_digest=?1",
        [evidence_head_digest],
    )?;
    let findings = summary
        .get("findings")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let now = now_rfc3339();
    let mut written = 0usize;
    for finding in findings {
        let Some(finding_id) = finding.get("id").and_then(Value::as_str) else {
            continue;
        };
        let annotator_id = finding
            .get("annotatorId")
            .or_else(|| finding.get("annotator_id"))
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let status = finding_status(finding.get("status").and_then(Value::as_str));
        let target = finding.get("target").cloned().unwrap_or(json!({}));
        let selector = target
            .get("selector")
            .and_then(Value::as_str)
            .unwrap_or("");
        conn.execute(
            "INSERT INTO annotation_findings(
                finding_id, evidence_head_digest, job_id, annotator_id, annotation_type,
                taxonomy_label, severity, status, target_selector, target_selector_json,
                evidence_selectors_json, payload_json, created_at
             ) VALUES(?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?8, ?9, '[]', ?10, ?11)
             ON CONFLICT(finding_id) DO UPDATE SET
                evidence_head_digest=excluded.evidence_head_digest,
                annotator_id=excluded.annotator_id,
                annotation_type=excluded.annotation_type,
                taxonomy_label=excluded.taxonomy_label,
                severity=excluded.severity,
                status=excluded.status,
                target_selector=excluded.target_selector,
                target_selector_json=excluded.target_selector_json,
                payload_json=excluded.payload_json",
            params![
                finding_id,
                evidence_head_digest,
                annotator_id,
                finding.get("type").and_then(Value::as_str),
                finding.get("label").and_then(Value::as_str),
                finding.get("severity").and_then(Value::as_str),
                status,
                selector,
                serde_json::to_string(&target)?,
                serde_json::to_string(finding.get("payload").unwrap_or(&json!({})))?,
                now,
            ],
        )?;
        written += 1;
    }
    Ok(written)
}

pub fn project_trace_head(
    conn: &Connection,
    campaign_id: &str,
    trace_id: &str,
    trace_digest: &str,
    annotations_payload: &Value,
    bundles_payload: &Value,
) -> Result<Option<ProjectedHead>> {
    let jobs = list_jobs_for_campaign(conn, campaign_id)?;
    let campaign_status = refresh_campaign_coverage(conn, campaign_id)?;
    let (session_id, eval_run_id, label, _) = campaign_row(conn, campaign_id)?.unwrap_or((None, None, None, campaign_status.clone()));
    let domain = jobs
        .first()
        .and_then(|job| domain_from_annotator(&job.annotator_id))
        .map(str::to_owned);
    let summary = build_workbench_summary(
        campaign_id,
        &campaign_status,
        label.as_deref(),
        domain.as_deref(),
        trace_id,
        trace_digest,
        annotations_payload,
        bundles_payload,
        &jobs,
    );
    let digest = summary
        .pointer("/evidenceHead/digest")
        .and_then(Value::as_str)
        .filter(|item| !item.is_empty())
        .map(str::to_owned);
    let Some(digest) = digest else {
        return Ok(None);
    };
    upsert_evidence_head(conn, &digest, trace_digest, &summary)?;
    replace_findings(conn, &digest, &summary)?;
    let rubric = summary.get("rubric").cloned().unwrap_or(json!({ "available": false }));
    if rubric.get("available").and_then(Value::as_bool) == Some(true) {
        let rubric_digest = rubric
            .get("digest")
            .and_then(Value::as_str)
            .filter(|item| !item.is_empty())
            .unwrap_or(&digest);
        upsert_available_rubric(conn, rubric_digest, &rubric, Some(&digest))?;
    } else {
        let rubric_reason = rubric
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("verifier_result_missing");
        upsert_unavailable_rubric(
            conn,
            &format!("unavailable:{digest}"),
            rubric_reason,
            Some(&digest),
        )?;
    }
    for job in jobs.iter().filter(|job| {
        job.trace_digest == trace_digest || job.trace_id.as_deref() == Some(trace_id)
    }) {
        mark_job_projected(conn, &job.job_id, Some(&digest))?;
    }
    Ok(Some(ProjectedHead {
        digest,
        trace_id: trace_id.to_string(),
        trace_digest: trace_digest.to_string(),
        container_id: jobs.first().and_then(|job| job.container_id.clone()),
        campaign_id: Some(campaign_id.to_string()),
        campaign_status,
        session_id,
        eval_run_id,
        rubric_digest: if rubric.get("available").and_then(Value::as_bool) == Some(true) {
            rubric
                .get("digest")
                .and_then(Value::as_str)
                .filter(|item| !item.is_empty())
                .map(str::to_owned)
        } else {
            None
        },
    }))
}

pub fn list_campaigns_for_eval(conn: &Connection, eval_run_id: &str) -> Result<Vec<Value>> {
    let mut statement = conn.prepare(
        "SELECT campaign_id, status, label, domain, coverage_json, container_id
         FROM annotation_campaigns WHERE eval_run_id=?1 ORDER BY updated_at DESC",
    )?;
    let rows = statement
        .query_map([eval_run_id], |row| {
            let coverage_json: String = row.get(4)?;
            Ok(json!({
                "campaignId": row.get::<_, String>(0)?,
                "status": row.get::<_, String>(1)?,
                "label": row.get::<_, Option<String>>(2)?,
                "domain": row.get::<_, Option<String>>(3)?,
                "coverage": serde_json::from_str::<Value>(&coverage_json).unwrap_or(json!({})),
                "containerId": row.get::<_, Option<String>>(5)?,
            }))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn list_findings_for_trace(conn: &Connection, trace_digest: &str) -> Result<Vec<Value>> {
    // The annotation container retains its own trace-content digest, whereas a
    // Workshop Trace V5 import has the archive digest.  The local import's
    // producerTraceId is the owner-bound identity that joins those namespaces;
    // use it only as a fallback after an exact evidence-head digest lookup.
    let (container_id, producer_trace_id) = conn
        .query_row(
            "SELECT container_id, metadata_json FROM traces WHERE digest=?1",
            [trace_digest],
            |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()?
        .map(|(container_id, metadata_json)| {
            let metadata: Value = serde_json::from_str(&metadata_json).unwrap_or(Value::Null);
            let producer_trace_id = metadata
                .get("producerTraceId")
                .or_else(|| metadata.get("rolloutId"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned);
            (container_id, producer_trace_id)
        })
        .unwrap_or((None, None));
    let mut statement = conn.prepare(
        "SELECT f.finding_id, f.annotator_id, f.annotation_type, f.taxonomy_label,
                f.severity, f.status, f.target_selector, f.target_selector_json,
                f.payload_json, h.digest, h.campaign_id
         FROM annotation_findings f
         JOIN annotation_evidence_heads h ON h.digest = f.evidence_head_digest
         WHERE h.trace_digest=?1
            OR (?2 IS NOT NULL AND h.campaign_id IN (
                SELECT j.campaign_id
                FROM annotation_jobs j
                WHERE j.trace_id=?2
                  AND (?3 IS NULL OR j.container_id=?3)
            ))
         ORDER BY f.created_at, f.finding_id",
    )?;
    let rows = statement
        .query_map((trace_digest, producer_trace_id, container_id), |row| {
            let target_json: String = row.get(7)?;
            let payload_json: String = row.get(8)?;
            Ok(json!({
                "id": row.get::<_, String>(0)?,
                "annotatorId": row.get::<_, String>(1)?,
                "type": row.get::<_, Option<String>>(2)?,
                "label": row.get::<_, Option<String>>(3)?,
                "severity": row.get::<_, Option<String>>(4)?,
                "status": row.get::<_, String>(5)?,
                "targetSelector": row.get::<_, Option<String>>(6)?,
                "target": serde_json::from_str::<Value>(&target_json).unwrap_or(json!({})),
                "payload": serde_json::from_str::<Value>(&payload_json).unwrap_or(json!({})),
                "evidenceHeadDigest": row.get::<_, String>(9)?,
                "campaignId": row.get::<_, Option<String>>(10)?,
            }))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn record_local_review(
    conn: &Connection,
    finding_id: &str,
    evidence_head_digest: &str,
    decision: &str,
    reviewer: &str,
    rationale: &str,
) -> Result<String> {
    let review_id = format!(
        "arev_{}",
        chrono::Utc::now().timestamp_millis()
    );
    conn.execute(
        "INSERT INTO annotation_reviews(
            review_id, finding_id, evidence_head_digest, decision, reviewer, rationale, created_at
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            review_id,
            finding_id,
            evidence_head_digest,
            decision,
            reviewer,
            rationale,
            now_rfc3339()
        ],
    )?;
    Ok(review_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::migrations::apply_migrations;

    #[test]
    fn missing_verifier_evidence_is_unavailable_not_zero() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        apply_migrations(&conn).unwrap();
        upsert_campaign(
            &conn,
            "acmp_1",
            "sealed",
            &json!({
                "containerId": "ctr_1",
                "evalRunId": "opt_eval_1",
                "domain": "craftax",
                "label": "post_rollout",
                "coverage": { "jobs": 1, "sealed": 1 }
            }),
        )
        .unwrap();
        upsert_evidence_head(
            &conn,
            "sha256:head",
            "sha256:trace",
            &json!({
                "schemaVersion": WORKBENCH_SCHEMA,
                "campaign": { "id": "acmp_1" },
                "findings": [{ "id": "ann_1", "label": "recovery.failure_not_detected" }],
                "rubric": { "available": false }
            }),
        )
        .unwrap();
        upsert_unavailable_rubric(&conn, "sha256:missing", "verifier_result_missing", Some("sha256:head"))
            .unwrap();
        let head = get_evidence_head(&conn, "sha256:head").unwrap().unwrap();
        assert_eq!(head.trace_digest, "sha256:trace");
        assert_eq!(head.campaign_id.as_deref(), Some("acmp_1"));
        assert_eq!(head.annotation_count, 1);
        assert_eq!(head.verifier_result_count, 0);
        assert!(head.bundle_id.is_none());
        let rubric = get_rubric_result(&conn, "sha256:missing").unwrap().unwrap();
        assert!(!rubric.available);
        assert_eq!(rubric.unavailable_reason.as_deref(), Some("verifier_result_missing"));
        assert_eq!(rubric.summary.get("available"), Some(&json!(false)));
        assert!(rubric.summary.get("score").is_none());
        let payload = projection_payload(&conn, "annotation_evidence_head", "sha256:head").unwrap();
        assert_eq!(payload["payload"]["schemaVersion"], json!(WORKBENCH_SCHEMA));
    }

    #[test]
    fn campaign_status_does_not_treat_a_mix_as_sealed() {
        assert_eq!(campaign_status_from_jobs(&[]), "submitted");
        assert_eq!(
            campaign_status_from_jobs(&["prepared".into(), "sealed".into()]),
            "running"
        );
        assert_eq!(
            campaign_status_from_jobs(&["sealed".into(), "sealed".into()]),
            "sealed"
        );
        assert_eq!(
            campaign_status_from_jobs(&["sealed".into(), "abstained".into()]),
            "partially_sealed"
        );
        assert_eq!(
            campaign_status_from_jobs(&["failed".into(), "failed".into()]),
            "failed"
        );
    }

    #[test]
    fn seed_and_job_snapshot_are_idempotent() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        apply_migrations(&conn).unwrap();
        let stage = json!({
            "status": "submitted",
            "containerId": "ctr_1",
            "campaignId": "acmp_1",
            "jobs": [
                {
                    "job_id": "ajob_1",
                    "trace_id": "trace_1",
                    "trace_digest": "sha256:trace",
                    "annotator_id": "craftax.recovery_facts"
                }
            ]
        });
        assert_eq!(seed_from_stage_payload(&conn, "opt_eval_1", Some("post_rollout"), &stage).unwrap(), 1);
        assert_eq!(seed_from_stage_payload(&conn, "opt_eval_1", Some("post_rollout"), &stage).unwrap(), 0);
        let jobs = list_jobs_needing_reconcile(&conn).unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].state, "prepared");
        let payload = json!({
            "terminal": true,
            "job": {
                "job_id": "ajob_1",
                "state": "sealed",
                "bundle_digest": "sha256:head",
                "applied_count": 2,
                "abstained_count": 0,
                "rejected_count": 0,
                "request": {
                    "source_trace_id": "trace_1",
                    "source_trace_digest": "sha256:trace",
                    "annotator_id": "craftax.recovery_facts"
                }
            }
        });
        let first = apply_job_snapshot(&conn, Some("acmp_1"), "ctr_1", &payload).unwrap();
        assert!(first.terminal, "Workshop polls GET /annotation-jobs until terminal");
        assert!(!first.already_projected);
        assert_eq!(first.bundle_digest.as_deref(), Some("sha256:head"));
        let second = apply_job_snapshot(&conn, Some("acmp_1"), "ctr_1", &payload).unwrap();
        assert!(!second.already_projected, "not projected until the evidence head is written");
        let annotations = json!({
            "trace_id": "trace_1",
            "bundle_digest": "sha256:head",
            "annotations": [
                {
                    "annotation_id": "ann_1",
                    "annotator_id": "craftax.recovery_facts",
                    "annotation_type": "recovery",
                    "labels": ["recovery.failure_not_detected"],
                    "status": "applied",
                    "rationale": "Died without naming the block.",
                    "target": { "kind": "span", "entity_id": "span_42" },
                    "payload": { "severity": "high" }
                },
                {
                    "annotation_id": "ann_ms",
                    "annotator_id": "craftax.milestone_progress",
                    "annotation_type": "milestone",
                    "labels": ["milestone.engine_verified"],
                    "status": "applied",
                    "rationale": "COLLECT_WOOD",
                    "target": { "kind": "event", "entity_id": "evt_wood" },
                    "payload": { "engine_verified": true }
                }
            ]
        });
        let bundles = json!({
            "bundles": [{
                "bundle_id": "evb_1",
                "bundle_digest": "sha256:head",
                "is_head": true,
                "annotation_count": 2,
                "verifier_result_count": 0
            }]
        });
        let projected = project_trace_head(
            &conn,
            "acmp_1",
            "trace_1",
            "sha256:trace",
            &annotations,
            &bundles,
        )
        .unwrap()
        .expect("head digest");
        assert_eq!(projected.digest, "sha256:head");
        assert_eq!(projected.campaign_status, "sealed");
        let again = project_trace_head(
            &conn,
            "acmp_1",
            "trace_1",
            "sha256:trace",
            &annotations,
            &bundles,
        )
        .unwrap()
        .expect("head digest");
        assert_eq!(again.digest, "sha256:head");
        let finding_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM annotation_findings WHERE evidence_head_digest='sha256:head'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(finding_count, 2, "re-projecting must replace, not duplicate");
        let rubric = get_rubric_result(&conn, "unavailable:sha256:head").unwrap().unwrap();
        assert!(!rubric.available);
        assert_eq!(rubric.unavailable_reason.as_deref(), Some("verifier_result_missing"));
        assert!(rubric.summary.get("score").is_none());
        let third = apply_job_snapshot(&conn, Some("acmp_1"), "ctr_1", &payload).unwrap();
        assert!(third.already_projected);
        assert!(list_jobs_needing_reconcile(&conn).unwrap().is_empty());
        let head = get_evidence_head(&conn, "sha256:head").unwrap().unwrap();
        assert_eq!(head.summary["findings"].as_array().unwrap().len(), 2);
        assert_eq!(head.summary["milestones"].as_array().unwrap().len(), 1);
        assert_eq!(head.summary["rubric"]["available"], json!(false));
        let campaigns = list_campaigns_for_eval(&conn, "opt_eval_1").unwrap();
        assert_eq!(campaigns.len(), 1);
        assert_eq!(campaigns[0]["campaignId"], json!("acmp_1"));
        assert_eq!(campaigns[0]["status"], json!("sealed"));
        assert_eq!(campaigns[0]["coverage"]["sealed"], json!(1));
        let findings = list_findings_for_trace(&conn, "sha256:trace").unwrap();
        assert_eq!(findings.len(), 2);
        assert_eq!(findings[0]["id"], json!("ann_1"));
        let review_id = record_local_review(
            &conn,
            "ann_1",
            "sha256:head",
            "flag",
            "workshop",
            "check this span",
        )
        .unwrap();
        assert!(review_id.starts_with("arev_"));
    }

    #[test]
    fn restart_resumes_running_jobs_without_a_second_reservation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("session.sqlite");
        let idempotency = "ik_campaign_restart_1";
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            apply_migrations(&conn).unwrap();
            let stage = json!({
                "status": "submitted",
                "containerId": "ctr_1",
                "campaignId": "acmp_restart",
                "jobs": [
                    {
                        "job_id": "ajob_restart",
                        "trace_id": "trace_1",
                        "trace_digest": "sha256:trace",
                        "annotator_id": "craftax.recovery_facts"
                    }
                ]
            });
            assert_eq!(
                seed_from_stage_payload(&conn, "opt_eval_restart", Some("post_rollout"), &stage)
                    .unwrap(),
                1
            );
            let running = json!({
                "terminal": false,
                "job": {
                    "job_id": "ajob_restart",
                    "state": "running",
                    "reservation_id": "rsv_restart",
                    "request": {
                        "source_trace_id": "trace_1",
                        "source_trace_digest": "sha256:trace",
                        "annotator_id": "craftax.recovery_facts",
                        "idempotency_key": idempotency
                    }
                }
            });
            let applied = apply_job_snapshot(&conn, Some("acmp_restart"), "ctr_1", &running).unwrap();
            assert!(!applied.terminal);
            assert_eq!(applied.state, "running");
            let jobs = list_jobs_needing_reconcile(&conn).unwrap();
            assert_eq!(jobs.len(), 1);
            assert_eq!(jobs[0].state, "running");
        }
        let conn = rusqlite::Connection::open(&path).unwrap();
        let jobs = list_jobs_needing_reconcile(&conn).unwrap();
        assert_eq!(jobs.len(), 1, "Workshop restart re-reads running jobs from SQLite");
        assert_eq!(jobs[0].job_id, "ajob_restart");
        assert_eq!(jobs[0].state, "running");
        let sealed = json!({
            "terminal": true,
            "job": {
                "job_id": "ajob_restart",
                "state": "sealed",
                "bundle_digest": "sha256:head",
                "reservation_id": "rsv_restart",
                "applied_count": 1,
                "abstained_count": 0,
                "rejected_count": 0,
                "request": {
                    "source_trace_id": "trace_1",
                    "source_trace_digest": "sha256:trace",
                    "annotator_id": "craftax.recovery_facts",
                    "idempotency_key": idempotency
                }
            }
        });
        let outcome = apply_job_snapshot(&conn, Some("acmp_restart"), "ctr_1", &sealed).unwrap();
        assert!(outcome.terminal);
        assert_eq!(outcome.state, "sealed");
        let job_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM annotation_jobs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(job_count, 1, "reconciler must not insert a second job");
        let (payload_json, reservation_id): (String, Option<String>) = conn
            .query_row(
                "SELECT payload_json, reservation_id FROM annotation_jobs WHERE job_id='ajob_restart'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        let payload: Value = serde_json::from_str(&payload_json).unwrap();
        assert_eq!(
            payload["job"]["request"]["idempotency_key"],
            json!(idempotency),
            "idempotency key is unchanged across restart"
        );
        assert_eq!(reservation_id.as_deref(), Some("rsv_restart"));
        let distinct: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT reservation_id) FROM annotation_jobs WHERE reservation_id IS NOT NULL",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(distinct, 1, "no second reservation is minted on resume");
    }

    #[test]
    fn verifier_result_summaries_project_as_available_scorecards() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        apply_migrations(&conn).unwrap();
        let stage = json!({
            "status": "submitted",
            "containerId": "ctr_1",
            "campaignId": "acmp_vr",
            "jobs": [{
                "job_id": "ajob_vr",
                "trace_id": "trace_1",
                "trace_digest": "sha256:trace",
                "annotator_id": "craftax.rubric_verifier"
            }]
        });
        seed_from_stage_payload(&conn, "opt_eval_vr", Some("post_rollout"), &stage).unwrap();
        apply_job_snapshot(
            &conn,
            Some("acmp_vr"),
            "ctr_1",
            &json!({
                "terminal": true,
                "job": {
                    "job_id": "ajob_vr",
                    "state": "sealed",
                    "bundle_digest": "sha256:head-vr",
                    "request": {
                        "source_trace_id": "trace_1",
                        "source_trace_digest": "sha256:trace",
                        "annotator_id": "craftax.rubric_verifier"
                    }
                }
            }),
        )
        .unwrap();
        let projected = project_trace_head(
            &conn,
            "acmp_vr",
            "trace_1",
            "sha256:trace",
            &json!({ "trace_id": "trace_1", "bundle_digest": "sha256:head-vr", "annotations": [] }),
            &json!({
                "bundles": [{
                    "bundle_id": "evb_vr",
                    "bundle_digest": "sha256:head-vr",
                    "is_head": true,
                    "annotation_count": 0,
                    "verifier_result_count": 1,
                    "verifier_results": [{
                        "verifier_result_id": "vres_1",
                        "content_digest": "sha256:vres",
                        "rubric_id": "craftax.execution_quality",
                        "score": 0.625,
                        "passed": true,
                        "verdict": "pass",
                        "verification_status": "valid",
                        "criterion_results": [{
                            "criterion_id": "grounding",
                            "score": 2.0,
                            "verdict": "pass",
                            "passed": true
                        }]
                    }]
                }]
            }),
        )
        .unwrap()
        .expect("head digest");
        assert_eq!(projected.digest, "sha256:head-vr");
        let head = get_evidence_head(&conn, "sha256:head-vr").unwrap().unwrap();
        assert_eq!(head.summary["rubric"]["available"], json!(true));
        assert_eq!(head.summary["rubric"]["score"], json!(0.625));
        let rubric = get_rubric_result(&conn, "sha256:vres").unwrap().unwrap();
        assert!(rubric.available);
        assert_eq!(rubric.summary.get("score"), Some(&json!(0.625)));
    }

    #[test]
    fn luna_verifier_result_projects_criterion_scorecard() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        apply_migrations(&conn).unwrap();
        let stage = json!({
            "status": "submitted",
            "containerId": "ctr_18082",
            "campaignId": "acmp_luna",
            "jobs": [{
                "job_id": "ajob_a0d742cc7ce34e3a",
                "trace_id": "roll_ab9de205861d",
                "trace_digest": "sha256:6e47e52c30126d3f360d74576569a7358230b0255d646e8465a57693fdb48d69",
                "annotator_id": "craftax.rubric_verifier"
            }]
        });
        seed_from_stage_payload(&conn, "opt_eval_luna", Some("post_rollout"), &stage).unwrap();
        apply_job_snapshot(
            &conn,
            Some("acmp_luna"),
            "ctr_18082",
            &json!({
                "terminal": true,
                "job": {
                    "job_id": "ajob_a0d742cc7ce34e3a",
                    "state": "sealed",
                    "bundle_digest": "sha256:668f85a4825bf96cb25476eecd96736e5f6cb430948fac18134b5207b780db7e",
                    "request": {
                        "source_trace_id": "roll_ab9de205861d",
                        "source_trace_digest": "sha256:6e47e52c30126d3f360d74576569a7358230b0255d646e8465a57693fdb48d69",
                        "annotator_id": "craftax.rubric_verifier"
                    }
                }
            }),
        )
        .unwrap();
        let projected = project_trace_head(
            &conn,
            "acmp_luna",
            "roll_ab9de205861d",
            "sha256:6e47e52c30126d3f360d74576569a7358230b0255d646e8465a57693fdb48d69",
            &json!({
                "trace_id": "roll_ab9de205861d",
                "bundle_digest": "sha256:668f85a4825bf96cb25476eecd96736e5f6cb430948fac18134b5207b780db7e",
                "annotations": []
            }),
            &json!({
                "bundles": [{
                    "bundle_id": "evb_de36e1a0ba95d45b",
                    "bundle_digest": "sha256:668f85a4825bf96cb25476eecd96736e5f6cb430948fac18134b5207b780db7e",
                    "is_head": true,
                    "annotation_count": 0,
                    "verifier_result_count": 1,
                    "verifier_results": [{
                        "verifier_result_id": "vres_2a294bcd197fd15c",
                        "content_digest": "sha256:f3b1f77bfb50067ac860cf551277fd038c73ac9fa3d8379ea6b57dddea3fe56d",
                        "verifier_id": "craftax.rubric_verifier.verifier",
                        "rubric_id": "craftax.execution_quality",
                        "rubric_digest": "sha256:6f6ffade02247deb3614c1f693dc37507aff4cccd4ead17bbfe17350bac815e1",
                        "score": 0.4722222222222222,
                        "passed": false,
                        "verdict": "fail",
                        "verification_status": "valid",
                        "pass_threshold": 0.5,
                        "criterion_results": [
                            {
                                "criterion_id": "state_grounding",
                                "score": 2.0,
                                "verdict": "pass",
                                "passed": true,
                                "status": "decisive",
                                "rationale": "The policy correctly identified the initial map."
                            },
                            {
                                "criterion_id": "belief_calibration",
                                "score": 3.0,
                                "verdict": "pass",
                                "passed": true,
                                "status": "decisive"
                            }
                        ]
                    }]
                }]
            }),
        )
        .unwrap()
        .expect("head digest");
        let head = get_evidence_head(&conn, &projected.digest).unwrap().unwrap();
        assert_eq!(head.summary["rubric"]["available"], json!(true));
        assert_eq!(head.summary["rubric"]["score"], json!(0.4722222222222222));
        assert_eq!(head.summary["rubric"]["passed"], json!(false));
        assert_eq!(
            head.summary["rubric"]["verifierResultId"],
            json!("vres_2a294bcd197fd15c")
        );
        assert_eq!(head.summary["rubric"]["criteria"].as_array().unwrap().len(), 2);
        let rubric = get_rubric_result(
            &conn,
            "sha256:f3b1f77bfb50067ac860cf551277fd038c73ac9fa3d8379ea6b57dddea3fe56d",
        )
        .unwrap()
        .unwrap();
        assert!(rubric.available);
        assert_eq!(rubric.summary.get("score"), Some(&json!(0.4722222222222222)));
        assert_ne!(rubric.summary.get("score"), Some(&json!(0.0)));
    }

    #[test]
    fn seed_from_amendments_reads_camel_case_stage_reports() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        apply_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO optimizer_runs(
                id, algorithm_id, status, source, created_at, payload_json, updated_at, session_ref
             ) VALUES('opt_eval_1','eval','completed','container','now','{}','now','sess_1')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO optimizer_evidence_amendments(
                amendment_id, optimizer_run_id, terminal_sequence, evidence_json, recorded_at
             ) VALUES('amd_1','opt_eval_1',4,?1,'now')",
            [json!({
                "delta": {
                    "annotationStage": {
                        "status": "submitted",
                        "containerId": "ctr_1",
                        "campaignId": "acmp_amd",
                        "jobs": [{
                            "job_id": "ajob_amd",
                            "trace_digest": "sha256:amd",
                            "annotator_id": "craftax.belief_facts"
                        }]
                    }
                },
                "raw": { "spec": { "label": "post_rollout" } }
            })
            .to_string()],
        )
        .unwrap();
        assert_eq!(seed_from_amendments(&conn).unwrap(), 1);
        let session: Option<String> = conn
            .query_row(
                "SELECT session_id FROM annotation_campaigns WHERE campaign_id='acmp_amd'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(session.as_deref(), Some("sess_1"));
        assert_eq!(seed_from_amendments(&conn).unwrap(), 0);
    }
}
