//! Typed per-algorithm results.
//!
//! `get_result` used to be one GEPA-shaped function that every algorithm fell
//! through. It looked for `best_candidate.json` on disk, dug a prompt out of it,
//! and refused to return anything for a *completed* run that had no materialized
//! prompt. A baseline eval has no candidate and no optimized prompt — nothing to
//! materialize, by design — so a 10/10 Banking77 campaign answered
//! "completed GEPA result omitted a materialized prompt".
//!
//! Dispatch is now on the run's own `algorithm_id`, never on which files happen
//! to exist. Each algorithm declares what its result *is*:
//!
//!   · `gepa_run_result.v1` — a selected candidate with materialized values.
//!   · `eval_run_result.v1` — trial counts, aggregate metrics, usage, evidence,
//!     and a baseline-only selection verdict. It is never asked for a prompt.
//!   · `sft_run_result.v1` — checkpoints and the inference endpoint, if any.
//!   · `environment_run_result.v1` — episode outcomes.
//!
//! Every variant reconciles against the sealed terminal manifest, so what a
//! result says about counts, usage, and selection is what the manifest froze.

use anyhow::Result;
use serde_json::{json, Map, Value};

use super::models::OptimizerRunRecord;
use super::terminal;

pub(super) const RESULT_SCHEMA_VERSION: &str = "optimizer_result.v1";

/// Which typed result a run produces. Derived from the authoritative
/// `algorithm_id` — the run record's own field, set at creation.
pub(super) fn result_kind(algorithm_id: &str) -> &'static str {
    match algorithm_id {
        "gepa" => "gepa_run_result.v1",
        "eval" => "eval_run_result.v1",
        "sft" => "sft_run_result.v1",
        "environment" | "go-ex" => "environment_run_result.v1",
        _ => "optimizer_run_result.v1",
    }
}

/// Does this algorithm's result contain a materialized candidate at all?
///
/// Only optimization algorithms produce one. Asking an eval for a materialized
/// prompt is a category error, and failing the read when it has none was the
/// bug, not the safeguard.
pub(super) fn materializes_candidate(algorithm_id: &str) -> bool {
    matches!(algorithm_id, "gepa" | "go-ex")
}

/// The envelope every typed result shares, before its algorithm-specific body.
pub(super) fn envelope(run: &OptimizerRunRecord, manifest: Option<&Value>) -> Map<String, Value> {
    let mut out = Map::new();
    out.insert("schemaVersion".into(), json!(RESULT_SCHEMA_VERSION));
    out.insert("resultKind".into(), json!(result_kind(&run.algorithm_id)));
    out.insert("optimizerRunId".into(), json!(run.id));
    out.insert("algorithmId".into(), json!(run.algorithm_id));
    out.insert("status".into(), json!(run.status));
    // The manifest's cursor, when sealed. A later poll's cursor is not the
    // cursor the run ended at.
    out.insert(
        "finalCursor".into(),
        manifest
            .and_then(|value| value.get("terminalCursor").cloned())
            .unwrap_or(json!(run.cursor_seq)),
    );
    out.insert(
        "usage".into(),
        serde_json::to_value(&run.usage).unwrap_or(Value::Null),
    );
    out.insert(
        "terminalManifest".into(),
        manifest.cloned().unwrap_or(Value::Null),
    );
    if manifest.is_none() {
        out.insert(
            "evidence".into(),
            json!({
                "state": "unsealed",
                "reason": "this run has no terminal manifest; counts below are a live reading, not a settled result",
            }),
        );
    }
    out.insert(
        "completionReceiptId".into(),
        json!(format!("optimizer_completion_{}", run.id)),
    );
    out
}

/// `eval_run_result.v1`.
///
/// A baseline eval's verdict is that there is no promotion decision to make. It
/// says so explicitly rather than leaving the caller to read an absent winner as
/// a failure.
pub(super) fn eval_result(run: &OptimizerRunRecord, manifest: Option<&Value>) -> Result<Value> {
    let mut out = envelope(run, manifest);
    let work = manifest
        .and_then(|value| value.get("work").cloned())
        .unwrap_or_else(|| {
            run.summary
                .get("progress")
                .cloned()
                .unwrap_or(Value::Null)
        });
    let selection = manifest
        .and_then(|value| value.get("selection").cloned())
        .filter(|value| !value.is_null())
        .or_else(|| run.summary.get("selection").cloned())
        .unwrap_or_else(|| {
            json!({
                "status": "inconclusive",
                "winnerId": null,
                "reason": "baseline-only evaluation; no promotion decision",
            })
        });
    out.insert("trials".into(), work);
    out.insert(
        "metrics".into(),
        json!({
            "meanReward": run.summary.get("meanReward").cloned().unwrap_or(Value::Null),
            "primaryMetric": "mean_reward",
            "selection": selection,
        }),
    );
    out.insert(
        "usageLanes".into(),
        run.summary
            .get("usageLanes")
            .cloned()
            .unwrap_or(Value::Null),
    );
    out.insert(
        "policyRef".into(),
        run.summary.get("policyRef").cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "evidenceRefs".into(),
        json!({
            "records": run.summary.get("records").map(|records| {
                json!({ "count": records.as_array().map(Vec::len).unwrap_or(0), "location": "summary.records" })
            }),
            "visualId": run.summary.get("visualId").cloned().unwrap_or(Value::Null),
            "artifacts": manifest
                .and_then(|value| value.get("artifactRefs").cloned())
                .unwrap_or(Value::Null),
        }),
    );
    out.insert(
        "limitations".into(),
        json!(["Baseline-only. No candidate generation and no uplift claim."]),
    );
    Ok(Value::Object(out))
}

/// `sft_run_result.v1`.
pub(super) fn sft_result(run: &OptimizerRunRecord, manifest: Option<&Value>) -> Result<Value> {
    let mut out = envelope(run, manifest);
    out.insert(
        "training".into(),
        json!({
            "baseModel": run.summary.get("baseModel").cloned().unwrap_or(Value::Null),
            "steps": manifest
                .and_then(|value| value.get("work").cloned())
                .unwrap_or(Value::Null),
            "trainLoss": run.summary.get("trainLoss").cloned().unwrap_or(Value::Null),
        }),
    );
    out.insert(
        "checkpoints".into(),
        run.summary
            .get("checkpoints")
            .cloned()
            .unwrap_or(json!([])),
    );
    out.insert(
        "inferenceEndpoint".into(),
        run.summary
            .get("inferenceEndpoint")
            .cloned()
            .unwrap_or(Value::Null),
    );
    Ok(Value::Object(out))
}

/// `environment_run_result.v1`.
pub(super) fn environment_result(
    run: &OptimizerRunRecord,
    manifest: Option<&Value>,
) -> Result<Value> {
    let mut out = envelope(run, manifest);
    out.insert(
        "episodes".into(),
        manifest
            .and_then(|value| value.get("work").cloned())
            .unwrap_or(Value::Null),
    );
    out.insert(
        "metrics".into(),
        run.summary.get("metrics").cloned().unwrap_or(Value::Null),
    );
    Ok(Value::Object(out))
}

/// Anything with no typed contract yet. Honest about being generic rather than
/// borrowing another algorithm's shape.
pub(super) fn generic_result(run: &OptimizerRunRecord, manifest: Option<&Value>) -> Result<Value> {
    let mut out = envelope(run, manifest);
    out.insert(
        "summary".into(),
        run.summary.clone(),
    );
    Ok(Value::Object(out))
}

/// Overlay the sealed manifest's frozen lanes onto a result body.
pub(super) fn with_manifest(result: Value, manifest: &Value) -> Value {
    let Some(object) = result.as_object().cloned() else {
        return result;
    };
    Value::Object(terminal::reconcile(object, manifest))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eval_run() -> OptimizerRunRecord {
        OptimizerRunRecord {
            schema_version: "optimizer_run.v1".into(),
            id: "opt_eval_banking77_81d51f81b59f".into(),
            algorithm_id: "eval".into(),
            algorithm_version: Some("1".into()),
            status: "completed".into(),
            source: "local".into(),
            objective: Some("Banking77 baseline eval".into()),
            project_ref: None,
            session_ref: Some("chat_1".into()),
            created_at: "2026-08-17T21:36:56+00:00".into(),
            started_at: Some("2026-08-17T21:36:56+00:00".into()),
            finished_at: Some("2026-08-17T21:36:57+00:00".into()),
            cursor_seq: 14,
            capabilities: Default::default(),
            execution_bindings: vec![],
            input_refs: vec![],
            output_refs: vec![],
            visual_refs: vec![],
            summary: json!({
                "meanReward": 1.0,
                "records": [1, 2, 3],
                "visualId": "vis_7fc27280fde04974bd0e88cc1cc67ee5",
                "policyRef": { "config": "banking77_gpt_4_1_nano" },
            }),
            usage: Default::default(),
            error: None,
        }
    }

    /// The reported defect, inverted into a contract: an eval result is
    /// answerable without any candidate materialization at all.
    #[test]
    fn an_eval_result_never_needs_a_materialized_prompt() {
        assert!(!materializes_candidate("eval"));
        assert_eq!(result_kind("eval"), "eval_run_result.v1");
        let result = eval_result(&eval_run(), None).unwrap();
        assert_eq!(result["resultKind"], json!("eval_run_result.v1"));
        assert_eq!(result["metrics"]["meanReward"], json!(1.0));
        assert!(result.get("selectedCandidate").is_none());
    }

    #[test]
    fn an_eval_result_states_the_baseline_only_verdict() {
        let result = eval_result(&eval_run(), None).unwrap();
        assert_eq!(
            result["metrics"]["selection"]["status"],
            json!("inconclusive")
        );
        assert_eq!(result["metrics"]["selection"]["winnerId"], Value::Null);
    }

    #[test]
    fn a_sealed_manifest_supplies_the_frozen_counts_and_cursor() {
        let manifest = json!({
            "terminalCursor": 14,
            "work": { "planned": 10, "succeeded": 10, "failed": 0, "skipped": 0, "unit": "trials" },
            "selection": { "status": "inconclusive", "winnerId": null },
        });
        let result = eval_result(&eval_run(), Some(&manifest)).unwrap();
        assert_eq!(result["finalCursor"], json!(14));
        assert_eq!(result["trials"]["succeeded"], json!(10));
        assert!(result.get("evidence").is_none(), "a sealed run is not unsealed");
    }

    /// A run with no manifest yet must say so rather than presenting a live
    /// reading as a settled result.
    #[test]
    fn an_unsealed_run_labels_its_result_as_unsettled() {
        let result = eval_result(&eval_run(), None).unwrap();
        assert_eq!(result["evidence"]["state"], json!("unsealed"));
    }

    #[test]
    fn each_algorithm_gets_its_own_result_kind() {
        assert_eq!(result_kind("gepa"), "gepa_run_result.v1");
        assert_eq!(result_kind("sft"), "sft_run_result.v1");
        assert_eq!(result_kind("environment"), "environment_run_result.v1");
        assert_eq!(result_kind("dag"), "optimizer_run_result.v1");
    }
}
