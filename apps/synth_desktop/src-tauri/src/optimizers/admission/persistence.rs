//! Durable admission records.
//!
//! A run keeps the whole canonical specification, not a summary of it. The
//! point is that reopening a finished run reconstructs exactly what executed —
//! the same container pin, evaluator, policy revision, seeds, and bounds — so
//! it can be inspected long after the container has moved on, and reused as the
//! basis of a new run.
//!
//! Reuse deliberately re-enters admission. The stored row carries the receipt
//! the run was approved with, but nothing here hands that receipt to a new run:
//! a receipt is consent for one specification at one moment, and spending it
//! twice is the thing the whole pipeline exists to prevent.

use super::ids::Digest;
use super::pipeline::{AdmissibleExecutionSpec, ApprovedExecutionSpec};
use super::spec::ExecutionSpec;
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension};

/// Read the immutable execution specification from the canonical kernel
/// admission record.
pub fn load_admitted_execution_spec(
    conn: &Connection,
    optimizer_run_id: &str,
) -> Result<Option<ExecutionSpec>> {
    let row = conn
        .query_row(
            "SELECT spec_json, spec_digest
             FROM optimizer_run_specs WHERE optimizer_run_id = ?1",
            [optimizer_run_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .context("read admitted optimizer execution specification")?;
    let Some((spec_json, spec_digest)) = row else {
        return Ok(None);
    };
    let execution_spec: ExecutionSpec =
        serde_json::from_str(&spec_json).context("decode admitted execution specification")?;
    let stored_digest = Digest::parse(spec_digest).context("decode admitted spec digest")?;
    let recomputed = execution_spec
        .digest()
        .context("recompute admitted execution spec digest")?;
    anyhow::ensure!(
        recomputed == stored_digest,
        "admitted execution specification for `{optimizer_run_id}` does not match its recorded digest"
    );
    Ok(Some(execution_spec))
}

/// Record an admission that has not yet been approved.
///
/// The draft identity is not an optimizer run id. No `optimizer_runs` row is
/// created here; consuming an approved draft is the transaction that mints the
/// run.
pub fn stage_admissible(
    conn: &Connection,
    draft_id: &str,
    admissible: &AdmissibleExecutionSpec,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let spec_json =
        serde_json::to_string(admissible.spec()).context("serialize execution specification")?;
    let mut draft = crate::optimizers::kernel::RunDraft::new(
        draft_id,
        crate::optimizers::kernel::AlgorithmKind::Eval,
        admissible.digest().as_str(),
        spec_json,
        &now,
    );
    draft
        .transition(crate::optimizers::kernel::AdmissionState::Validating, &now)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    draft
        .transition(
            crate::optimizers::kernel::AdmissionState::AwaitingApproval,
            &now,
        )
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    crate::optimizers::kernel::persist::insert_draft(conn, &draft)
}

/// Persist an already-approved eval specification as a draft, still without a
/// run. `consume_approved_eval_draft` is the only writer that may create the run.
pub fn stage_approved_eval_draft(
    conn: &Connection,
    draft_id: &str,
    approved: &ApprovedExecutionSpec,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let spec_json =
        serde_json::to_string(approved.spec()).context("serialize execution specification")?;
    let mut draft = crate::optimizers::kernel::RunDraft::new(
        draft_id,
        crate::optimizers::kernel::AlgorithmKind::Eval,
        approved.digest().as_str(),
        spec_json,
        &now,
    );
    draft.authorization_ref = Some(approved.binding().receipt_id.as_str().to_string());
    for next in [
        crate::optimizers::kernel::AdmissionState::Validating,
        crate::optimizers::kernel::AdmissionState::AwaitingApproval,
        crate::optimizers::kernel::AdmissionState::Approved,
    ] {
        draft
            .transition(next, &now)
            .map_err(|error| anyhow::anyhow!("{error}"))?;
    }
    crate::optimizers::kernel::persist::insert_draft(conn, &draft)
}

/// Seal the spec, consume the draft, and write the initial kernel projection.
/// The optimizer run row must be inserted in the same transaction, after the
/// draft is approved and before this returns.
pub fn consume_approved_eval_draft(
    conn: &Connection,
    draft_id: &str,
    run_id: &str,
    admitted_at: &str,
) -> Result<crate::optimizers::kernel::AdmissionCommit> {
    let draft = crate::optimizers::kernel::persist::load_draft(conn, draft_id)?
        .ok_or_else(|| anyhow::anyhow!("optimizer run draft `{draft_id}` does not exist"))?;
    let commit = crate::optimizers::kernel::AdmissionCommit::from_approved_draft(
        &draft,
        run_id,
        crate::optimizers::kernel::ExecutionPlacement::DirectContainerEvaluation,
        admitted_at,
    )
    .map_err(|error| anyhow::anyhow!("{error}"))?;
    crate::optimizers::kernel::persist::consume_draft(conn, draft_id, admitted_at)?;
    crate::optimizers::kernel::persist::insert_spec(conn, &commit)?;
    let state = crate::optimizers::kernel::RunKernelState::from_admission(&commit);
    crate::optimizers::kernel::persist::upsert_projection(conn, &state)?;
    Ok(commit)
}
