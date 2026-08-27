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

use super::ids::{
    ApprovalReceiptId, DeclarationDigest, Digest, PolicyRevision, RecipeId,
};
use super::pipeline::{AdmissibleExecutionSpec, ApprovedExecutionSpec};
use super::spec::{ExecutionSpec, RecipeSourceKind};
use super::state::{RolloutRecord, RolloutState, RolloutStateHolder, RunProgress, RunState};
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::BTreeMap;

/// One persisted admission, as the columns the handoff specifies.
#[derive(Clone, Debug, PartialEq)]
pub struct EvaluationRunRecord {
    pub optimizer_run_id: String,
    pub recipe_source_kind: RecipeSourceKind,
    pub catalog_recipe_id: Option<RecipeId>,
    pub execution_spec: ExecutionSpec,
    pub execution_spec_digest: Digest,
    pub container_declaration_digest: DeclarationDigest,
    pub policy_revision: PolicyRevision,
    pub policy_configuration_digest: Digest,
    pub approval_receipt_id: ApprovalReceiptId,
}

impl EvaluationRunRecord {
    /// Build the durable record from the approved specification that is about
    /// to execute, so the row and the execution cannot disagree.
    pub fn from_approved(optimizer_run_id: impl Into<String>, approved: &ApprovedExecutionSpec) -> Self {
        let spec = approved.spec();
        let binding = approved.binding();
        Self {
            optimizer_run_id: optimizer_run_id.into(),
            recipe_source_kind: spec.source_kind,
            catalog_recipe_id: spec.catalog_recipe_id.clone(),
            execution_spec: spec.clone(),
            execution_spec_digest: approved.digest().clone(),
            container_declaration_digest: spec.recipe.container.declaration_digest.clone(),
            policy_revision: spec.recipe.policy.revision.clone(),
            policy_configuration_digest: spec.recipe.policy.configuration_digest.clone(),
            approval_receipt_id: binding.receipt_id.clone(),
        }
    }
}

/// Write the admission record. Called in the same transaction that creates the
/// run, so a run can never exist without the specification it is running.
pub fn insert_evaluation_run(conn: &Connection, record: &EvaluationRunRecord) -> Result<()> {
    let spec_json = serde_json::to_string(&record.execution_spec)
        .context("serialize execution specification")?;
    conn.execute(
        "INSERT INTO evaluation_runs (
            optimizer_run_id, recipe_source_kind, catalog_recipe_id,
            execution_spec_json, execution_spec_digest,
            container_declaration_digest, policy_revision,
            policy_configuration_digest, approval_receipt_id, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            record.optimizer_run_id,
            record.recipe_source_kind.as_str(),
            record.catalog_recipe_id.as_ref().map(RecipeId::as_str),
            spec_json,
            record.execution_spec_digest.as_str(),
            record.container_declaration_digest.as_str(),
            record.policy_revision.as_str(),
            record.policy_configuration_digest.as_str(),
            record.approval_receipt_id.as_str(),
            chrono::Utc::now().to_rfc3339(),
        ],
    )
    .context("insert evaluation_runs row")?;
    Ok(())
}

/// Read one admission back.
///
/// The stored digest is re-checked against the stored specification rather than
/// trusted. A row whose specification was rewritten in place — by a migration,
/// a manual edit, a re-serialization — is a corrupted record, and answering
/// with it as though it were the approved specification would be exactly the
/// silent substitution the digest exists to catch.
pub fn load_evaluation_run(
    conn: &Connection,
    optimizer_run_id: &str,
) -> Result<Option<EvaluationRunRecord>> {
    let row = conn
        .query_row(
            "SELECT recipe_source_kind, catalog_recipe_id, execution_spec_json,
                    execution_spec_digest, container_declaration_digest,
                    policy_revision, policy_configuration_digest, approval_receipt_id
             FROM evaluation_runs WHERE optimizer_run_id = ?1",
            [optimizer_run_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            },
        )
        .optional()
        .context("read evaluation_runs row")?;
    let Some((
        source_kind,
        catalog_recipe_id,
        spec_json,
        spec_digest,
        declaration_digest,
        policy_revision,
        policy_configuration_digest,
        approval_receipt_id,
    )) = row
    else {
        return Ok(None);
    };

    let execution_spec: ExecutionSpec =
        serde_json::from_str(&spec_json).context("decode stored execution specification")?;
    let stored_digest = Digest::parse(spec_digest).context("decode stored spec digest")?;
    let recomputed = execution_spec
        .digest()
        .context("recompute stored spec digest")?;
    anyhow::ensure!(
        recomputed == stored_digest,
        "stored execution specification for `{optimizer_run_id}` does not match its recorded \
         digest (recorded {stored_digest}, recomputed {recomputed}); refusing to present a \
         rewritten specification as the approved one"
    );

    let recipe_source_kind = match source_kind.as_str() {
        "inline" => RecipeSourceKind::Inline,
        "catalog" => RecipeSourceKind::Catalog,
        other => anyhow::bail!("unknown recipe_source_kind `{other}`"),
    };

    Ok(Some(EvaluationRunRecord {
        optimizer_run_id: optimizer_run_id.to_string(),
        recipe_source_kind,
        catalog_recipe_id: catalog_recipe_id.map(RecipeId::new).transpose()?,
        execution_spec,
        execution_spec_digest: stored_digest,
        container_declaration_digest: DeclarationDigest::new(declaration_digest)?,
        policy_revision: PolicyRevision::new(policy_revision)?,
        policy_configuration_digest: Digest::parse(policy_configuration_digest)?,
        approval_receipt_id: ApprovalReceiptId::new(approval_receipt_id)?,
    }))
}

/// Persist per-rollout state so a restart restores real progress rather than
/// re-deriving it from a completed count.
pub fn save_run_progress(
    conn: &Connection,
    optimizer_run_id: &str,
    progress: &RunProgress,
) -> Result<()> {
    conn.execute(
        "UPDATE evaluation_runs SET run_state = ?2, credential_revocation_confirmed = ?3
         WHERE optimizer_run_id = ?1",
        params![
            optimizer_run_id,
            progress.state.as_str(),
            progress.credential_revocation_confirmed as i64,
        ],
    )
    .context("update evaluation_runs progress")?;
    for (index, record) in &progress.rollouts {
        let state = record.state.map(|RolloutStateHolder(state)| state);
        conn.execute(
            "INSERT INTO evaluation_rollouts (
                optimizer_run_id, rollout_index, rollout_state, rollout_id,
                reward, trace_ref, cost_micros, total_tokens, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(optimizer_run_id, rollout_index) DO UPDATE SET
                rollout_state = excluded.rollout_state,
                rollout_id = excluded.rollout_id,
                reward = excluded.reward,
                trace_ref = excluded.trace_ref,
                cost_micros = excluded.cost_micros,
                total_tokens = excluded.total_tokens,
                updated_at = excluded.updated_at",
            params![
                optimizer_run_id,
                *index as i64,
                state.map(RolloutState::as_str),
                record.rollout_id.as_ref().map(|value| value.as_str()),
                record.reward,
                record.trace_ref.as_deref(),
                record.cost_micros.map(|value| value as i64),
                record.total_tokens.map(|value| value as i64),
                chrono::Utc::now().to_rfc3339(),
            ],
        )
        .context("upsert evaluation_rollouts row")?;
    }
    Ok(())
}

/// Restore per-rollout state after a restart.
///
/// A `NULL` column comes back as `None`, never as `0` or `0.0`. That is the
/// whole reason these columns are nullable: a rollout whose reward was never
/// observed must read as "reward missing" after a restart, exactly as it did
/// before one.
pub fn load_run_progress(conn: &Connection, optimizer_run_id: &str) -> Result<Option<RunProgress>> {
    let header = conn
        .query_row(
            "SELECT run_state, credential_revocation_confirmed
             FROM evaluation_runs WHERE optimizer_run_id = ?1",
            [optimizer_run_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                ))
            },
        )
        .optional()
        .context("read evaluation_runs progress header")?;
    let Some((run_state, revoked)) = header else {
        return Ok(None);
    };

    let mut statement = conn.prepare(
        "SELECT rollout_index, rollout_state, rollout_id, reward, trace_ref,
                cost_micros, total_tokens
         FROM evaluation_rollouts WHERE optimizer_run_id = ?1 ORDER BY rollout_index",
    )?;
    let mut rollouts: BTreeMap<u32, RolloutRecord> = BTreeMap::new();
    let rows = statement.query_map([optimizer_run_id], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<f64>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<i64>>(5)?,
            row.get::<_, Option<i64>>(6)?,
        ))
    })?;
    for row in rows {
        let (index, state, rollout_id, reward, trace_ref, cost_micros, total_tokens) = row?;
        rollouts.insert(
            index as u32,
            RolloutRecord {
                state: state.as_deref().and_then(parse_rollout_state).map(RolloutStateHolder),
                rollout_id: rollout_id.map(super::ids::RolloutId::new).transpose()?,
                reward,
                trace_ref,
                cost_micros: cost_micros.map(|value| value as u64),
                total_tokens: total_tokens.map(|value| value as u64),
            },
        );
    }

    Ok(Some(RunProgress {
        // A row written before this column existed reads as `Draft`, which is
        // the only state that claims nothing about what happened.
        state: run_state
            .as_deref()
            .and_then(parse_run_state)
            .unwrap_or(RunState::Draft),
        rollouts,
        credential_revocation_confirmed: revoked == Some(1),
    }))
}

fn parse_run_state(value: &str) -> Option<RunState> {
    serde_json::from_value(serde_json::Value::String(value.to_string())).ok()
}

fn parse_rollout_state(value: &str) -> Option<RolloutState> {
    serde_json::from_value(serde_json::Value::String(value.to_string())).ok()
}

/// Record an admission that has not yet been approved, so a draft survives a
/// restart and the operator is not asked to reconstruct it from memory.
pub fn stage_admissible(
    conn: &Connection,
    optimizer_run_id: &str,
    admissible: &AdmissibleExecutionSpec,
) -> Result<()> {
    let spec_json =
        serde_json::to_string(admissible.spec()).context("serialize execution specification")?;
    let spec = admissible.spec();
    conn.execute(
        "INSERT INTO evaluation_run_drafts (
            optimizer_run_id, recipe_source_kind, catalog_recipe_id,
            execution_spec_json, execution_spec_digest, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT(optimizer_run_id) DO UPDATE SET
            recipe_source_kind = excluded.recipe_source_kind,
            catalog_recipe_id = excluded.catalog_recipe_id,
            execution_spec_json = excluded.execution_spec_json,
            execution_spec_digest = excluded.execution_spec_digest",
        params![
            optimizer_run_id,
            spec.source_kind.as_str(),
            spec.catalog_recipe_id.as_ref().map(RecipeId::as_str),
            spec_json,
            admissible.digest().as_str(),
            chrono::Utc::now().to_rfc3339(),
        ],
    )
    .context("upsert evaluation_run_drafts row")?;
    Ok(())
}
