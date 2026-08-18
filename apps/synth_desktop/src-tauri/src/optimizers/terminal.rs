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
use super::models::{OptimizerEventEnvelope, OptimizerRunRecord};

pub(super) const TERMINAL_MANIFEST_SCHEMA: &str = "optimizer_terminal_manifest.v1";

/// Terminal status spellings the manifest may carry. `failed_evidence` is the
/// one that did not exist before: compute succeeded, its evidence did not, and
/// calling that "completed" is what hid the Banking77 loss.
pub(super) const STATUS_FAILED_EVIDENCE: &str = "failed_evidence";

/// Work counts, in the unit the algorithm actually plans in. Every field is
/// `Option` because a producer that never declared a plan has not declared zero.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct WorkCounts {
    pub planned: Option<u64>,
    pub succeeded: Option<u64>,
    pub failed: Option<u64>,
    pub skipped: Option<u64>,
    pub unit: Option<&'static str>,
}

impl WorkCounts {
    fn to_value(&self) -> Value {
        json!({
            "planned": self.planned,
            "succeeded": self.succeeded,
            "failed": self.failed,
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
    match run.algorithm_id.as_str() {
        "eval" => eval_counts(events),
        "gepa" | "go-ex" => rollout_counts(run, events),
        "sft" => sft_counts(events),
        _ => WorkCounts::default(),
    }
}

fn eval_counts(events: &[OptimizerEventEnvelope]) -> WorkCounts {
    let planned = events
        .iter()
        .filter(|event| event.event_type == "eval.run.planned")
        .filter_map(|event| {
            event
                .snapshot
                .as_ref()?
                .get("planned_trials")
                .or_else(|| event.snapshot.as_ref()?.get("plannedTrials"))
                .and_then(Value::as_u64)
        })
        .next_back();
    let mut succeeded = 0u64;
    let mut failed = 0u64;
    let mut saw_terminal = false;
    for event in events {
        if event.event_type != "eval.trial.terminal" {
            continue;
        }
        saw_terminal = true;
        let valid = event
            .item
            .as_ref()
            .and_then(|item| item.get("valid"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if valid {
            succeeded += 1;
        } else {
            failed += 1;
        }
    }
    if planned.is_none() && !saw_terminal {
        return WorkCounts {
            unit: Some("trials"),
            ..WorkCounts::default()
        };
    }
    let settled = succeeded + failed;
    WorkCounts {
        planned,
        succeeded: Some(succeeded),
        failed: Some(failed),
        skipped: planned.map(|planned| planned.saturating_sub(settled)),
        unit: Some("trials"),
    }
}

/// GEPA and GO-Ex rollout counts.
///
/// These used to look for `rollout.completed`, `rollout.terminal`, and
/// `gepa.rollout.completed` — three spellings the GEPA producer has never
/// emitted. Every GEPA manifest therefore sealed with `succeeded: null`, and a
/// 140-rollout search settled looking exactly like a search that measured
/// nothing. The counts now come from the events that do exist, via the same
/// reduction the typed result and the verdict are built from.
///
/// `planned` stays `None` on purpose. GEPA declares a rollout *ceiling*, not a
/// plan: a run that spends 320 of an 850 budget has not skipped 530 rollouts,
/// it has finished under budget.
fn rollout_counts(run: &OptimizerRunRecord, events: &[OptimizerEventEnvelope]) -> WorkCounts {
    let evidence = gepa_evidence::reduce(run, events);
    if evidence.rollouts_allocated == 0
        && evidence.rollouts_scored == 0
        && evidence.rollouts_failed == 0
    {
        return WorkCounts {
            unit: Some("rollouts"),
            ..WorkCounts::default()
        };
    }
    WorkCounts {
        planned: None,
        succeeded: Some(evidence.rollouts_scored),
        failed: Some(evidence.rollouts_failed),
        skipped: None,
        unit: Some("rollouts"),
    }
}

fn sft_counts(events: &[OptimizerEventEnvelope]) -> WorkCounts {
    let planned = events
        .iter()
        .filter_map(|event| {
            event
                .snapshot
                .as_ref()?
                .get("total_steps")
                .or_else(|| event.snapshot.as_ref()?.get("totalSteps"))
                .and_then(Value::as_u64)
        })
        .next_back();
    let completed = events
        .iter()
        .filter_map(|event| {
            event
                .snapshot
                .as_ref()?
                .get("step")
                .or_else(|| event.delta.get("step"))
                .and_then(Value::as_u64)
        })
        .next_back();
    WorkCounts {
        planned,
        succeeded: completed,
        failed: None,
        skipped: None,
        unit: Some("steps"),
    }
}

/// Usage as the manifest records it: lanes preserved, unknowns preserved.
fn usage_value(run: &OptimizerRunRecord, events: &[OptimizerEventEnvelope]) -> Value {
    // The terminal manifest is answerable to the durable event log, just like
    // work counts. Re-fold deltas at the frozen cursor instead of trusting the
    // mutable run projection: concurrent terminal events can otherwise leave
    // the cached total one rollout behind even though every event is present.
    let deltas = events
        .iter()
        .filter_map(|event| event.usage_delta.as_ref())
        .collect::<Vec<_>>();
    let prompt_tokens = if deltas.is_empty() {
        run.usage.prompt_tokens
    } else {
        deltas
            .iter()
            .filter_map(|usage| usage.get("prompt_tokens").and_then(Value::as_u64))
            .sum()
    };
    let completion_tokens = if deltas.is_empty() {
        run.usage.completion_tokens
    } else {
        deltas
            .iter()
            .filter_map(|usage| usage.get("completion_tokens").and_then(Value::as_u64))
            .sum()
    };
    let rollouts = if deltas.is_empty() {
        run.usage.rollouts
    } else {
        deltas
            .iter()
            .filter_map(|usage| usage.get("rollouts").and_then(Value::as_u64))
            .sum()
    };
    let wall_time_ms = if deltas.is_empty() {
        run.usage.wall_time_ms
    } else {
        deltas
            .iter()
            .filter_map(|usage| usage.get("wall_time_ms").and_then(Value::as_u64))
            .sum()
    };
    let reported_costs = deltas
        .iter()
        .filter_map(|usage| {
            usage
                .get("cost_usd")
                .or_else(|| usage.get("costUsd"))
                .and_then(Value::as_f64)
        })
        .collect::<Vec<_>>();
    let cost_usd = if reported_costs.is_empty() {
        run.usage.cost_usd
    } else {
        Some(reported_costs.iter().sum::<f64>())
    };
    let lanes = run
        .summary
        .get("usageLanes")
        .cloned()
        .unwrap_or(Value::Null);
    json!({
        "costUsd": cost_usd,
        "promptTokens": prompt_tokens,
        "completionTokens": completion_tokens,
        "rollouts": rollouts,
        "wallTimeMs": wall_time_ms,
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
    let mut usage = usage_value(run, events);
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
    let terminal_status = manifest
        .get("terminalStatus")
        .and_then(Value::as_str)
        .unwrap_or("completed");
    let terminal_cursor = manifest
        .get("terminalCursor")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    conn.execute(
        "INSERT INTO optimizer_terminal_manifests(
            optimizer_run_id, schema_version, algorithm_id, terminal_status,
            terminal_cursor, sealed_at, payload_json
         ) VALUES (?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(optimizer_run_id) DO NOTHING",
        params![
            run_id,
            TERMINAL_MANIFEST_SCHEMA,
            manifest
                .get("algorithmId")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            terminal_status,
            terminal_cursor as i64,
            chrono::Utc::now().to_rfc3339(),
            serde_json::to_string(manifest)?,
        ],
    )
    .context("seal optimizer terminal manifest")?;
    load(conn, run_id)?.context("terminal manifest disappeared immediately after sealing")
}

pub(super) fn load(conn: &Connection, run_id: &str) -> Result<Option<Value>> {
    let payload: Option<String> = conn
        .query_row(
            "SELECT payload_json FROM optimizer_terminal_manifests WHERE optimizer_run_id = ?1",
            params![run_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(match payload {
        Some(payload) => Some(serde_json::from_str(&payload)?),
        None => None,
    })
}

/// Record that a sealed run's evidence lane failed after the fact. The manifest
/// body stays as sealed; only the degradation lane is amended, because a
/// degradation discovered later is new information about a settled run, not a
/// different ending.
pub(super) fn amend_degradation(conn: &Connection, run_id: &str, degradation: Value) -> Result<()> {
    let Some(mut manifest) = load(conn, run_id)? else {
        return Ok(());
    };
    if let Some(object) = manifest.as_object_mut() {
        let mut entries = object
            .get("degradation")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_else(|| match object.get("degradation") {
                Some(Value::Null) | None => Vec::new(),
                Some(other) => vec![other.clone()],
            });
        entries.push(degradation);
        object.insert("degradation".into(), Value::Array(entries));
    }
    conn.execute(
        "UPDATE optimizer_terminal_manifests SET payload_json = ?2 WHERE optimizer_run_id = ?1",
        params![run_id, serde_json::to_string(&manifest)?],
    )?;
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
        "selection",
        "gepaEvidence",
        "degradation",
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
    fn terminal_usage_comes_from_the_frozen_event_log() {
        let mut stale = run("eval");
        stale.usage.rollouts = 3;
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
        let manifest = derive(&stale, &events, "completed", None);
        assert_eq!(manifest.pointer("/usage/rollouts"), Some(&json!(4)));
        assert_eq!(manifest.pointer("/usage/wallTimeMs"), Some(&json!(100)));
        assert_eq!(manifest.pointer("/usage/costUsd"), Some(&json!(0.0)));
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
