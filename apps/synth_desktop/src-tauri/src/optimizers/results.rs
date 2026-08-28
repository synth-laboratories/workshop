//! Typed optimizer results rendered from the durable run-kernel projection.
//!
//! Result dispatch belongs to `AlgorithmProjection::settle`. This adapter only
//! adds the stable IPC envelope. Filesystem artifacts and the retained legacy
//! terminal manifest remain evidence; neither can choose or rewrite the body.

use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};

use super::kernel::{AlgorithmKind, AlgorithmResult, RunKernelState};
use super::models::OptimizerRunRecord;

pub(super) const RESULT_SCHEMA_VERSION: &str = "optimizer_result.v2";

pub(super) fn result_kind(algorithm_id: &str) -> &'static str {
    AlgorithmKind::parse_wire(algorithm_id)
        .map(AlgorithmKind::result_schema)
        .unwrap_or("optimizer_run_result.v2")
}

/// Wrap one algorithm-owned settled result without interpreting its fields.
pub(super) fn from_kernel(
    run: &OptimizerRunRecord,
    state: &RunKernelState,
    result: AlgorithmResult,
    retained_legacy_manifest: Option<&Value>,
) -> Result<Value> {
    if run.id != state.run_id {
        bail!(
            "optimizer result identity mismatch: record {} != projection {}",
            run.id,
            state.run_id
        );
    }
    if result.kind() != state.algorithm {
        bail!(
            "optimizer result algorithm mismatch: body {} != projection {}",
            result.kind().wire_id(),
            state.algorithm.wire_id()
        );
    }

    let typed = serde_json::to_value(&result).context("encode typed optimizer result")?;
    let mut out = Map::new();
    out.insert("schemaVersion".into(), json!(RESULT_SCHEMA_VERSION));
    out.insert("resultKind".into(), json!(state.algorithm.result_schema()));
    out.insert("optimizerRunId".into(), json!(state.run_id));
    out.insert("algorithmId".into(), json!(state.algorithm.wire_id()));
    out.insert("status".into(), json!(run.status));
    out.insert("lifecycle".into(), json!(state.lifecycle.as_str()));
    out.insert("phase".into(), serde_json::to_value(state.phase)?);
    out.insert("condition".into(), serde_json::to_value(state.condition)?);
    let final_cursor = state
        .terminal
        .as_ref()
        .map(|terminal| terminal.final_sequence)
        .unwrap_or(state.aggregate_sequence);
    out.insert("finalCursor".into(), json!(final_cursor));
    if state.aggregate_sequence > final_cursor {
        out.insert("enrichmentCursor".into(), json!(state.aggregate_sequence));
    }
    out.insert(
        "projectionRevision".into(),
        json!(state.projection_revision),
    );
    out.insert("specDigest".into(), json!(state.spec_digest));
    out.insert("usage".into(), serde_json::to_value(state.usage())?);
    out.insert("work".into(), serde_json::to_value(state.work_summary())?);
    out.insert(
        "evidence".into(),
        serde_json::to_value(state.evidence_state())?,
    );
    out.insert("terminal".into(), serde_json::to_value(&state.terminal)?);
    out.insert("typedResult".into(), typed.clone());
    out.insert(
        "retainedLegacyManifest".into(),
        retained_legacy_manifest.cloned().unwrap_or(Value::Null),
    );
    out.insert(
        "completionReceiptId".into(),
        json!(format!("optimizer_completion_{}", run.id)),
    );

    // Keep algorithm fields convenient for existing clients while the tagged
    // `typedResult` remains the canonical, lossless body.
    if let Some(body) = typed.as_object() {
        for (key, value) in body {
            if key != "algorithm" && !out.contains_key(key) {
                out.insert(key.clone(), value.clone());
            }
        }
    }
    Ok(Value::Object(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn result_schema_is_owned_by_the_algorithm_registry() {
        assert_eq!(result_kind("eval"), "eval_run_result.v1");
        assert_eq!(result_kind("gepa"), "gepa_run_result.v1");
        assert_eq!(result_kind("go-ex"), "go_ex_run_result.v1");
        assert_eq!(result_kind("sft"), "sft_run_result.v1");
        assert_eq!(result_kind("cispo"), "cispo_run_result.v1");
        assert_eq!(result_kind("gelo"), "optimizer_run_result.v2");
    }
}
