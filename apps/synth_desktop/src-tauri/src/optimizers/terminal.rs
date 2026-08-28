//! The terminal manifest: one write-once record of how a run ended.
//!
//! Before this existed, "how did the run end?" had six answers — the run row's
//! `status`, its `summary` JSON, the event stream, the cached state slices, the
//! progress card's own reduction, and whatever `get_result` could scrape off
//! disk — and they could all disagree. A late poll could overwrite a settled
//! record with an older cursor's view of it, and a run whose evidence never
//! persisted still read as a clean success.
//!
//! A manifest is sealed exactly once, inside the same transaction that appends
//! the terminal event, at the cursor that event advanced to. Later writes are
//! refused, not merged: a second attempt returns the sealed record. Every
//! terminal surface — `get_result`, the MCP poll, the progress card's frozen
//! numbers, restart recovery — reconciles against this one row.
//!
//! Values that were never measured stay `null`. A manifest that reports `0`
//! where nothing was reported would be the same lie the progress card was
//! telling with "0 trials".

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Map, Value};

use super::gepa_evidence;
use super::models::{OptimizerEventEnvelope, OptimizerRunRecord, OptimizerUsageSummary};

pub(super) const TERMINAL_MANIFEST_SCHEMA: &str = "optimizer_terminal_manifest.v1";
const TERMINAL_MANIFEST_SCHEMA_V2: &str = "optimizer_terminal_manifest.v2";

/// Terminal status spellings the manifest may carry. `failed_evidence` is the
/// one that did not exist before: compute succeeded, its evidence did not, and
/// calling that "completed" is what hid the Banking77 loss.

/// Work counts, in the unit the algorithm actually plans in. Every field is
/// `Option` because a producer that never declared a plan has not declared zero.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct WorkCounts {
    pub planned: Option<u64>,
    pub succeeded: Option<u64>,
    pub failed: Option<u64>,
    pub cancelled: Option<u64>,
    pub skipped: Option<u64>,
    pub unit: Option<&'static str>,
}

impl WorkCounts {
    fn to_value(&self) -> Value {
        json!({
            "planned": self.planned,
            "succeeded": self.succeeded,
            "failed": self.failed,
            "cancelled": self.cancelled,
            "skipped": self.skipped,
            "unit": self.unit,
        })
    }
}

/// Count the terminal work an event history actually proves, per algorithm.
///
/// This reads events, never the run summary: the summary is a projection a
/// worker wrote, and the whole point of the manifest is to be answerable to the
/// durable log. When the log proves nothing, the counts stay `None` and the
/// renderer says so.
pub(super) fn work_counts(
    run: &OptimizerRunRecord,
    events: &[OptimizerEventEnvelope],
) -> WorkCounts {
    if let Ok(algorithm) = crate::optimizers::kernel::AlgorithmKind::parse_wire(&run.algorithm_id) {
        let placement =
            crate::optimizers::kernel::bridge::placement_from_run_source(algorithm, &run.source);
        if let Ok(state) = crate::optimizers::kernel::bridge::reduce_envelopes(
            &run.id,
            algorithm,
            placement,
            run.id.as_str(),
            events,
        ) {
            return kernel_work_counts(&state.work_summary());
        }
    }
    WorkCounts::default()
}

fn kernel_work_counts(summary: &crate::optimizers::kernel::WorkSummary) -> WorkCounts {
    let unit = match summary.unit.as_deref() {
        Some("trials") => Some("trials"),
        Some("rollouts") => Some("rollouts"),
        Some("child_evals") => Some("child_evals"),
        Some("checkpoint_evals") => Some("checkpoint_evals"),
        Some("rollout_groups") => Some("rollout_groups"),
        _ => None,
    };
    WorkCounts {
        planned: summary.planned,
        succeeded: summary.succeeded,
        failed: summary.failed,
        cancelled: summary.cancelled,
        skipped: if summary.fixed_denominator {
            summary.planned.map(|planned| {
                planned.saturating_sub(
                    summary.succeeded.unwrap_or(0)
                        + summary.failed.unwrap_or(0)
                        + summary.cancelled.unwrap_or(0),
                )
            })
        } else {
            None
        },
        unit,
    }
}

/// Usage as the manifest records it: lanes preserved, unknowns preserved.
fn usage_value(run: &OptimizerRunRecord) -> Value {
    // The durable run row is the canonical usage accumulator. Event deltas
    // feed it; they are not a second terminal authority to re-sum here.
    let lanes = run
        .summary
        .get("usageLanes")
        .cloned()
        .unwrap_or(Value::Null);
    json!({
        "costUsd": run.usage.cost_usd,
        "promptTokens": run.usage.prompt_tokens,
        "completionTokens": run.usage.completion_tokens,
        "rollouts": run.usage.rollouts,
        "wallTimeMs": run.usage.wall_time_ms,
        // Policy and grader/scorer telemetry are different money and different
        // tokens. Collapsing them into one total is how a grader-heavy recipe
        // starts reading as a cheap policy.
        "lanes": lanes,
        "costComplete": run
            .usage
            .extra
            .get("costTelemetryComplete")
            .and_then(Value::as_bool),
    })
}

fn selection_value(run: &OptimizerRunRecord, events: &[OptimizerEventEnvelope]) -> Value {
    events
        .iter()
        .rev()
        .find_map(|event| {
            event
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.get("selection"))
                .cloned()
        })
        .or_else(|| run.summary.get("selection").cloned())
        .unwrap_or(Value::Null)
}

/// GEPA's selection, stated as a decision rather than as a winner.
///
/// The producer never writes a `selection` snapshot, so every GEPA manifest
/// sealed with `selection: null`. Filling that with the frontier's best
/// candidate alone would be worse than null: the frontier's best is frequently
/// the seed, and presenting the seed as the selected candidate is how a search
/// that improved nothing came to read as a promotion. The three identities stay
/// separate here — what was proposed, what the optimizer selected, and what may
/// be deployed — and the verdict carries the evidence for the third.
fn gepa_selection(evidence: &gepa_evidence::GepaEvidence, run_status: &str) -> Value {
    let (verdict, detail) = evidence.verdict(run_status);
    json!({
        "seedCandidateId": evidence.seed_candidate_id,
        "selectedCandidateId": evidence.selected_candidate_id,
        "accepted": evidence
            .selected_candidate_id
            .as_deref()
            .zip(evidence.seed_candidate_id.as_deref())
            .map(|(selected, seed)| selected != seed),
        "verdict": verdict.as_str(),
        "verdictDetail": detail,
    })
}

/// Build the manifest for a run that has just reached `terminal_status` at
/// `terminal_cursor`. Pure: the caller seals it.
pub(super) fn derive(
    run: &OptimizerRunRecord,
    events: &[OptimizerEventEnvelope],
    terminal_status: &str,
    degradation: Option<Value>,
) -> Value {
    let counts = work_counts(run, events);
    // GEPA is the one algorithm whose settlement is a *judgement*, not a count,
    // so its manifest carries the whole reduction: candidate lineage, per-stage
    // scores with sample counts, proposal accounting, and the verdict those
    // rest on. Every other surface reads it from here instead of re-deriving a
    // second opinion.
    let gepa = matches!(run.algorithm_id.as_str(), "gepa" | "go-ex")
        .then(|| gepa_evidence::reduce(run, events));
    let mut usage = usage_value(run);
    // The producer's own per-lane roll-up is the only place proposer spend is
    // separated from policy spend. Without it a search that burned a frontier
    // model on ten proposals reads as a cheap nano-model run.
    if let (Some(lanes), Some(object)) = (
        gepa.as_ref().and_then(|gepa| gepa.usage_lanes.clone()),
        usage.as_object_mut(),
    ) {
        if object.get("lanes").map(Value::is_null).unwrap_or(true) {
            object.insert("lanes".into(), lanes);
        }
    }
    let artifact_refs: Vec<Value> = run
        .output_refs
        .iter()
        .chain(run.visual_refs.iter())
        .map(|reference| {
            json!({
                "kind": reference.kind,
                "id": reference.id,
                "role": reference.role,
                "title": reference.title,
            })
        })
        .collect();
    json!({
        "schemaVersion": TERMINAL_MANIFEST_SCHEMA,
        "optimizerRunId": run.id,
        "workflowId": run
            .summary
            .get("workflowId")
            .and_then(Value::as_str)
            .unwrap_or(run.id.as_str()),
        "algorithmId": run.algorithm_id,
        "algorithmVersion": run.algorithm_version,
        "recipeId": run.summary.get("recipeId").cloned().unwrap_or(Value::Null),
        "sessionRef": run.session_ref,
        "terminalStatus": terminal_status,
        "terminalCursor": run.cursor_seq,
        "work": counts.to_value(),
        "usage": usage,
        "paidComputeApproval": run.usage.extra.get("paidComputeApproval").cloned().unwrap_or(Value::Null),
        "selection": match gepa.as_ref() {
            Some(gepa) => gepa_selection(gepa, terminal_status),
            None => selection_value(run, events),
        },
        "gepaEvidence": gepa
            .as_ref()
            .map(|gepa| gepa.to_value(terminal_status))
            .unwrap_or(Value::Null),
        "resultRefs": run
            .output_refs
            .iter()
            .filter(|reference| reference.kind == "result")
            .map(|reference| json!({ "id": reference.id, "role": reference.role }))
            .collect::<Vec<_>>(),
        "artifactRefs": artifact_refs,
        "degradation": degradation.unwrap_or(Value::Null),
        "credentialChain": run.summary.get("credentialChain").cloned().unwrap_or(Value::Null),
        "startedAt": run.started_at,
        "finishedAt": run.finished_at,
        "error": run.error.clone().unwrap_or(Value::Null),
    })
}

/// Seal a manifest. Write-once: if one is already durable, the durable one is
/// returned unchanged and the caller's is discarded. A later poll carrying an
/// older cursor can therefore never replace a settled record.
pub(super) fn seal(conn: &Connection, run_id: &str, manifest: &Value) -> Result<Value> {
    if let Some(existing) = load(conn, run_id)? {
        return Ok(existing);
    }
    let mut manifest = manifest.clone();
    populate_canonical_usage(conn, run_id, &mut manifest)?;
    let envelope = validate_manifest(run_id, &manifest)?;
    // Write-side closed-world assertion: a terminal receipt claiming children
    // are still running or queued describes a run that has not actually ended.
    // Pre-existing sealed manifests are tolerated on read (see `load`); a new
    // seal is not.
    if let Some(open) = open_terminal_work(&manifest) {
        anyhow::bail!(
            "refusing to seal an open-world terminal manifest for {run_id}: {open}"
        );
    }
    conn.execute(
        "INSERT INTO optimizer_terminal_manifests(
            optimizer_run_id, schema_version, algorithm_id, terminal_status,
            terminal_cursor, sealed_at, payload_json
         ) VALUES (?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(optimizer_run_id) DO NOTHING",
        params![
            run_id,
            envelope.schema_version,
            envelope.algorithm_id,
            envelope.terminal_status,
            i64::try_from(envelope.terminal_cursor)
                .context("optimizer terminal cursor exceeds SQLite integer range")?,
            chrono::Utc::now().to_rfc3339(),
            serde_json::to_string(&manifest)?,
        ],
    )
    .context("seal optimizer terminal manifest")?;
    load(conn, run_id)?.context("terminal manifest disappeared immediately after sealing")
}

pub(crate) fn load(conn: &Connection, run_id: &str) -> Result<Option<Value>> {
    let row: Option<(String, String, String, i64, String)> = conn
        .query_row(
            "SELECT schema_version, algorithm_id, terminal_status, terminal_cursor, payload_json
             FROM optimizer_terminal_manifests WHERE optimizer_run_id = ?1",
            params![run_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            },
        )
        .optional()?;
    let Some((schema_version, algorithm_id, terminal_status, terminal_cursor, payload)) = row
    else {
        return Ok(None);
    };
    let payload: Value =
        serde_json::from_str(&payload).context("decode optimizer terminal manifest payload")?;
    let envelope = validate_manifest(run_id, &payload)?;
    // Tolerate-or-migrate decision for legacy seals: tolerate on read, loudly.
    // Manifests sealed before the closed-world write guard may carry
    // running/queued counts; refusing them would make those runs unloadable
    // (the incident this warns about), so they load with an operator-visible
    // warning instead.
    if let Some(open) = open_terminal_work(&payload) {
        crate::platform::logging::report(
            "optimizers",
            "eprintln",
            format!(
                "optimizer run {run_id} has a legacy open-world terminal manifest ({open}); \
                 loading it as sealed — its work counts predate reducer-level closure"
            ),
        );
    }
    let terminal_cursor = u64::try_from(terminal_cursor)
        .context("optimizer terminal manifest has a negative cursor")?;
    if schema_version != envelope.schema_version
        || algorithm_id != envelope.algorithm_id
        || terminal_status != envelope.terminal_status
        || terminal_cursor != envelope.terminal_cursor
    {
        anyhow::bail!("optimizer terminal manifest {run_id} envelope disagrees with its payload");
    }
    Ok(Some(payload))
}

/// Nonterminal work a terminal manifest still claims, if any.
///
/// Counts that are absent make no claim; only explicit `running`/`queued`
/// (v2 manifests freeze the kernel `WorkSummary` verbatim) above zero mark a
/// manifest open-world.
fn open_terminal_work(manifest: &Value) -> Option<String> {
    let work = manifest.get("work")?.as_object()?;
    let open: Vec<String> = ["running", "queued"]
        .iter()
        .filter_map(|key| {
            let count = work.get(*key).and_then(Value::as_u64)?;
            (count > 0).then(|| format!("work.{key}={count}"))
        })
        .collect();
    (!open.is_empty()).then(|| open.join(", "))
}

struct ManifestEnvelope<'a> {
    schema_version: &'a str,
    algorithm_id: &'a str,
    terminal_status: &'a str,
    terminal_cursor: u64,
}

fn validate_manifest<'a>(run_id: &str, manifest: &'a Value) -> Result<ManifestEnvelope<'a>> {
    let object = manifest
        .as_object()
        .context("optimizer terminal manifest must be an object")?;
    let required_string = |key: &str| -> Result<&'a str> {
        object
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .with_context(|| format!("optimizer terminal manifest is missing typed {key}"))
    };
    let schema_version = required_string("schemaVersion")?;
    if !matches!(
        schema_version,
        TERMINAL_MANIFEST_SCHEMA | TERMINAL_MANIFEST_SCHEMA_V2
    ) {
        anyhow::bail!("unsupported optimizer terminal manifest schema {schema_version:?}");
    }
    let payload_run_id = required_string("optimizerRunId")?;
    if payload_run_id != run_id {
        anyhow::bail!("optimizer terminal manifest belongs to {payload_run_id}, not {run_id}");
    }
    let algorithm_id = required_string("algorithmId")?;
    let terminal_cursor = object
        .get("terminalCursor")
        .and_then(Value::as_u64)
        .context("optimizer terminal manifest is missing typed terminalCursor")?;
    let terminal_status = match schema_version {
        TERMINAL_MANIFEST_SCHEMA => required_string("terminalStatus")?,
        TERMINAL_MANIFEST_SCHEMA_V2 => object
            .get("terminal")
            .and_then(Value::as_object)
            .and_then(|terminal| terminal.get("kind"))
            .and_then(Value::as_str)
            .filter(|kind| matches!(*kind, "completed" | "failed" | "cancelled" | "degraded"))
            .context("optimizer terminal manifest v2 is missing typed terminal.kind")?,
        _ => unreachable!("schema checked above"),
    };
    Ok(ManifestEnvelope {
        schema_version,
        algorithm_id,
        terminal_status,
        terminal_cursor,
    })
}

fn populate_canonical_usage(conn: &Connection, run_id: &str, manifest: &mut Value) -> Result<()> {
    let usage_json: String = conn
        .query_row(
            "SELECT usage_json FROM optimizer_runs WHERE id = ?1",
            params![run_id],
            |row| row.get(0),
        )
        .optional()?
        .with_context(|| format!("optimizer run {run_id} disappeared before terminal sealing"))?;
    let usage: OptimizerUsageSummary = serde_json::from_str(&usage_json)
        .with_context(|| format!("decode canonical usage for optimizer run {run_id}"))?;
    let object = manifest
        .as_object_mut()
        .context("optimizer terminal manifest must be an object")?;
    let terminal_usage = object
        .get_mut("usage")
        .and_then(Value::as_object_mut)
        .context("optimizer terminal manifest is missing typed usage")?;
    terminal_usage.insert("costUsd".into(), json!(usage.cost_usd));
    terminal_usage.insert("promptTokens".into(), json!(usage.prompt_tokens));
    terminal_usage.insert("completionTokens".into(), json!(usage.completion_tokens));
    terminal_usage.insert("rollouts".into(), json!(usage.rollouts));
    terminal_usage.insert("wallTimeMs".into(), json!(usage.wall_time_ms));
    let approval = usage.extra.get("paidComputeApproval").cloned();
    if let Some(approval) = approval.as_ref() {
        validate_paid_compute_approval(approval)?;
    }
    object.insert(
        "paidComputeApproval".into(),
        approval.unwrap_or(Value::Null),
    );
    Ok(())
}

fn validate_paid_compute_approval(approval: &Value) -> Result<()> {
    let object = approval
        .as_object()
        .context("paidComputeApproval must be an object")?;
    object
        .get("approvalId")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .context("paidComputeApproval is missing typed approvalId")?;
    let cap = object
        .get("cap")
        .and_then(Value::as_object)
        .context("paidComputeApproval is missing typed cap")?;
    for key in ["maxCostUsdMicros", "maxRollouts"] {
        if !cap
            .get(key)
            .is_some_and(|value| value.is_null() || value.as_u64().is_some())
        {
            anyhow::bail!("paidComputeApproval cap is missing typed {key}");
        }
    }
    object
        .get("receiptViolation")
        .and_then(Value::as_bool)
        .context("paidComputeApproval is missing typed receiptViolation")?;
    Ok(())
}

/// Merge the sealed manifest over a live projection of the same run.
///
/// Terminal numbers come from the manifest; nothing a later poll computes may
/// move them. Used by every terminal read path.
pub(super) fn reconcile(
    mut projection: Map<String, Value>,
    manifest: &Value,
) -> Map<String, Value> {
    for key in [
        "terminalStatus",
        "terminalCursor",
        "work",
        "usage",
        "paidComputeApproval",
        "selection",
        "gepaEvidence",
        "degradation",
        "credentialChain",
        "startedAt",
        "finishedAt",
    ] {
        if let Some(value) = manifest.get(key) {
            projection.insert(key.into(), value.clone());
        }
    }
    projection
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimizers::models::{OptimizerRunRecord, OPTIMIZER_EVENT_SCHEMA_VERSION};

    fn run(algorithm_id: &str) -> OptimizerRunRecord {
        OptimizerRunRecord {
            schema_version: "optimizer_run.v1".into(),
            id: "run_1".into(),
            algorithm_id: algorithm_id.into(),
            algorithm_version: Some("1".into()),
            status: "completed".into(),
            source: "local".into(),
            objective: None,
            project_ref: None,
            session_ref: Some("chat_1".into()),
            created_at: "2026-08-17T21:36:56+00:00".into(),
            started_at: Some("2026-08-17T21:36:56+00:00".into()),
            finished_at: Some("2026-08-17T21:36:57+00:00".into()),
            cursor_seq: 13,
            capabilities: Default::default(),
            execution_bindings: vec![],
            input_refs: vec![],
            output_refs: vec![],
            visual_refs: vec![],
            summary: json!({}),
            usage: Default::default(),
            error: None,
        }
    }

    fn event(
        seq: u64,
        event_type: &str,
        item: Option<Value>,
        snapshot: Option<Value>,
    ) -> OptimizerEventEnvelope {
        OptimizerEventEnvelope {
            schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
            event_id: Some(format!("run_1:{seq}")),
            event_type: event_type.into(),
            sequence_number: seq,
            occurred_at: "2026-08-17T21:36:57+00:00".into(),
            optimizer_run_id: "run_1".into(),
            algorithm_id: "eval".into(),
            level: None,
            item,
            delta: Map::new(),
            snapshot: snapshot.and_then(|value| value.as_object().cloned()),
            usage_delta: None,
            artifact_refs: vec![],
            error: None,
            raw: json!({}),
        }
    }

    #[test]
    fn eval_counts_come_from_the_event_log_not_the_summary() {
        let mut events = vec![event(
            2,
            "eval.run.planned",
            None,
            Some(json!({ "planned_trials": 10 })),
        )];
        for seq in 3..13 {
            events.push(event(
                seq,
                "eval.trial.terminal",
                Some(json!({ "valid": true })),
                None,
            ));
        }
        let counts = work_counts(&run("eval"), &events);
        assert_eq!(counts.planned, Some(10));
        assert_eq!(counts.succeeded, Some(10));
        assert_eq!(counts.failed, Some(0));
        assert_eq!(counts.skipped, Some(0));
    }

    #[test]
    fn terminal_usage_comes_from_the_canonical_run_accumulator() {
        let mut canonical = run("eval");
        canonical.usage.rollouts = 3;
        canonical.usage.wall_time_ms = 75;
        let events = (1..=4)
            .map(|seq| {
                let mut entry = event(
                    seq,
                    "eval.trial.terminal",
                    Some(json!({ "valid": true })),
                    None,
                );
                entry.usage_delta = json!({
                    "rollouts": 1,
                    "wall_time_ms": 25,
                    "cost_usd": 0.0
                })
                .as_object()
                .cloned();
                entry
            })
            .collect::<Vec<_>>();
        let manifest = derive(&canonical, &events, "completed", None);
        assert_eq!(manifest.pointer("/usage/rollouts"), Some(&json!(3)));
        assert_eq!(manifest.pointer("/usage/wallTimeMs"), Some(&json!(75)));
        assert_eq!(manifest.pointer("/usage/costUsd"), Some(&Value::Null));
    }

    /// The failure this whole file is a response to: no evidence must read as
    /// *unknown*, never as a tidy row of zeroes.
    #[test]
    fn an_empty_event_log_reports_unknown_work_not_zero() {
        let counts = work_counts(&run("eval"), &[]);
        assert_eq!(counts.planned, None);
        assert_eq!(counts.succeeded, None);
        assert_eq!(counts.failed, None);
        assert_eq!(counts.unit, Some("trials"));
    }

    #[test]
    fn a_partial_plan_reports_the_unfinished_trials_as_skipped() {
        let events = vec![
            event(
                2,
                "eval.run.planned",
                None,
                Some(json!({ "planned_trials": 10 })),
            ),
            event(
                3,
                "eval.trial.terminal",
                Some(json!({ "valid": true })),
                None,
            ),
            event(
                4,
                "eval.trial.terminal",
                Some(json!({ "valid": false })),
                None,
            ),
        ];
        let counts = work_counts(&run("eval"), &events);
        assert_eq!(counts.succeeded, Some(1));
        assert_eq!(counts.failed, Some(1));
        assert_eq!(counts.skipped, Some(8));
    }

    #[test]
    fn a_manifest_records_unmeasured_cost_as_null() {
        let manifest = derive(&run("eval"), &[], "completed", None);
        assert_eq!(manifest.pointer("/usage/costUsd"), Some(&Value::Null));
        assert_eq!(manifest.pointer("/work/planned"), Some(&Value::Null));
    }

    #[test]
    fn a_manifest_carries_the_credential_receipt_chain() {
        let mut record = run("eval");
        record.summary = json!({
            "credentialChain": {
                "schemaVersion": "workshop.credential-chain.v1",
                "leaseDigest": "sha256:abc",
                "capabilityRevoked": true
            }
        });
        let manifest = derive(&record, &[], "completed", None);
        assert_eq!(
            manifest.pointer("/credentialChain/leaseDigest"),
            Some(&json!("sha256:abc"))
        );
        assert_eq!(
            manifest.pointer("/credentialChain/capabilityRevoked"),
            Some(&json!(true))
        );
    }

    fn manifest_tables(conn: &Connection, include_runs: bool) {
        if include_runs {
            conn.execute_batch(
                "CREATE TABLE optimizer_runs(id TEXT PRIMARY KEY, usage_json TEXT NOT NULL);",
            )
            .unwrap();
        }
        conn.execute_batch(
            "CREATE TABLE optimizer_terminal_manifests(
                optimizer_run_id TEXT PRIMARY KEY, schema_version TEXT NOT NULL,
                algorithm_id TEXT NOT NULL, terminal_status TEXT NOT NULL,
                terminal_cursor INTEGER NOT NULL, sealed_at TEXT NOT NULL,
                payload_json TEXT NOT NULL);",
        )
        .unwrap();
    }

    #[test]
    fn sealing_freezes_canonical_usage_and_paid_compute_approval() {
        let conn = Connection::open_in_memory().unwrap();
        manifest_tables(&conn, true);
        let usage = json!({
            "costUsd": 1.25, "promptTokens": 90, "completionTokens": 10,
            "rollouts": 4, "wallTimeMs": 200,
            "extra": { "paidComputeApproval": {
                "approvalId": "approval-1",
                "cap": { "maxCostUsdMicros": 2_000_000, "maxRollouts": 5 },
                "receiptViolation": false
            }}
        });
        conn.execute(
            "INSERT INTO optimizer_runs(id, usage_json) VALUES (?1, ?2)",
            params!["run_1", usage.to_string()],
        )
        .unwrap();
        let manifest = json!({
            "schemaVersion": TERMINAL_MANIFEST_SCHEMA_V2,
            "optimizerRunId": "run_1", "algorithmId": "eval", "terminalCursor": 8,
            "terminal": { "kind": "completed" }, "usage": {}
        });
        let sealed = seal(&conn, "run_1", &manifest).unwrap();
        assert_eq!(sealed.pointer("/usage/costUsd"), Some(&json!(1.25)));
        assert_eq!(sealed.pointer("/usage/rollouts"), Some(&json!(4)));
        assert_eq!(
            sealed.pointer("/paidComputeApproval/approvalId"),
            Some(&json!("approval-1"))
        );
        let row_schema: String = conn.query_row(
            "SELECT schema_version FROM optimizer_terminal_manifests WHERE optimizer_run_id='run_1'",
            [], |row| row.get(0),
        ).unwrap();
        assert_eq!(row_schema, TERMINAL_MANIFEST_SCHEMA_V2);
    }

    #[test]
    fn load_rejects_a_manifest_whose_envelope_schema_disagrees() {
        let conn = Connection::open_in_memory().unwrap();
        manifest_tables(&conn, false);
        let payload = json!({
            "schemaVersion": TERMINAL_MANIFEST_SCHEMA_V2,
            "optimizerRunId": "run_1", "algorithmId": "eval", "terminalCursor": 8,
            "terminal": { "kind": "completed" }, "usage": {}
        });
        conn.execute(
            "INSERT INTO optimizer_terminal_manifests VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                "run_1",
                TERMINAL_MANIFEST_SCHEMA,
                "eval",
                "completed",
                8,
                "now",
                payload.to_string()
            ],
        )
        .unwrap();
        let error = load(&conn, "run_1").unwrap_err().to_string();
        assert!(error.contains("envelope disagrees"), "{error}");
    }

    #[test]
    fn sealing_refuses_an_open_world_terminal_manifest() {
        let conn = Connection::open_in_memory().unwrap();
        manifest_tables(&conn, true);
        conn.execute(
            "INSERT INTO optimizer_runs(id, usage_json) VALUES (?1, ?2)",
            params!["run_1", json!({}).to_string()],
        )
        .unwrap();
        let manifest = json!({
            "schemaVersion": TERMINAL_MANIFEST_SCHEMA_V2,
            "optimizerRunId": "run_1", "algorithmId": "eval", "terminalCursor": 8,
            "terminal": { "kind": "failed" }, "usage": {},
            "work": { "planned": 5, "running": 4, "queued": 0, "succeeded": 0, "failed": 1 }
        });
        let error = seal(&conn, "run_1", &manifest).unwrap_err().to_string();
        assert!(error.contains("open-world"), "{error}");
        assert!(error.contains("work.running=4"), "{error}");
    }

    /// Legacy seals from before the write guard must stay loadable: refusing
    /// them is what made the incident run permanently unreadable. They load
    /// as-is with an operator-visible warning.
    #[test]
    fn a_legacy_open_world_manifest_still_loads() {
        let conn = Connection::open_in_memory().unwrap();
        manifest_tables(&conn, false);
        let payload = json!({
            "schemaVersion": TERMINAL_MANIFEST_SCHEMA_V2,
            "optimizerRunId": "run_1", "algorithmId": "eval", "terminalCursor": 8,
            "terminal": { "kind": "failed" }, "usage": {},
            "work": { "planned": 5, "running": 4, "queued": 0, "succeeded": 0, "failed": 1 }
        });
        conn.execute(
            "INSERT INTO optimizer_terminal_manifests VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                "run_1",
                TERMINAL_MANIFEST_SCHEMA_V2,
                "eval",
                "failed",
                8,
                "now",
                payload.to_string()
            ],
        )
        .unwrap();
        let loaded = load(&conn, "run_1").unwrap().expect("legacy manifest loads");
        assert_eq!(loaded.pointer("/work/running"), Some(&json!(4)));
    }

    #[test]
    fn reconcile_freezes_terminal_numbers_over_a_later_projection() {
        let manifest = json!({
            "terminalCursor": 13,
            "work": { "succeeded": 10 },
        });
        let projection = Map::from_iter([
            ("terminalCursor".into(), json!(1)),
            ("work".into(), json!({ "succeeded": 0 })),
            ("phase".into(), json!("done")),
        ]);
        let merged = reconcile(projection, &manifest);
        assert_eq!(merged.get("terminalCursor"), Some(&json!(13)));
        assert_eq!(merged.get("work"), Some(&json!({ "succeeded": 10 })));
        assert_eq!(merged.get("phase"), Some(&json!("done")));
    }
}
