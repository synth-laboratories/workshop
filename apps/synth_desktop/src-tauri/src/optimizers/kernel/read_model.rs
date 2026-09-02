//! The shared optimizer read model: bounded summary, durable paged
//! collections, and reducer checkpoints.
//!
//! The append-only journal stays the audit authority and the algorithm
//! projection stays the reducer's truth. Neither is the ordinary UI read.
//! What a live surface reads is:
//!
//!   · one **bounded summary** per run — algorithm-neutral, byte-budgeted,
//!     conditional on projection revision;
//!   · **collections** — candidates, rollouts, evaluations, metric points,
//!     proposer calls, artifacts, evidence refs — as keyset-paged rows with a
//!     stable identity, ordinal, sequence, and revision;
//!   · **historical projections** at an arbitrary sequence, served from the
//!     nearest reducer checkpoint plus a short suffix fold, never from a
//!     client-side reduction of the whole journal.
//!
//! Rows for the projection-derived collections are materialized inside the
//! same SQLite transaction that upserts the projection (`persist::
//! upsert_projection` → [`sync_collection_rows`]), so a reader that observes
//! projection revision *n* observes the rows of revision *n*, never a
//! half-written set. Artifacts and evidence refs have their own durable
//! tables and are paged from those in the same read transaction as the
//! summary.

use std::collections::BTreeMap;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::algorithm::{AlgorithmProjection, AlgorithmResult};
use super::commit::RunKernelState;
use super::evidence::{EvidenceRef, UsageCompleteness};
use super::types::{
    AlgorithmKind, EvidenceCompleteness, ExecutionPlacement, RunCondition, RunLifecycle, RunPhase,
    TerminalKind, TerminalReason, WorkItemLifecycle,
};
use super::view::RunViewContext;
use super::work::WorkSummary;
use crate::optimizers::models::{OptimizerExecutionBinding, OptimizerRunRecord};

pub const RUN_SUMMARY_SCHEMA_VERSION: &str = "optimizer_run_summary.v1";
pub const COLLECTION_ROW_SCHEMA_VERSION: &str = "optimizer_collection_row.v1";
pub const HISTORICAL_PROJECTION_SCHEMA_VERSION: &str = "optimizer_historical_projection.v1";

/// Serialized-size budget for the primary summary. A ceiling, not a target.
pub const SUMMARY_BYTE_BUDGET: usize = 64 * 1024;
/// A page never carries more rows than this, whatever the caller asks for.
pub const COLLECTION_PAGE_MAX_ROWS: u32 = 100;
/// A page stops early once its rows serialize past this many bytes.
pub const COLLECTION_PAGE_MAX_BYTES: usize = 512 * 1024;
/// Default page size when the caller sends none. There is no "all rows".
pub const COLLECTION_PAGE_DEFAULT_ROWS: u32 = 50;
/// Checkpoint at least this often in events while the state is small.
pub const CHECKPOINT_EVENT_INTERVAL: u64 = 500;
/// Each additional band of serialized state bytes stretches the interval.
pub const CHECKPOINT_BYTE_BAND: usize = 256 * 1024;
/// Retained checkpoints per run; older ones are thinned, not the newest.
pub const MAX_CHECKPOINTS_PER_RUN: usize = 64;

// ---------------------------------------------------------------------------
// Collections
// ---------------------------------------------------------------------------

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum RunCollection {
    Candidates,
    Rollouts,
    Evaluations,
    MetricPoints,
    ProposerCalls,
    Artifacts,
    EvidenceRefs,
}

impl RunCollection {
    pub const ALL: [Self; 7] = [
        Self::Candidates,
        Self::Rollouts,
        Self::Evaluations,
        Self::MetricPoints,
        Self::ProposerCalls,
        Self::Artifacts,
        Self::EvidenceRefs,
    ];

    /// Collections materialized from the algorithm projection at fold time.
    pub const PROJECTED: [Self; 5] = [
        Self::Candidates,
        Self::Rollouts,
        Self::Evaluations,
        Self::MetricPoints,
        Self::ProposerCalls,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Candidates => "candidates",
            Self::Rollouts => "rollouts",
            Self::Evaluations => "evaluations",
            Self::MetricPoints => "metric_points",
            Self::ProposerCalls => "proposer_calls",
            Self::Artifacts => "artifacts",
            Self::EvidenceRefs => "evidence_refs",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|item| item.as_str() == value)
    }
}

/// Whether an algorithm updates existing rows in this collection. This is a
/// property of the projection/collection pair, not of the collection name:
/// GEPA evaluations are terminal append-only facts, while Eval work items and
/// training checkpoint evaluations advance in place.
fn collection_is_mutable(projection: &AlgorithmProjection, collection: RunCollection) -> bool {
    matches!(
        (projection, collection),
        (_, RunCollection::Candidates)
            | (
                AlgorithmProjection::Eval(_),
                RunCollection::Rollouts | RunCollection::Evaluations
            )
            | (
                AlgorithmProjection::Sft(_),
                RunCollection::Evaluations | RunCollection::MetricPoints
            )
            | (
                AlgorithmProjection::Cispo(_),
                RunCollection::Rollouts | RunCollection::Evaluations | RunCollection::MetricPoints
            )
            | (
                AlgorithmProjection::GoEx(_),
                RunCollection::Rollouts | RunCollection::ProposerCalls
            )
    )
}

/// One row of a durable collection. The common envelope is deliberately
/// small; algorithm-specific detail lives in `details` under its own version.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RunCollectionRow {
    pub schema_version: String,
    pub run_id: String,
    pub algorithm: AlgorithmKind,
    pub collection: RunCollection,
    pub item_id: String,
    /// Position in the collection's append order. Keyset cursor; stable
    /// across concurrent appends because appends only ever add higher ones.
    #[specta(type = specta_typescript::Number)]
    pub ordinal: u64,
    /// Aggregate sequence the projection had reached when this row was
    /// written or last changed.
    #[specta(type = specta_typescript::Number)]
    pub sequence: u64,
    /// Projection revision that wrote or last changed this row.
    #[specta(type = specta_typescript::Number)]
    pub revision: u64,
    pub kind: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub score: Option<f64>,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub status: Option<String>,
    pub details_version: String,
    /// True only on a collection page when this row's detail alone exceeds
    /// the page byte budget. The common envelope remains visible; callers
    /// fetch the full detail through `run_collection_item` on selection.
    #[serde(default)]
    pub details_deferred: bool,
    #[specta(type = specta_typescript::Number)]
    pub details_bytes: u64,
    #[specta(type = specta_typescript::Unknown)]
    pub details: Value,
}

/// What an algorithm projection contributes per row before the persistence
/// layer stamps run identity, ordinal, sequence, and revision on it.
#[derive(Clone, Debug, PartialEq)]
pub struct RowSeed {
    pub item_id: String,
    pub kind: String,
    pub label: Option<String>,
    pub parent_id: Option<String>,
    pub score: Option<f64>,
    pub cost_usd: Option<f64>,
    pub status: Option<String>,
    pub details_version: String,
    pub details: Value,
}

impl RowSeed {
    fn new(item_id: impl Into<String>, kind: &str, details_version: &str, details: Value) -> Self {
        Self {
            item_id: item_id.into(),
            kind: kind.to_string(),
            label: None,
            parent_id: None,
            score: None,
            cost_usd: None,
            status: None,
            details_version: details_version.to_string(),
            details,
        }
    }
}

/// How many rows the projection currently holds for `collection`.
pub fn collection_len(projection: &AlgorithmProjection, collection: RunCollection) -> usize {
    match (projection, collection) {
        (AlgorithmProjection::Gepa(p), RunCollection::Candidates) => p.candidate_order.len(),
        (AlgorithmProjection::Gepa(p), RunCollection::Rollouts | RunCollection::Evaluations) => {
            p.evaluations.len()
        }
        (AlgorithmProjection::Gepa(p), RunCollection::ProposerCalls) => p.proposer_calls.len(),
        (AlgorithmProjection::Eval(p), RunCollection::Candidates) => p.candidates.len(),
        (AlgorithmProjection::Eval(p), RunCollection::Rollouts) => p.evidence_ledger.len(),
        (AlgorithmProjection::Eval(p), RunCollection::Evaluations) => {
            p.trials.len() + p.scorecards.len()
        }
        (AlgorithmProjection::GoEx(p), RunCollection::Candidates) => p.candidates.len(),
        (AlgorithmProjection::GoEx(p), RunCollection::Rollouts) => p.child_rollouts.len(),
        (AlgorithmProjection::GoEx(p), RunCollection::Evaluations) => p.child_eval_run_ids.len(),
        (AlgorithmProjection::GoEx(p), RunCollection::ProposerCalls) => p.proposer_calls.len(),
        (AlgorithmProjection::Sft(p), RunCollection::Evaluations) => p.evaluations.len(),
        (AlgorithmProjection::Sft(p), RunCollection::MetricPoints) => p.metrics.points.len(),
        (AlgorithmProjection::Sft(p), RunCollection::Candidates) => {
            p.checkpoints.len() + p.curation_candidates.len()
        }
        (AlgorithmProjection::Cispo(p), RunCollection::Evaluations) => p.evaluations.len(),
        (AlgorithmProjection::Cispo(p), RunCollection::MetricPoints) => p.metrics.points.len(),
        (AlgorithmProjection::Cispo(p), RunCollection::Candidates) => p.checkpoints.len(),
        (AlgorithmProjection::Cispo(p), RunCollection::Rollouts) => p.work_items.len(),
        _ => 0,
    }
}

/// Rows `[from, to)` of `collection`, in append order. Only the requested
/// window is built, so syncing a tail costs the tail rather than the history.
pub fn collection_rows(
    projection: &AlgorithmProjection,
    collection: RunCollection,
    from: usize,
    to: usize,
) -> Vec<RowSeed> {
    let len = collection_len(projection, collection);
    let to = to.min(len);
    if from >= to {
        return Vec::new();
    }
    let range = from..to;
    match (projection, collection) {
        (AlgorithmProjection::Gepa(p), RunCollection::Candidates) => range
            .filter_map(|index| {
                let id = p.candidate_order.get(index)?;
                let candidate = p.candidates.get(id)?;
                let mut seed = RowSeed::new(
                    id.clone(),
                    "gepa_candidate",
                    "gepa_candidate.v1",
                    serde_json::to_value(candidate).unwrap_or(Value::Null),
                );
                seed.label = candidate.source.clone();
                seed.parent_id = candidate.parent_id.clone();
                seed.score = candidate
                    .heldout_reward
                    .or(candidate.train_reward)
                    .or(candidate.minibatch_reward);
                seed.status = candidate
                    .gate_accepted
                    .map(|accepted| if accepted { "accepted" } else { "rejected" }.to_string());
                Some(seed)
            })
            .collect(),
        (AlgorithmProjection::Gepa(p), RunCollection::Rollouts | RunCollection::Evaluations) => {
            range
                .filter_map(|index| {
                    let evaluation = p.evaluations.get(index)?;
                    let mut seed = RowSeed::new(
                        evaluation.id.clone(),
                        "gepa_evaluation",
                        "gepa_evaluation.v1",
                        serde_json::to_value(evaluation).unwrap_or(Value::Null),
                    );
                    seed.label = evaluation.stage.clone();
                    seed.parent_id = evaluation.candidate_id.clone();
                    seed.score = evaluation.reward;
                    seed.cost_usd = evaluation.cost_usd;
                    seed.status = Some(
                        if evaluation.reward.is_some() {
                            "scored"
                        } else {
                            "failed"
                        }
                        .into(),
                    );
                    Some(seed)
                })
                .collect()
        }
        (AlgorithmProjection::Gepa(p), RunCollection::ProposerCalls) => range
            .filter_map(|index| {
                let call = p.proposer_calls.get(index)?;
                let mut seed = RowSeed::new(
                    format!("proposer:{}:{index}", call.generation),
                    "gepa_proposer_call",
                    "gepa_proposer_call.v1",
                    serde_json::to_value(call).unwrap_or(Value::Null),
                );
                seed.label = call.model.clone();
                seed.cost_usd = call.cost_usd;
                seed.score = Some(call.proposal_count as f64);
                Some(seed)
            })
            .collect(),
        (AlgorithmProjection::Eval(p), RunCollection::Candidates) => range
            .filter_map(|index| {
                let id = p.candidates.get(index)?;
                let scorecards = p
                    .scorecards
                    .iter()
                    .filter(|scorecard| scorecard.candidate_id == *id)
                    .collect::<Vec<_>>();
                let latest = scorecards.last().copied();
                let mut seed = RowSeed::new(
                    id.clone(),
                    "eval_candidate",
                    "eval_candidate.v2",
                    json!({ "id": id, "scorecards": scorecards }),
                );
                seed.label = latest.and_then(|row| row.label.clone());
                seed.score = latest.and_then(|row| row.score);
                seed.cost_usd = latest.and_then(|row| row.cost_usd);
                seed.status = latest.and_then(|row| row.status.clone());
                Some(seed)
            })
            .collect(),
        (AlgorithmProjection::Eval(p), RunCollection::Rollouts) => range
            .filter_map(|index| {
                let entry = p.evidence_ledger.get(index)?;
                let trial = p.trials.iter().find(|trial| trial.id == entry.work_item_id);
                let mut seed = RowSeed::new(
                    entry.work_item_id.clone(),
                    "eval_rollout",
                    "eval_rollout_evidence.v2",
                    json!({ "evidence": entry, "trial": trial }),
                );
                seed.label = trial
                    .and_then(|row| row.stage.clone())
                    .or_else(|| entry.rollout_id.clone());
                seed.parent_id = trial
                    .and_then(|row| row.candidate_id.clone())
                    .or_else(|| entry.trial_id.clone());
                seed.score = trial.and_then(|row| row.reward);
                seed.status = Some(format!("{:?}", entry.state).to_lowercase());
                Some(seed)
            })
            .collect(),
        (AlgorithmProjection::Eval(p), RunCollection::Evaluations) => range
            .filter_map(|index| {
                if let Some(trial) = p.trials.get(index) {
                    let mut seed = RowSeed::new(
                        trial.id.clone(),
                        "eval_trial",
                        "eval_trial.v2",
                        serde_json::to_value(trial).unwrap_or(Value::Null),
                    );
                    seed.label = trial.stage.clone();
                    seed.parent_id = trial.candidate_id.clone();
                    seed.score = trial.reward;
                    seed.status = Some(trial.status.clone());
                    return Some(seed);
                }
                let scorecard = p.scorecards.get(index.checked_sub(p.trials.len())?)?;
                let mut seed = RowSeed::new(
                    scorecard.id.clone(),
                    "eval_scorecard",
                    "eval_scorecard.v1",
                    serde_json::to_value(scorecard).unwrap_or(Value::Null),
                );
                seed.label = scorecard.stage.clone();
                seed.parent_id = Some(scorecard.candidate_id.clone());
                seed.score = scorecard.score;
                seed.cost_usd = scorecard.cost_usd;
                seed.status = scorecard.status.clone();
                Some(seed)
            })
            .collect(),
        (AlgorithmProjection::GoEx(p), RunCollection::Candidates) => range
            .filter_map(|index| {
                let candidate = p.candidates.get(index)?;
                let mut seed = RowSeed::new(
                    candidate.id.clone(),
                    "go_ex_candidate",
                    "go_ex_candidate.v2",
                    serde_json::to_value(candidate).unwrap_or(Value::Null),
                );
                seed.parent_id = candidate.parent_id.clone();
                seed.score = candidate.score;
                seed.status = candidate.status.clone();
                Some(seed)
            })
            .collect(),
        (AlgorithmProjection::GoEx(p), RunCollection::Rollouts) => range
            .filter_map(|index| {
                let rollout = p.child_rollouts.get(index)?;
                let mut seed = RowSeed::new(
                    rollout.id.clone(),
                    "go_ex_child_rollout",
                    "go_ex_child_rollout.v1",
                    serde_json::to_value(rollout).unwrap_or(Value::Null),
                );
                seed.parent_id = rollout.candidate_id.clone();
                seed.score = rollout.reward;
                seed.cost_usd = rollout.cost_usd;
                seed.status = rollout.status.clone();
                Some(seed)
            })
            .collect(),
        (AlgorithmProjection::GoEx(p), RunCollection::Evaluations) => range
            .filter_map(|index| {
                let id = p.child_eval_run_ids.get(index)?;
                Some(RowSeed::new(
                    id.clone(),
                    "go_ex_child_eval",
                    "child_eval_run.v1",
                    json!({ "optimizerRunId": id }),
                ))
            })
            .collect(),
        (AlgorithmProjection::GoEx(p), RunCollection::ProposerCalls) => range
            .filter_map(|index| {
                let call = p.proposer_calls.get(index)?;
                let mut seed = RowSeed::new(
                    call.id.clone(),
                    "go_ex_proposer_call",
                    "go_ex_proposer_call.v1",
                    serde_json::to_value(call).unwrap_or(Value::Null),
                );
                seed.label = call.model.clone();
                seed.cost_usd = call.cost_usd;
                seed.status = Some(call.status.clone());
                Some(seed)
            })
            .collect(),
        (AlgorithmProjection::Sft(p), RunCollection::Evaluations) => range
            .filter_map(|index| training_evaluation_row(p.evaluations.get(index)?))
            .collect(),
        (AlgorithmProjection::Sft(p), RunCollection::MetricPoints) => range
            .filter_map(|index| metric_point_row(p.metrics.points.get(index)?))
            .collect(),
        (AlgorithmProjection::Sft(p), RunCollection::Candidates) => range
            .filter_map(|index| {
                if let Some(checkpoint) = p.checkpoints.get(index) {
                    return checkpoint_row(checkpoint, p.selected_checkpoint_id.as_deref());
                }
                let curation_index = index.checked_sub(p.checkpoints.len())?;
                let candidate = p.curation_candidates.get(curation_index)?;
                let id = candidate
                    .get("id")
                    .or_else(|| candidate.get("candidate_id"))
                    .or_else(|| candidate.get("rollout_id"))
                    .or_else(|| candidate.get("trace_id"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("curation:{curation_index}"));
                let mut seed = RowSeed::new(
                    id,
                    "sft_curation_candidate",
                    "sft_curation_candidate.v1",
                    candidate.clone(),
                );
                seed.score = candidate
                    .get("score")
                    .or_else(|| candidate.get("reward"))
                    .and_then(Value::as_f64);
                seed.status = candidate
                    .get("status")
                    .or_else(|| candidate.get("decision"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                Some(seed)
            })
            .collect(),
        (AlgorithmProjection::Cispo(p), RunCollection::Evaluations) => range
            .filter_map(|index| training_evaluation_row(p.evaluations.get(index)?))
            .collect(),
        (AlgorithmProjection::Cispo(p), RunCollection::MetricPoints) => range
            .filter_map(|index| metric_point_row(p.metrics.points.get(index)?))
            .collect(),
        (AlgorithmProjection::Cispo(p), RunCollection::Candidates) => range
            .filter_map(|index| {
                checkpoint_row(p.checkpoints.get(index)?, p.policy_checkpoint_id.as_deref())
            })
            .collect(),
        (AlgorithmProjection::Cispo(p), RunCollection::Rollouts) => range
            .filter_map(|index| work_item_row(p.work_items.get(index)?, "cispo_rollout_group"))
            .collect(),
        _ => Vec::new(),
    }
}

fn work_item_row(item: &super::work::WorkItem, kind: &str) -> Option<RowSeed> {
    let mut seed = RowSeed::new(
        item.work_item_id.clone(),
        kind,
        "work_item.v1",
        json!({
            "workItemId": item.work_item_id,
            "kind": item.kind,
            "lifecycle": item.lifecycle,
            "terminal": item.terminal,
            "externalRef": item.external_ref,
        }),
    );
    seed.status = Some(
        item.terminal
            .map(|kind| kind.as_str().to_string())
            .unwrap_or_else(|| item.lifecycle.as_str().to_string()),
    );
    Some(seed)
}

fn training_evaluation_row(
    evaluation: &super::algorithms::training::TrainingEvaluationSummary,
) -> Option<RowSeed> {
    let mut seed = RowSeed::new(
        evaluation.id.clone(),
        "training_evaluation",
        "training_evaluation.v1",
        serde_json::to_value(evaluation).unwrap_or(Value::Null),
    );
    seed.label = evaluation.phase.clone();
    seed.parent_id = evaluation.checkpoint_id.clone();
    seed.score = evaluation.score;
    seed.status = evaluation.status.clone();
    Some(seed)
}

fn metric_point_row(point: &super::algorithms::training::TrainingMetricPoint) -> Option<RowSeed> {
    let mut seed = RowSeed::new(
        format!("step:{}", point.step),
        "training_metric_point",
        "training_metric_point.v1",
        serde_json::to_value(point).unwrap_or(Value::Null),
    );
    seed.score = point.loss;
    Some(seed)
}

fn checkpoint_row(id: &str, selected: Option<&str>) -> Option<RowSeed> {
    let mut seed = RowSeed::new(
        id,
        "training_checkpoint",
        "training_checkpoint.v1",
        json!({ "checkpointId": id }),
    );
    seed.status = Some(
        if selected == Some(id) {
            "selected"
        } else {
            "ready"
        }
        .into(),
    );
    Some(seed)
}

// ---------------------------------------------------------------------------
// Row persistence (inside the projection's own transaction)
// ---------------------------------------------------------------------------

fn insert_row(
    conn: &Connection,
    state: &RunKernelState,
    collection: RunCollection,
    ordinal: u64,
    seed: &RowSeed,
) -> Result<()> {
    conn.execute(
        "INSERT INTO optimizer_run_collection_rows(
            optimizer_run_id, collection, item_id, ordinal, sequence, revision,
            kind, label, parent_id, score, cost_usd, status, details_version, details_json, updated_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, datetime('now'))
         ON CONFLICT(optimizer_run_id, collection, item_id) DO UPDATE SET
            ordinal=excluded.ordinal,
            sequence=excluded.sequence,
            revision=excluded.revision,
            kind=excluded.kind,
            label=excluded.label,
            parent_id=excluded.parent_id,
            score=excluded.score,
            cost_usd=excluded.cost_usd,
            status=excluded.status,
            details_version=excluded.details_version,
            details_json=excluded.details_json,
            updated_at=excluded.updated_at",
        params![
            state.run_id,
            collection.as_str(),
            seed.item_id,
            ordinal as i64,
            state.aggregate_sequence as i64,
            state.projection_revision as i64,
            seed.kind,
            seed.label,
            seed.parent_id,
            seed.score,
            seed.cost_usd,
            seed.status,
            seed.details_version,
            serde_json::to_string(&seed.details).context("serialize collection row details")?,
        ],
    )
    .with_context(|| {
        format!(
            "insert {} row {} for {}",
            collection.as_str(),
            seed.item_id,
            state.run_id
        )
    })?;
    Ok(())
}

fn stored_row_count(conn: &Connection, run_id: &str, collection: RunCollection) -> Result<u64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM optimizer_run_collection_rows
         WHERE optimizer_run_id = ?1 AND collection = ?2",
        params![run_id, collection.as_str()],
        |row| row.get(0),
    )?;
    Ok(count.max(0) as u64)
}

fn clear_rows(conn: &Connection, run_id: &str, collection: RunCollection) -> Result<()> {
    conn.execute(
        "DELETE FROM optimizer_run_collection_rows WHERE optimizer_run_id = ?1 AND collection = ?2",
        params![run_id, collection.as_str()],
    )?;
    Ok(())
}

/// Bring the materialized rows in line with `state.projection`.
///
/// Append-only collections sync their tail: rows already stored are trusted
/// and only `[stored, len)` is built. A projection that *shrank* — a reducer
/// upgrade replaying to a different shape, or a repair — rebuilds the
/// collection from zero. Mutable collections are bounded by construction
/// (candidates, checkpoints), so every row is compared and only changed rows
/// are rewritten; an untouched row keeps its original revision, which is what
/// lets a reader tell "changed since revision n" apart from "resent".
pub fn sync_collection_rows(conn: &Connection, state: &RunKernelState) -> Result<()> {
    for collection in RunCollection::PROJECTED {
        let len = collection_len(&state.projection, collection) as u64;
        let stored = stored_row_count(conn, &state.run_id, collection)?;
        if collection_is_mutable(&state.projection, collection) {
            if stored > len {
                clear_rows(conn, &state.run_id, collection)?;
            }
            let seeds = collection_rows(&state.projection, collection, 0, len as usize);
            type StoredSeed = (
                u64,
                String,
                Option<String>,
                Option<String>,
                Option<f64>,
                Option<f64>,
                Option<String>,
                String,
                String,
            );
            let mut existing: BTreeMap<String, StoredSeed> = BTreeMap::new();
            let mut statement = conn.prepare(
                "SELECT item_id, ordinal, kind, label, parent_id, score, cost_usd,
                        status, details_version, details_json
                 FROM optimizer_run_collection_rows
                 WHERE optimizer_run_id = ?1 AND collection = ?2",
            )?;
            let rows = statement.query_map(params![state.run_id, collection.as_str()], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?.max(0) as u64,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<f64>>(5)?,
                    row.get::<_, Option<f64>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, String>(9)?,
                ))
            })?;
            for row in rows {
                let (
                    item_id,
                    ordinal,
                    kind,
                    label,
                    parent_id,
                    score,
                    cost_usd,
                    status,
                    details_version,
                    details,
                ) = row?;
                existing.insert(
                    item_id,
                    (
                        ordinal,
                        kind,
                        label,
                        parent_id,
                        score,
                        cost_usd,
                        status,
                        details_version,
                        details,
                    ),
                );
            }
            drop(statement);
            let live_ids = seeds
                .iter()
                .map(|seed| seed.item_id.as_str())
                .collect::<std::collections::BTreeSet<_>>();
            for stale_id in existing
                .keys()
                .filter(|item_id| !live_ids.contains(item_id.as_str()))
            {
                conn.execute(
                    "DELETE FROM optimizer_run_collection_rows
                     WHERE optimizer_run_id = ?1 AND collection = ?2 AND item_id = ?3",
                    params![state.run_id, collection.as_str(), stale_id],
                )?;
            }
            for (ordinal, seed) in seeds.iter().enumerate() {
                let details = serde_json::to_string(&seed.details)?;
                let unchanged = existing.get(&seed.item_id).is_some_and(|stored| {
                    stored.0 == ordinal as u64
                        && stored.1 == seed.kind
                        && stored.2 == seed.label
                        && stored.3 == seed.parent_id
                        && stored.4 == seed.score
                        && stored.5 == seed.cost_usd
                        && stored.6 == seed.status
                        && stored.7 == seed.details_version
                        && stored.8 == details
                });
                if !unchanged {
                    insert_row(conn, state, collection, ordinal as u64, seed)?;
                }
            }
            continue;
        }
        let from = if stored > len {
            clear_rows(conn, &state.run_id, collection)?;
            0
        } else {
            stored
        };
        if from < len {
            let seeds = collection_rows(&state.projection, collection, from as usize, len as usize);
            for (offset, seed) in seeds.iter().enumerate() {
                insert_row(conn, state, collection, from + offset as u64, seed)?;
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Queries
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RunCollectionFilter {
    /// Rows whose `parent_id` equals this (candidate for evaluations, trial
    /// for rollouts, checkpoint for training evaluations).
    #[serde(default)]
    pub parent_id: Option<String>,
    /// Rows whose `label` equals this (GEPA stage, training phase, model).
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    /// Rows written or changed after this projection revision.
    #[serde(default)]
    #[specta(type = Option<specta_typescript::Number>)]
    pub changed_after_revision: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RunCollectionQuery {
    /// Opaque keyset cursor from the previous page's `next_cursor`.
    #[serde(default)]
    pub cursor: Option<String>,
    /// Rows requested. Clamped to [`COLLECTION_PAGE_MAX_ROWS`]; absent means
    /// [`COLLECTION_PAGE_DEFAULT_ROWS`].
    #[serde(default)]
    #[specta(type = Option<specta_typescript::Number>)]
    pub limit: Option<u32>,
    #[serde(default)]
    pub filter: Option<RunCollectionFilter>,
    /// Newest ordinals first. Default is append order.
    #[serde(default)]
    pub descending: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RunCollectionPage {
    pub run_id: String,
    pub collection: RunCollection,
    pub rows: Vec<RunCollectionRow>,
    /// Present when more rows match; absent means the page reached the end.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// Rows matching the filter across all pages, as of this read.
    #[specta(type = specta_typescript::Number)]
    pub total: u64,
    #[specta(type = specta_typescript::Number)]
    pub projection_revision: u64,
    #[specta(type = specta_typescript::Number)]
    pub as_of_sequence: u64,
    /// The page ended on the byte budget before reaching `limit`.
    pub truncated_by_bytes: bool,
    /// Rows requested after clamping.
    #[specta(type = specta_typescript::Number)]
    pub limit: u32,
}

fn decode_cursor(cursor: Option<&str>) -> Result<Option<u64>> {
    match cursor {
        None | Some("") => Ok(None),
        Some(raw) => raw
            .strip_prefix("ord:")
            .and_then(|value| value.parse::<u64>().ok())
            .map(Some)
            .ok_or_else(|| anyhow::anyhow!("invalid collection cursor {raw:?}")),
    }
}

fn encode_cursor(ordinal: u64) -> String {
    format!("ord:{ordinal}")
}

/// Page a projection-derived collection from its materialized rows.
///
/// The page is read in the caller's transaction alongside the projection
/// revision it reports, so rows and revision describe one durable state.
pub fn query_collection_rows(
    conn: &Connection,
    run_id: &str,
    algorithm: AlgorithmKind,
    collection: RunCollection,
    query: &RunCollectionQuery,
    projection_revision: u64,
    as_of_sequence: u64,
) -> Result<RunCollectionPage> {
    let limit = query
        .limit
        .unwrap_or(COLLECTION_PAGE_DEFAULT_ROWS)
        .clamp(1, COLLECTION_PAGE_MAX_ROWS);
    let filter = query.filter.clone().unwrap_or_default();
    let after = decode_cursor(query.cursor.as_deref())?;
    let order = if query.descending { "DESC" } else { "ASC" };
    let cursor_clause = match (after, query.descending) {
        (None, _) => String::new(),
        (Some(_), false) => " AND ordinal > ?7".into(),
        (Some(_), true) => " AND ordinal < ?7".into(),
    };
    let sql = format!(
        "SELECT item_id, ordinal, sequence, revision, kind, label, parent_id, score, cost_usd,
                status, details_version, details_json
         FROM optimizer_run_collection_rows
         WHERE optimizer_run_id = ?1 AND collection = ?2
           AND (?3 IS NULL OR parent_id = ?3)
           AND (?4 IS NULL OR label = ?4)
           AND (?5 IS NULL OR status = ?5)
           AND (?6 IS NULL OR kind = ?6)
           AND (?8 IS NULL OR revision > ?8)
           {cursor_clause}
         ORDER BY ordinal {order}
         LIMIT ?9"
    );
    let count_sql = "SELECT COUNT(*) FROM optimizer_run_collection_rows
         WHERE optimizer_run_id = ?1 AND collection = ?2
           AND (?3 IS NULL OR parent_id = ?3)
           AND (?4 IS NULL OR label = ?4)
           AND (?5 IS NULL OR status = ?5)
           AND (?6 IS NULL OR kind = ?6)
           AND (?8 IS NULL OR revision > ?8)";
    let changed_after = filter.changed_after_revision.map(|value| value as i64);
    let total: i64 = conn.query_row(
        count_sql,
        params![
            run_id,
            collection.as_str(),
            filter.parent_id,
            filter.label,
            filter.status,
            filter.kind,
            after.map(|value| value as i64),
            changed_after,
        ],
        |row| row.get(0),
    )?;
    let mut statement = conn.prepare(&sql)?;
    // One extra row tells us whether a next page exists without a second
    // count query per page.
    let rows = statement.query_map(
        params![
            run_id,
            collection.as_str(),
            filter.parent_id,
            filter.label,
            filter.status,
            filter.kind,
            after.map(|value| value as i64),
            changed_after,
            i64::from(limit) + 1,
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?.max(0) as u64,
                row.get::<_, i64>(2)?.max(0) as u64,
                row.get::<_, i64>(3)?.max(0) as u64,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, Option<f64>>(7)?,
                row.get::<_, Option<f64>>(8)?,
                row.get::<_, Option<String>>(9)?,
                row.get::<_, String>(10)?,
                row.get::<_, String>(11)?,
            ))
        },
    )?;
    let mut page_rows = Vec::new();
    let mut bytes = 0usize;
    let mut truncated_by_bytes = false;
    let mut has_more = false;
    for row in rows {
        let (
            item_id,
            ordinal,
            sequence,
            revision,
            kind,
            label,
            parent_id,
            score,
            cost_usd,
            status,
            details_version,
            details_json,
        ) = row?;
        if page_rows.len() as u32 >= limit {
            has_more = true;
            break;
        }
        // Budget on the stored detail bytes plus a fixed envelope estimate.
        // A single oversized detail is represented by its envelope and must
        // be fetched explicitly through the item endpoint; pages themselves
        // never exceed the advertised byte ceiling.
        let row_bytes = details_json.len() + 256;
        if !page_rows.is_empty() && bytes + row_bytes > COLLECTION_PAGE_MAX_BYTES {
            truncated_by_bytes = true;
            has_more = true;
            break;
        }
        let details_deferred = row_bytes > COLLECTION_PAGE_MAX_BYTES;
        if details_deferred {
            truncated_by_bytes = true;
            bytes += 256;
        } else {
            bytes += row_bytes;
        }
        page_rows.push(RunCollectionRow {
            schema_version: COLLECTION_ROW_SCHEMA_VERSION.into(),
            run_id: run_id.to_string(),
            algorithm,
            collection,
            item_id,
            ordinal,
            sequence,
            revision,
            kind,
            label,
            parent_id,
            score,
            cost_usd,
            status,
            details_version,
            details_deferred,
            details_bytes: details_json.len() as u64,
            details: if details_deferred {
                Value::Null
            } else {
                serde_json::from_str(&details_json).unwrap_or(Value::Null)
            },
        });
    }
    let next_cursor = if has_more {
        page_rows.last().map(|row| encode_cursor(row.ordinal))
    } else {
        None
    };
    Ok(RunCollectionPage {
        run_id: run_id.to_string(),
        collection,
        rows: page_rows,
        next_cursor,
        total: total.max(0) as u64,
        projection_revision,
        as_of_sequence,
        truncated_by_bytes,
        limit,
    })
}

pub fn load_collection_row(
    conn: &Connection,
    run_id: &str,
    algorithm: AlgorithmKind,
    collection: RunCollection,
    item_id: &str,
) -> Result<Option<RunCollectionRow>> {
    conn.query_row(
        "SELECT ordinal, sequence, revision, kind, label, parent_id, score, cost_usd,
                status, details_version, details_json
         FROM optimizer_run_collection_rows
         WHERE optimizer_run_id = ?1 AND collection = ?2 AND item_id = ?3",
        params![run_id, collection.as_str(), item_id],
        |row| {
            let details_json = row.get::<_, String>(10)?;
            Ok(RunCollectionRow {
                schema_version: COLLECTION_ROW_SCHEMA_VERSION.into(),
                run_id: run_id.to_string(),
                algorithm,
                collection,
                item_id: item_id.to_string(),
                ordinal: row.get::<_, i64>(0)?.max(0) as u64,
                sequence: row.get::<_, i64>(1)?.max(0) as u64,
                revision: row.get::<_, i64>(2)?.max(0) as u64,
                kind: row.get(3)?,
                label: row.get(4)?,
                parent_id: row.get(5)?,
                score: row.get(6)?,
                cost_usd: row.get(7)?,
                status: row.get(8)?,
                details_version: row.get(9)?,
                details_deferred: false,
                details_bytes: details_json.len() as u64,
                details: serde_json::from_str::<Value>(&details_json).unwrap_or(Value::Null),
            })
        },
    )
    .optional()
    .context("load optimizer collection row")
}

/// Page an in-memory row list the same way the SQL pager does. Used for the
/// collections that are not materialized (artifacts, evidence refs), which
/// are bounded by their own tables and read in the same transaction.
pub fn page_rows_in_memory(
    run_id: &str,
    collection: RunCollection,
    rows: Vec<RunCollectionRow>,
    query: &RunCollectionQuery,
    projection_revision: u64,
    as_of_sequence: u64,
) -> Result<RunCollectionPage> {
    let limit = query
        .limit
        .unwrap_or(COLLECTION_PAGE_DEFAULT_ROWS)
        .clamp(1, COLLECTION_PAGE_MAX_ROWS);
    let filter = query.filter.clone().unwrap_or_default();
    let after = decode_cursor(query.cursor.as_deref())?;
    let mut matching: Vec<RunCollectionRow> = rows
        .into_iter()
        .filter(|row| {
            filter
                .parent_id
                .as_ref()
                .is_none_or(|value| row.parent_id.as_ref() == Some(value))
                && filter
                    .label
                    .as_ref()
                    .is_none_or(|value| row.label.as_ref() == Some(value))
                && filter
                    .status
                    .as_ref()
                    .is_none_or(|value| row.status.as_ref() == Some(value))
                && filter.kind.as_ref().is_none_or(|value| &row.kind == value)
                && filter
                    .changed_after_revision
                    .is_none_or(|value| row.revision > value)
        })
        .collect();
    let total = matching.len() as u64;
    if query.descending {
        matching.sort_by(|left, right| right.ordinal.cmp(&left.ordinal));
    } else {
        matching.sort_by_key(|row| row.ordinal);
    }
    let mut page_rows = Vec::new();
    let mut bytes = 0usize;
    let mut truncated_by_bytes = false;
    let mut has_more = false;
    for row in matching {
        if let Some(after) = after {
            if (!query.descending && row.ordinal <= after)
                || (query.descending && row.ordinal >= after)
            {
                continue;
            }
        }
        if page_rows.len() as u32 >= limit {
            has_more = true;
            break;
        }
        let details_bytes = serde_json::to_string(&row.details)
            .map(|s| s.len())
            .unwrap_or(0);
        let row_bytes = details_bytes + 256;
        if !page_rows.is_empty() && bytes + row_bytes > COLLECTION_PAGE_MAX_BYTES {
            truncated_by_bytes = true;
            has_more = true;
            break;
        }
        if row_bytes > COLLECTION_PAGE_MAX_BYTES {
            truncated_by_bytes = true;
            bytes += 256;
            page_rows.push(RunCollectionRow {
                details: Value::Null,
                details_deferred: true,
                details_bytes: details_bytes as u64,
                ..row
            });
        } else {
            bytes += row_bytes;
            page_rows.push(RunCollectionRow {
                details_bytes: details_bytes as u64,
                ..row
            });
        }
    }
    let next_cursor = if has_more {
        page_rows.last().map(|row| encode_cursor(row.ordinal))
    } else {
        None
    };
    Ok(RunCollectionPage {
        run_id: run_id.to_string(),
        collection,
        rows: page_rows,
        next_cursor,
        total,
        projection_revision,
        as_of_sequence,
        truncated_by_bytes,
        limit,
    })
}

/// Evidence-ref rows derived from the run's current evidence state.
pub fn evidence_ref_rows(state: &RunKernelState) -> Vec<RunCollectionRow> {
    state
        .evidence_state()
        .refs
        .iter()
        .enumerate()
        .map(|(ordinal, reference)| evidence_ref_row(state, ordinal as u64, reference))
        .collect()
}

fn evidence_ref_row(
    state: &RunKernelState,
    ordinal: u64,
    reference: &EvidenceRef,
) -> RunCollectionRow {
    RunCollectionRow {
        schema_version: COLLECTION_ROW_SCHEMA_VERSION.into(),
        run_id: state.run_id.clone(),
        algorithm: state.algorithm,
        collection: RunCollection::EvidenceRefs,
        item_id: format!("{}:{}", reference.kind, reference.id),
        ordinal,
        sequence: state.aggregate_sequence,
        revision: state.projection_revision,
        kind: reference.kind.clone(),
        label: reference.digest.clone(),
        parent_id: None,
        score: None,
        cost_usd: None,
        status: None,
        details_version: "evidence_ref.v1".into(),
        details_deferred: false,
        details_bytes: 0,
        details: serde_json::to_value(reference).unwrap_or(Value::Null),
    }
}

/// Artifact rows from the durable artifact index.
pub fn artifact_rows(
    state: &RunKernelState,
    artifacts: &[crate::optimizers::models::OptimizerRunArtifact],
) -> Vec<RunCollectionRow> {
    artifacts
        .iter()
        .enumerate()
        .map(|(ordinal, artifact)| RunCollectionRow {
            schema_version: COLLECTION_ROW_SCHEMA_VERSION.into(),
            run_id: state.run_id.clone(),
            algorithm: state.algorithm,
            collection: RunCollection::Artifacts,
            item_id: artifact.artifact_id.clone(),
            ordinal: ordinal as u64,
            sequence: artifact.sequence,
            revision: state.projection_revision,
            kind: artifact.kind.clone(),
            label: artifact.rollout_id.clone(),
            parent_id: artifact.work_item_id.clone(),
            score: None,
            cost_usd: None,
            status: None,
            details_version: artifact.schema_version.clone(),
            details_deferred: false,
            details_bytes: 0,
            details: serde_json::to_value(artifact).unwrap_or(Value::Null),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Bounded summary
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RunConcurrencySummary {
    #[serde(default)]
    #[specta(type = Option<specta_typescript::Number>)]
    pub configured: Option<u64>,
    #[serde(default)]
    #[specta(type = Option<specta_typescript::Number>)]
    pub observed_max: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RunThroughputSummary {
    pub unit: String,
    pub per_minute: f64,
    #[specta(type = specta_typescript::Number)]
    pub measured_over_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RunEvidenceSummary {
    pub completeness: EvidenceCompleteness,
    #[serde(default)]
    pub reason: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub ref_count: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RunTerminalSummary {
    pub kind: TerminalKind,
    #[serde(default)]
    pub reason: Option<TerminalReason>,
    #[specta(type = specta_typescript::Number)]
    pub final_sequence: u64,
    pub sealed_at: String,
    #[serde(default)]
    pub failure_ref: Option<String>,
    pub evidence: RunEvidenceSummary,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RunResultSummary {
    pub schema: String,
    #[serde(default)]
    pub verdict: Option<String>,
    #[serde(default)]
    pub selected_item_id: Option<String>,
    #[serde(default)]
    pub best_score: Option<f64>,
    /// The settled algorithm result. Small by construction: counts, ids,
    /// usage. Never candidate content or rollout detail.
    #[specta(type = specta_typescript::Unknown)]
    pub value: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RunCollectionSummary {
    pub collection: RunCollection,
    #[specta(type = specta_typescript::Number)]
    pub count: u64,
    /// Highest projection revision that wrote or changed a row.
    #[specta(type = specta_typescript::Number)]
    pub latest_revision: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RunSummaryBudget {
    #[specta(type = specta_typescript::Number)]
    pub bytes: u64,
    #[specta(type = specta_typescript::Number)]
    pub limit: u64,
    pub within: bool,
}

/// The algorithm-neutral, byte-budgeted run summary every live surface
/// mounts from. Growing collections are counted here and paged elsewhere.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerRunSummary {
    pub schema_version: String,
    pub run_id: String,
    pub algorithm: AlgorithmKind,
    /// Compatibility status string the run record carries.
    pub status: String,
    pub lifecycle: RunLifecycle,
    #[serde(default)]
    pub phase: Option<RunPhase>,
    pub condition: RunCondition,
    pub placement: ExecutionPlacement,
    #[serde(default)]
    pub terminal: Option<RunTerminalSummary>,
    #[serde(default)]
    pub failure_ref: Option<String>,
    pub spec_id: String,
    pub spec_digest: String,
    pub reducer_version: String,
    #[specta(type = specta_typescript::Number)]
    pub projection_revision: u64,
    #[specta(type = specta_typescript::Number)]
    pub as_of_sequence: u64,
    #[specta(type = specta_typescript::Number)]
    pub tail_cursor: u64,
    pub source: String,
    #[serde(default)]
    pub objective: Option<String>,
    #[serde(default)]
    pub session_ref: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub finished_at: Option<String>,
    #[serde(default)]
    #[specta(type = Option<specta_typescript::Number>)]
    pub elapsed_ms: Option<u64>,
    pub concurrency: RunConcurrencySummary,
    pub work: WorkSummary,
    pub usage: UsageCompleteness,
    /// Whether the cost figure is complete according to the run record.
    pub cost_complete: bool,
    #[serde(default)]
    pub throughput: Option<RunThroughputSummary>,
    #[serde(default)]
    pub result: Option<RunResultSummary>,
    pub collections: Vec<RunCollectionSummary>,
    pub execution_bindings: Vec<OptimizerExecutionBinding>,
    /// Compact setup facts (dataset, container, models) as the projection
    /// reduced them. Never task rows, prompts, or credentials.
    #[specta(type = specta_typescript::Unknown)]
    pub setup: Value,
    /// Latest observed execution shape (workers, job state).
    #[specta(type = specta_typescript::Unknown)]
    pub runtime: Value,
    pub evidence: RunEvidenceSummary,
    #[specta(type = specta_typescript::Number)]
    pub artifact_count: u64,
    #[specta(type = specta_typescript::Number)]
    pub input_ref_count: u64,
    #[specta(type = specta_typescript::Number)]
    pub output_ref_count: u64,
    #[specta(type = specta_typescript::Number)]
    pub visual_ref_count: u64,
    pub budget: RunSummaryBudget,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerRunSummaryEnvelope {
    pub unchanged: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<OptimizerRunSummary>,
    #[specta(type = specta_typescript::Number)]
    pub projection_revision: u64,
    #[specta(type = specta_typescript::Number)]
    pub tail_cursor: u64,
}

fn evidence_summary(state: &super::evidence::EvidenceState) -> RunEvidenceSummary {
    RunEvidenceSummary {
        completeness: state.completeness,
        reason: state.reason.clone(),
        ref_count: state.refs.len() as u64,
    }
}

fn number_at(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| {
        value.get(key).and_then(|found| {
            found
                .as_u64()
                .or_else(|| found.as_f64().map(|f| f.max(0.0) as u64))
        })
    })
}

fn compact_fields(value: &Value, keys: &[&str]) -> Value {
    let Some(source) = value.as_object() else {
        return Value::Null;
    };
    Value::Object(
        keys.iter()
            .filter_map(|key| {
                source
                    .get(*key)
                    .map(|value| ((*key).to_string(), value.clone()))
            })
            .collect(),
    )
}

fn parse_rfc3339_ms(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|parsed| parsed.timestamp_millis())
}

/// Build the summary from a loaded kernel state and its run record.
pub fn summarize(
    state: &RunKernelState,
    run: &OptimizerRunRecord,
    context: &RunViewContext,
    collection_counts: Vec<RunCollectionSummary>,
    tail_cursor: u64,
    now_ms: i64,
) -> OptimizerRunSummary {
    let (setup, runtime, observed_max) = match &state.projection {
        AlgorithmProjection::Gepa(p) => {
            (p.contract.clone(), p.runtime.clone(), p.max_active_workers)
        }
        AlgorithmProjection::Cispo(p) => (
            json!({
                "clipIdentity": p.clip_identity,
                "clipConfig": p.clip_config,
                "warmStartId": p.warm_start_id,
                "groupSize": p.group_size,
                "rewardVariance": p.reward_variance,
                "advantageMean": p.mean_advantage,
                "advantageStd": p.advantage_std,
                "optimizerSteps": p.optimizer_steps
            }),
            Value::Null,
            None,
        ),
        AlgorithmProjection::Sft(p) => (
            json!({
                "datasetDigest": p.dataset_digest,
                "configDigest": p.config_digest,
                "dataset": compact_fields(&p.dataset_summary, &["source", "config", "revision", "digest", "dataset_digest", "row_count", "label_count", "splits"]),
                "curation": compact_fields(&p.curation_summary, &["collected", "considered", "accepted", "rejected", "rejections_by_reason", "seeds_covered", "achievements_covered"]),
                "comparison": compact_fields(&p.comparison_summary, &["split_digest", "seed_manifest_digest", "base_label", "trained_label", "status", "score", "delta"])
            }),
            compact_fields(
                &p.compute_summary,
                &[
                    "backend",
                    "device",
                    "model",
                    "status",
                    "workers",
                    "tokens_per_second",
                ],
            ),
            None,
        ),
        AlgorithmProjection::Eval(p) => (
            compact_fields(
                &p.setup,
                &[
                    "plannedTrials",
                    "planned_trials",
                    "parallelism",
                    "globalCapacity",
                    "global_capacity",
                    "manifestDigest",
                    "manifest_digest",
                    "candidateSetId",
                    "candidate_set_id",
                    "dataset",
                    "container",
                ],
            ),
            json!({ "seedLedger": p.seed_ledger, "selection": p.selection }),
            None,
        ),
        AlgorithmProjection::GoEx(p) => (
            json!({
                "dataEngine": compact_fields(&p.data_engine, &["status", "dataset", "counts", "childEvalRunIds"]),
                "frontier": compact_fields(&p.frontier, &["bestCandidateId", "best_candidate_id", "candidate_frontier"])
            }),
            json!({
                "board": compact_fields(&p.board, &["phase", "previousPhase", "tick", "tick_index", "reason", "status"]),
                "agents": compact_fields(&p.agents, &["status", "counts"]),
                "remoteStatus": p.remote_status
            }),
            None,
        ),
    };
    let configured = number_at(
        &setup,
        &[
            "scale_leases",
            "scaleLeases",
            "maxConcurrency",
            "max_concurrency",
            "concurrency",
        ],
    )
    .or_else(|| {
        number_at(
            &runtime,
            &["maxWorkers", "max_workers", "configuredWorkers"],
        )
    });
    let observed_max =
        observed_max.or_else(|| number_at(&runtime, &["activeWorkers", "active_workers"]));

    let started_ms = run.started_at.as_deref().and_then(parse_rfc3339_ms);
    let finished_ms = run.finished_at.as_deref().and_then(parse_rfc3339_ms);
    let elapsed_ms = started_ms.map(|start| {
        let end = finished_ms.unwrap_or(now_ms);
        (end - start).max(0) as u64
    });
    let work = state.work_summary();
    let throughput = match (elapsed_ms, work.succeeded, work.unit.as_deref()) {
        (Some(ms), Some(done), Some(unit)) if ms > 0 && done > 0 => Some(RunThroughputSummary {
            unit: unit.to_string(),
            per_minute: done as f64 / (ms as f64 / 60_000.0),
            measured_over_ms: ms,
        }),
        _ => None,
    };

    let result = state.projection.settle().ok().map(|settled| {
        let (verdict, selected, best) = match &settled {
            AlgorithmResult::Gepa(result) => (
                Some(format!("{:?}", result.verdict)),
                result.selected_candidate_id.clone(),
                match &state.projection {
                    AlgorithmProjection::Gepa(p) => result
                        .selected_candidate_id
                        .as_ref()
                        .and_then(|id| p.candidates.get(id))
                        .and_then(|candidate| candidate.heldout_reward.or(candidate.train_reward)),
                    _ => None,
                },
            ),
            AlgorithmResult::Eval(result) => (
                Some(format!("{:?}", result.selection)),
                None,
                result.mean_reward,
            ),
            AlgorithmResult::GoEx(result) => (None, result.selected_candidate_id.clone(), None),
            AlgorithmResult::Sft(result) => (
                None,
                result.selected_checkpoint_id.clone(),
                result.train_loss,
            ),
            AlgorithmResult::Cispo(result) => (
                None,
                result.policy_checkpoint_id.clone(),
                result.mean_advantage,
            ),
        };
        RunResultSummary {
            schema: state.algorithm.result_schema().into(),
            verdict: verdict.map(|value| value.to_lowercase()),
            selected_item_id: selected,
            best_score: best,
            value: serde_json::to_value(&settled).unwrap_or(Value::Null),
        }
    });

    let cost_complete = run
        .usage
        .extra
        .get("costComplete")
        .or_else(|| run.usage.extra.get("cost_complete"))
        .and_then(Value::as_bool)
        .unwrap_or(state.usage().cost_usd.is_some());

    let mut summary = OptimizerRunSummary {
        schema_version: RUN_SUMMARY_SCHEMA_VERSION.into(),
        run_id: state.run_id.clone(),
        algorithm: state.algorithm,
        status: run.status.clone(),
        lifecycle: state.lifecycle,
        phase: state.phase,
        condition: state.condition,
        placement: state.placement,
        terminal: state.terminal.as_ref().map(|terminal| RunTerminalSummary {
            kind: terminal.kind,
            reason: terminal.reason,
            final_sequence: terminal.final_sequence,
            sealed_at: terminal.sealed_at.clone(),
            failure_ref: terminal.failure_ref.clone(),
            evidence: evidence_summary(&terminal.evidence),
        }),
        failure_ref: state.failure_ref.clone(),
        spec_id: state.run_id.clone(),
        spec_digest: state.spec_digest.clone(),
        reducer_version: state.algorithm.reducer_version().into(),
        projection_revision: state.projection_revision,
        as_of_sequence: state.aggregate_sequence,
        tail_cursor,
        source: run.source.clone(),
        objective: run.objective.clone(),
        session_ref: run.session_ref.clone(),
        created_at: run.created_at.clone(),
        started_at: run.started_at.clone(),
        finished_at: run.finished_at.clone(),
        elapsed_ms,
        concurrency: RunConcurrencySummary {
            configured,
            observed_max,
        },
        work,
        usage: state.usage(),
        cost_complete,
        throughput,
        result,
        collections: collection_counts,
        execution_bindings: context.execution_bindings.clone(),
        setup,
        runtime,
        evidence: evidence_summary(&state.evidence_state()),
        artifact_count: context.artifacts.len() as u64,
        input_ref_count: context.input_refs.len() as u64,
        output_ref_count: context.output_refs.len() as u64,
        visual_ref_count: context.visual_refs.len() as u64,
        budget: RunSummaryBudget {
            bytes: 0,
            limit: SUMMARY_BYTE_BUDGET as u64,
            within: true,
        },
    };
    let bytes = serde_json::to_vec(&summary)
        .map(|body| body.len())
        .unwrap_or(0);
    summary.budget = RunSummaryBudget {
        bytes: bytes as u64,
        limit: SUMMARY_BYTE_BUDGET as u64,
        within: bytes <= SUMMARY_BYTE_BUDGET,
    };
    summary
}

/// Per-collection counts and latest revisions from the materialized rows,
/// plus the non-materialized collections passed in by the caller.
pub fn collection_counts(
    conn: &Connection,
    state: &RunKernelState,
    artifact_count: u64,
) -> Result<Vec<RunCollectionSummary>> {
    let mut statement = conn.prepare(
        "SELECT collection, COUNT(*), COALESCE(MAX(revision), 0)
         FROM optimizer_run_collection_rows
         WHERE optimizer_run_id = ?1
         GROUP BY collection",
    )?;
    let rows = statement.query_map(params![state.run_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?.max(0) as u64,
            row.get::<_, i64>(2)?.max(0) as u64,
        ))
    })?;
    let mut stored: BTreeMap<RunCollection, (u64, u64)> = BTreeMap::new();
    for row in rows {
        let (collection, count, revision) = row?;
        if let Some(collection) = RunCollection::parse(&collection) {
            stored.insert(collection, (count, revision));
        }
    }
    let evidence_refs = state.evidence_state().refs.len() as u64;
    Ok(RunCollection::ALL
        .into_iter()
        .map(|collection| {
            let (count, latest_revision) = match collection {
                RunCollection::Artifacts => (artifact_count, state.projection_revision),
                RunCollection::EvidenceRefs => (evidence_refs, state.projection_revision),
                projected => stored.get(&projected).copied().unwrap_or((0, 0)),
            };
            RunCollectionSummary {
                collection,
                count,
                latest_revision,
            }
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Checkpoints and historical projections
// ---------------------------------------------------------------------------

fn latest_checkpoint_sequence(
    conn: &Connection,
    run_id: &str,
    reducer_version: &str,
) -> Result<Option<u64>> {
    let value: Option<i64> = conn
        .query_row(
            "SELECT MAX(as_of_sequence) FROM optimizer_projection_checkpoints
             WHERE optimizer_run_id = ?1 AND reducer_version = ?2",
            params![run_id, reducer_version],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    Ok(value.map(|sequence| sequence.max(0) as u64))
}

/// Write a checkpoint of `state` at its current sequence.
pub fn write_checkpoint(conn: &Connection, state: &RunKernelState) -> Result<()> {
    let payload = serde_json::to_string(state).context("serialize kernel checkpoint")?;
    conn.execute(
        "INSERT INTO optimizer_projection_checkpoints(
            optimizer_run_id, as_of_sequence, reducer_version, projection_revision,
            state_json, byte_len, created_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))
         ON CONFLICT(optimizer_run_id, as_of_sequence) DO UPDATE SET
            reducer_version=excluded.reducer_version,
            projection_revision=excluded.projection_revision,
            state_json=excluded.state_json,
            byte_len=excluded.byte_len,
            created_at=excluded.created_at",
        params![
            state.run_id,
            state.aggregate_sequence as i64,
            state.algorithm.reducer_version(),
            state.projection_revision as i64,
            payload,
            payload.len() as i64,
        ],
    )
    .context("insert optimizer projection checkpoint")?;
    thin_checkpoints(conn, &state.run_id)?;
    Ok(())
}

/// Keep at most [`MAX_CHECKPOINTS_PER_RUN`] checkpoints by deleting every
/// other older one. The newest always survives; coverage of the early
/// history thins rather than disappears.
fn thin_checkpoints(conn: &Connection, run_id: &str) -> Result<()> {
    let mut statement = conn.prepare(
        "SELECT as_of_sequence FROM optimizer_projection_checkpoints
         WHERE optimizer_run_id = ?1 ORDER BY as_of_sequence ASC",
    )?;
    let sequences: Vec<i64> = statement
        .query_map(params![run_id], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    if sequences.len() <= MAX_CHECKPOINTS_PER_RUN {
        return Ok(());
    }
    let newest = sequences.len() - 1;
    for (index, sequence) in sequences.iter().enumerate() {
        if index != newest && index % 2 == 1 {
            conn.execute(
                "DELETE FROM optimizer_projection_checkpoints
                 WHERE optimizer_run_id = ?1 AND as_of_sequence = ?2",
                params![run_id, sequence],
            )?;
        }
    }
    Ok(())
}

/// Checkpoint when enough events have elapsed since the last one, or at the
/// terminal boundary. The interval stretches with serialized state size so a
/// multi-megabyte projection is not rewritten every few hundred events.
pub fn maybe_checkpoint(
    conn: &Connection,
    state: &RunKernelState,
    projection_bytes: usize,
) -> Result<bool> {
    if state.aggregate_sequence == 0 {
        return Ok(false);
    }
    let last = latest_checkpoint_sequence(conn, &state.run_id, state.algorithm.reducer_version())?;
    let bands = (projection_bytes / CHECKPOINT_BYTE_BAND).max(0) as u64 + 1;
    let interval = CHECKPOINT_EVENT_INTERVAL.saturating_mul(bands);
    let terminal = state.lifecycle.is_terminal() || state.terminal.is_some();
    let due = match last {
        None => true,
        Some(last) if last >= state.aggregate_sequence => false,
        Some(last) => terminal || state.aggregate_sequence - last >= interval,
    };
    if !due {
        return Ok(false);
    }
    write_checkpoint(conn, state)?;
    Ok(true)
}

/// The nearest checkpoint at or before `sequence` for the current reducer.
pub fn load_checkpoint_at_or_before(
    conn: &Connection,
    run_id: &str,
    reducer_version: &str,
    sequence: u64,
) -> Result<Option<RunKernelState>> {
    let payload: Option<String> = conn
        .query_row(
            "SELECT state_json FROM optimizer_projection_checkpoints
             WHERE optimizer_run_id = ?1 AND reducer_version = ?2 AND as_of_sequence <= ?3
             ORDER BY as_of_sequence DESC LIMIT 1",
            params![run_id, reducer_version, sequence as i64],
            |row| row.get(0),
        )
        .optional()?;
    payload
        .map(|body| serde_json::from_str(&body).context("decode optimizer projection checkpoint"))
        .transpose()
}

pub fn checkpoint_sequences(conn: &Connection, run_id: &str) -> Result<Vec<u64>> {
    let mut statement = conn.prepare(
        "SELECT as_of_sequence FROM optimizer_projection_checkpoints
         WHERE optimizer_run_id = ?1 ORDER BY as_of_sequence ASC",
    )?;
    let rows = statement.query_map(params![run_id], |row| row.get::<_, i64>(0))?;
    Ok(rows
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .map(|value| value.max(0) as u64)
        .collect())
}

/// A projection as it stood at a requested sequence.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalProjection {
    pub schema_version: String,
    pub run_id: String,
    #[specta(type = specta_typescript::Number)]
    pub requested_sequence: u64,
    #[specta(type = specta_typescript::Number)]
    pub as_of_sequence: u64,
    /// Checkpoint the fold started from; absent when it started from zero.
    #[serde(default)]
    #[specta(type = Option<specta_typescript::Number>)]
    pub checkpoint_sequence: Option<u64>,
    /// Events folded after the checkpoint to reach the requested sequence.
    #[specta(type = specta_typescript::Number)]
    pub replayed_events: u64,
    pub view: super::view::OptimizerRunViewV2,
}

/// Where the projected rows would be if the whole collection were counted:
/// the work-item lifecycle breakdown is the one algorithm-neutral fact every
/// consumer can reason about.
pub fn work_state_counts(state: &RunKernelState) -> BTreeMap<&'static str, u64> {
    let mut counts = BTreeMap::new();
    for item in state.projection.work_items() {
        let key = match item.lifecycle {
            WorkItemLifecycle::Terminal => item
                .terminal
                .map(|kind| kind.as_str())
                .unwrap_or("terminal"),
            other => other.as_str(),
        };
        *counts.entry(key).or_insert(0) += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimizers::kernel::algorithms::gepa::{
        GepaCandidate, GepaEvaluationSummary, GepaProposerCallSummary,
    };
    use crate::optimizers::kernel::algorithms::training::{
        TrainingEvaluationSummary, TrainingMetricPoint,
    };
    use crate::optimizers::kernel::types::ExecutionPlacement;

    fn schema(conn: &Connection) {
        // The row layer is exercised on its own here; the run row it would
        // reference in production is stubbed so the foreign key is satisfied.
        conn.execute_batch(
            "CREATE TABLE optimizer_runs (id TEXT PRIMARY KEY);
             INSERT INTO optimizer_runs(id) VALUES ('gepa_fixture'), ('sft_fixture'), ('many');",
        )
        .unwrap();
        conn.execute_batch(crate::storage::migrations::OPTIMIZER_READ_MODEL_CREATE_ONLY)
            .unwrap();
    }

    /// A GEPA state shaped like the Banking77 run in the handoff, scaled by
    /// argument: `candidates` candidates, `rollouts` evaluations, with a
    /// realistic (~1.5 KB) prompt on every candidate.
    pub(crate) fn gepa_state(candidates: usize, rollouts: usize) -> RunKernelState {
        let mut state = RunKernelState::new(
            "gepa_fixture",
            AlgorithmKind::Gepa,
            ExecutionPlacement::LocalPythonProcess,
            "sha256:spec",
        );
        let prompt = "You are a banking assistant. Classify the customer's message into one of the 77 intents. ".repeat(16);
        let AlgorithmProjection::Gepa(projection) = &mut state.projection else {
            unreachable!()
        };
        for index in 0..candidates {
            let id = format!("cand_{index}");
            projection.candidate_order.push(id.clone());
            projection.candidates.insert(
                id.clone(),
                GepaCandidate {
                    id,
                    parent_id: (index > 0).then(|| format!("cand_{}", index / 2)),
                    generation: Some(index as u64 / 3),
                    source: Some(if index == 0 { "seed" } else { "proposer" }.into()),
                    digest: Some(format!("sha256:{index:064x}")),
                    values: json!({ "prompt": format!("{prompt} variant {index}") }),
                    proposal_index: Some(index as u64),
                    heldout_reward: Some(0.5 + index as f64 * 0.001),
                    train_reward: Some(0.6),
                    minibatch_reward: Some(0.55),
                    gate_accepted: Some(index % 2 == 0),
                },
            );
        }
        for index in 0..rollouts {
            projection.evaluations.push(GepaEvaluationSummary {
                id: format!("eval_{index}"),
                candidate_id: Some(format!("cand_{}", index % candidates.max(1))),
                stage: Some(
                    ["candidate_minibatch", "candidate_full_train", "heldout"][index % 3].into(),
                ),
                example_id: Some(format!("train:{}", index % 50)),
                rollout_id: Some(format!("rollout_{index}")),
                reward: Some((index % 7) as f64 / 7.0),
                cost_usd: Some(0.0004),
            });
        }
        for generation in 0..(candidates / 3).max(1) {
            projection.proposer_calls.push(GepaProposerCallSummary {
                generation: generation as u64,
                model: Some("openai/gpt-5.6-luna".into()),
                provider: Some("openai".into()),
                proposal_count: 3,
                cost_usd: Some(0.02),
            });
        }
        projection.rollouts_scored = rollouts as u64;
        projection.max_active_workers = Some(4);
        projection.contract = json!({
            "container_spec_id": "banking77-gepa-b-v6",
            "scale_leases": 4,
            "dataset": { "source": "PolyAI/banking77", "row_count": 3080, "label_count": 77 },
            "policy_model": "openai/gpt-5.6-luna"
        });
        projection.runtime = json!({ "activeWorkers": 4, "job": { "state": "running" } });
        state.aggregate_sequence = (rollouts * 6 + candidates * 4) as u64;
        state.projection_revision = state.aggregate_sequence / 3 + 1;
        state
    }

    fn run_record(state: &RunKernelState) -> OptimizerRunRecord {
        serde_json::from_value(json!({
            "schemaVersion": "optimizer_run.v1",
            "id": state.run_id,
            "algorithmId": state.algorithm.wire_id(),
            "status": "running",
            "source": "local",
            "createdAt": "2026-08-31T00:00:00Z",
            "startedAt": "2026-08-31T00:00:10Z",
            "cursorSeq": state.aggregate_sequence
        }))
        .unwrap()
    }

    #[test]
    fn summary_stays_within_budget_at_ten_candidates_and_a_thousand_rollouts() {
        let state = gepa_state(10, 1_080);
        let summary = summarize(
            &state,
            &run_record(&state),
            &RunViewContext::default(),
            vec![],
            state.aggregate_sequence,
            1_756_600_000_000,
        );
        let bytes = serde_json::to_vec(&summary).unwrap().len();
        assert!(bytes <= SUMMARY_BYTE_BUDGET, "summary is {bytes} bytes");
        assert!(summary.budget.within);
        assert!(
            bytes < 8 * 1024,
            "summary should be far under the ceiling; got {bytes}"
        );
        assert_eq!(summary.concurrency.configured, Some(4));
        assert_eq!(summary.concurrency.observed_max, Some(4));
        assert!(summary.elapsed_ms.is_some());
        assert!(
            summary.throughput.is_none(),
            "no succeeded work items means no throughput claim"
        );
    }

    #[test]
    fn summary_size_is_flat_as_collections_grow() {
        let small = gepa_state(10, 1_000);
        let large = gepa_state(50, 10_000);
        let small_bytes = serde_json::to_vec(&summarize(
            &small,
            &run_record(&small),
            &RunViewContext::default(),
            vec![],
            0,
            0,
        ))
        .unwrap()
        .len();
        let large_bytes = serde_json::to_vec(&summarize(
            &large,
            &run_record(&large),
            &RunViewContext::default(),
            vec![],
            0,
            0,
        ))
        .unwrap()
        .len();
        assert!(large_bytes <= SUMMARY_BYTE_BUDGET);
        assert!(
            large_bytes < small_bytes + 1024,
            "ten times the rollouts must not grow the summary: {small_bytes} → {large_bytes}"
        );
        let projection_bytes = serde_json::to_vec(&large.projection).unwrap().len();
        assert!(projection_bytes > 10 * large_bytes, "the summary is the bounded read; the projection is not ({projection_bytes} vs {large_bytes})");
    }

    #[test]
    fn rows_sync_incrementally_and_page_by_keyset_within_budgets() {
        let conn = Connection::open_in_memory().unwrap();
        schema(&conn);
        let mut state = gepa_state(10, 250);
        sync_collection_rows(&conn, &state).unwrap();
        assert_eq!(
            stored_row_count(&conn, &state.run_id, RunCollection::Rollouts).unwrap(),
            250
        );
        assert_eq!(
            stored_row_count(&conn, &state.run_id, RunCollection::Candidates).unwrap(),
            10
        );

        // Append 30 more evaluations at a later revision: only the tail is written.
        let first_revision = state.projection_revision;
        state.projection_revision += 1;
        state.aggregate_sequence += 30;
        if let AlgorithmProjection::Gepa(projection) = &mut state.projection {
            for index in 250..280 {
                projection.evaluations.push(GepaEvaluationSummary {
                    id: format!("eval_{index}"),
                    candidate_id: Some("cand_1".into()),
                    stage: Some("heldout".into()),
                    example_id: None,
                    rollout_id: None,
                    reward: Some(0.9),
                    cost_usd: None,
                });
            }
            // A candidate changes (heldout reward lands); the others do not.
            projection
                .candidates
                .get_mut("cand_3")
                .unwrap()
                .heldout_reward = Some(0.99);
        }
        sync_collection_rows(&conn, &state).unwrap();
        let changed = query_collection_rows(
            &conn,
            &state.run_id,
            state.algorithm,
            RunCollection::Rollouts,
            &RunCollectionQuery {
                filter: Some(RunCollectionFilter {
                    changed_after_revision: Some(first_revision),
                    ..Default::default()
                }),
                limit: Some(100),
                ..Default::default()
            },
            state.projection_revision,
            state.aggregate_sequence,
        )
        .unwrap();
        assert_eq!(changed.total, 30);
        assert_eq!(changed.rows.len(), 30);
        assert!(changed.next_cursor.is_none());
        let candidates_changed = query_collection_rows(
            &conn,
            &state.run_id,
            state.algorithm,
            RunCollection::Candidates,
            &RunCollectionQuery {
                filter: Some(RunCollectionFilter {
                    changed_after_revision: Some(first_revision),
                    ..Default::default()
                }),
                ..Default::default()
            },
            state.projection_revision,
            state.aggregate_sequence,
        )
        .unwrap();
        assert_eq!(
            candidates_changed.total, 1,
            "an unchanged candidate keeps its original revision"
        );
        assert_eq!(candidates_changed.rows[0].item_id, "cand_3");

        // Keyset paging walks every row exactly once, at most 100 per page.
        let mut cursor = None;
        let mut seen = Vec::new();
        loop {
            let page = query_collection_rows(
                &conn,
                &state.run_id,
                state.algorithm,
                RunCollection::Rollouts,
                &RunCollectionQuery {
                    cursor: cursor.clone(),
                    limit: Some(1_000),
                    ..Default::default()
                },
                state.projection_revision,
                state.aggregate_sequence,
            )
            .unwrap();
            assert!(page.rows.len() as u32 <= COLLECTION_PAGE_MAX_ROWS);
            assert_eq!(
                page.limit, COLLECTION_PAGE_MAX_ROWS,
                "a limit over the ceiling is clamped, not honoured"
            );
            assert_eq!(page.total, 280);
            let bytes = serde_json::to_vec(&page).unwrap().len();
            assert!(bytes <= COLLECTION_PAGE_MAX_BYTES, "page is {bytes} bytes");
            seen.extend(page.rows.iter().map(|row| row.ordinal));
            match page.next_cursor {
                Some(next) => cursor = Some(next),
                None => break,
            }
        }
        assert_eq!(seen, (0..280).collect::<Vec<u64>>());

        // Filters narrow by parent (candidate) and label (stage).
        let filtered = query_collection_rows(
            &conn,
            &state.run_id,
            state.algorithm,
            RunCollection::Evaluations,
            &RunCollectionQuery {
                filter: Some(RunCollectionFilter {
                    parent_id: Some("cand_1".into()),
                    label: Some("heldout".into()),
                    ..Default::default()
                }),
                limit: Some(10),
                ..Default::default()
            },
            state.projection_revision,
            state.aggregate_sequence,
        )
        .unwrap();
        assert!(filtered.total >= 30);
        assert!(filtered
            .rows
            .iter()
            .all(|row| row.parent_id.as_deref() == Some("cand_1")
                && row.label.as_deref() == Some("heldout")));
        assert!(filtered.next_cursor.is_some());

        // Item lookup returns the durable candidate content.
        let candidate = load_collection_row(
            &conn,
            &state.run_id,
            state.algorithm,
            RunCollection::Candidates,
            "cand_3",
        )
        .unwrap()
        .unwrap();
        assert_eq!(candidate.score, Some(0.99));
        assert!(candidate.details["values"]["prompt"]
            .as_str()
            .unwrap()
            .contains("variant 3"));
    }

    #[test]
    fn a_shrunken_projection_rebuilds_its_rows_from_zero() {
        let conn = Connection::open_in_memory().unwrap();
        schema(&conn);
        let state = gepa_state(10, 100);
        sync_collection_rows(&conn, &state).unwrap();
        let smaller = gepa_state(4, 40);
        sync_collection_rows(&conn, &smaller).unwrap();
        assert_eq!(
            stored_row_count(&conn, &smaller.run_id, RunCollection::Rollouts).unwrap(),
            40
        );
        assert_eq!(
            stored_row_count(&conn, &smaller.run_id, RunCollection::Candidates).unwrap(),
            4
        );
    }

    #[test]
    fn large_candidate_rows_stop_a_page_on_bytes_not_rows() {
        let conn = Connection::open_in_memory().unwrap();
        schema(&conn);
        let mut state = gepa_state(0, 0);
        if let AlgorithmProjection::Gepa(projection) = &mut state.projection {
            let huge = "x".repeat(200 * 1024);
            for index in 0..6 {
                let id = format!("big_{index}");
                projection.candidate_order.push(id.clone());
                projection.candidates.insert(
                    id.clone(),
                    GepaCandidate {
                        id,
                        values: json!({ "prompt": huge.clone() }),
                        ..GepaCandidate::default()
                    },
                );
            }
        }
        sync_collection_rows(&conn, &state).unwrap();
        let page = query_collection_rows(
            &conn,
            &state.run_id,
            state.algorithm,
            RunCollection::Candidates,
            &RunCollectionQuery {
                limit: Some(6),
                ..Default::default()
            },
            1,
            1,
        )
        .unwrap();
        assert!(page.truncated_by_bytes);
        assert!(page.rows.len() < 6 && !page.rows.is_empty());
        assert!(page.next_cursor.is_some());
        assert!(serde_json::to_vec(&page).unwrap().len() <= COLLECTION_PAGE_MAX_BYTES);
    }

    #[test]
    fn a_single_oversized_detail_is_deferred_to_the_item_endpoint() {
        let conn = Connection::open_in_memory().unwrap();
        schema(&conn);
        let mut state = gepa_state(0, 0);
        if let AlgorithmProjection::Gepa(projection) = &mut state.projection {
            projection.candidate_order.push("oversized".into());
            projection.candidates.insert(
                "oversized".into(),
                GepaCandidate {
                    id: "oversized".into(),
                    values: json!({ "prompt": "x".repeat(COLLECTION_PAGE_MAX_BYTES + 32_000) }),
                    ..Default::default()
                },
            );
        }
        sync_collection_rows(&conn, &state).unwrap();
        let page = query_collection_rows(
            &conn,
            &state.run_id,
            state.algorithm,
            RunCollection::Candidates,
            &RunCollectionQuery::default(),
            1,
            1,
        )
        .unwrap();
        assert_eq!(page.rows.len(), 1);
        assert!(page.rows[0].details_deferred);
        assert!(page.rows[0].details.is_null());
        assert!(serde_json::to_vec(&page).unwrap().len() < 8 * 1024);
        let item = load_collection_row(
            &conn,
            &state.run_id,
            state.algorithm,
            RunCollection::Candidates,
            "oversized",
        )
        .unwrap()
        .unwrap();
        assert!(!item.details_deferred);
        assert!(
            item.details["values"]["prompt"].as_str().unwrap().len() > COLLECTION_PAGE_MAX_BYTES
        );
    }

    #[test]
    fn training_series_project_into_metric_and_evaluation_collections() {
        let conn = Connection::open_in_memory().unwrap();
        schema(&conn);
        let mut state = RunKernelState::new(
            "sft_fixture",
            AlgorithmKind::Sft,
            ExecutionPlacement::LocalPythonProcess,
            "sha256:spec",
        );
        if let AlgorithmProjection::Sft(projection) = &mut state.projection {
            for step in 0..100_000u64 {
                projection.metrics.push(TrainingMetricPoint {
                    step,
                    loss: Some(1.0 / (step + 1) as f64),
                    sequence: step + 1,
                    ..Default::default()
                });
            }
            projection.evaluations.push(TrainingEvaluationSummary {
                id: "ckpt-40".into(),
                step: Some(40),
                score: Some(0.8),
                sequence: 41,
                ..Default::default()
            });
        }
        state.aggregate_sequence = 100_001;
        state.projection_revision = 7;
        sync_collection_rows(&conn, &state).unwrap();
        let page = query_collection_rows(
            &conn,
            &state.run_id,
            state.algorithm,
            RunCollection::MetricPoints,
            &RunCollectionQuery {
                limit: Some(100),
                descending: true,
                ..Default::default()
            },
            7,
            100_001,
        )
        .unwrap();
        assert!(
            page.total <= 2_001,
            "metric series is bounded: {}",
            page.total
        );
        assert_eq!(
            page.rows[0].item_id, "step:99999",
            "newest first when descending"
        );
        let evaluations = query_collection_rows(
            &conn,
            &state.run_id,
            state.algorithm,
            RunCollection::Evaluations,
            &RunCollectionQuery::default(),
            7,
            100_001,
        )
        .unwrap();
        assert_eq!(evaluations.rows.len(), 1);
        assert_eq!(evaluations.rows[0].score, Some(0.8));
        let first_revision = evaluations.rows[0].revision;
        if let AlgorithmProjection::Sft(projection) = &mut state.projection {
            projection.evaluations[0].score = Some(0.93);
            projection.evaluations[0].status = Some("completed".into());
        }
        state.projection_revision += 1;
        sync_collection_rows(&conn, &state).unwrap();
        let changed = query_collection_rows(
            &conn,
            &state.run_id,
            state.algorithm,
            RunCollection::Evaluations,
            &RunCollectionQuery {
                filter: Some(RunCollectionFilter {
                    changed_after_revision: Some(first_revision),
                    ..Default::default()
                }),
                ..Default::default()
            },
            state.projection_revision,
            state.aggregate_sequence,
        )
        .unwrap();
        assert_eq!(
            changed.total, 1,
            "an in-place score update must not leave a stale collection row"
        );
        assert_eq!(changed.rows[0].score, Some(0.93));
        let projection_bytes = serde_json::to_vec(&state.projection).unwrap().len();
        assert!(
            projection_bytes < 512 * 1024,
            "a 100k-step run keeps a bounded projection: {projection_bytes}"
        );
    }

    #[test]
    fn checkpoints_are_spaced_by_events_and_bytes_and_thinned() {
        let conn = Connection::open_in_memory().unwrap();
        schema(&conn);
        let mut state = gepa_state(2, 0);
        state.aggregate_sequence = 0;
        assert!(
            !maybe_checkpoint(&conn, &state, 1_000).unwrap(),
            "nothing to checkpoint at sequence 0"
        );
        state.aggregate_sequence = 1;
        assert!(
            maybe_checkpoint(&conn, &state, 1_000).unwrap(),
            "the first checkpoint lands immediately"
        );
        state.aggregate_sequence = 400;
        assert!(
            !maybe_checkpoint(&conn, &state, 1_000).unwrap(),
            "inside the interval"
        );
        state.aggregate_sequence = 501;
        assert!(maybe_checkpoint(&conn, &state, 1_000).unwrap());
        state.aggregate_sequence = 1_100;
        assert!(
            !maybe_checkpoint(&conn, &state, 2 * CHECKPOINT_BYTE_BAND).unwrap(),
            "a large state stretches the interval"
        );
        state.aggregate_sequence = 2_100;
        assert!(maybe_checkpoint(&conn, &state, 2 * CHECKPOINT_BYTE_BAND).unwrap());
        state.aggregate_sequence = 2_105;
        state.lifecycle = RunLifecycle::Terminal;
        assert!(
            maybe_checkpoint(&conn, &state, 1_000).unwrap(),
            "the terminal boundary always checkpoints"
        );
        assert_eq!(
            checkpoint_sequences(&conn, &state.run_id).unwrap(),
            vec![1, 501, 2_100, 2_105]
        );

        let mut many = gepa_state(2, 0);
        many.run_id = "many".into();
        for sequence in 1..=(MAX_CHECKPOINTS_PER_RUN as u64 * 3) {
            many.aggregate_sequence = sequence;
            write_checkpoint(&conn, &many).unwrap();
        }
        let retained = checkpoint_sequences(&conn, "many").unwrap();
        assert!(retained.len() <= MAX_CHECKPOINTS_PER_RUN);
        assert_eq!(
            *retained.last().unwrap(),
            MAX_CHECKPOINTS_PER_RUN as u64 * 3,
            "the newest checkpoint survives thinning"
        );
        assert!(retained[0] < 10, "early history keeps some coverage");
        assert_eq!(
            load_checkpoint_at_or_before(
                &conn,
                "many",
                AlgorithmKind::Gepa.reducer_version(),
                1_000
            )
            .unwrap()
            .unwrap()
            .aggregate_sequence,
            MAX_CHECKPOINTS_PER_RUN as u64 * 3
        );
        assert!(
            load_checkpoint_at_or_before(&conn, "many", "gepa.projection.v99", 1_000)
                .unwrap()
                .is_none(),
            "a stale reducer's checkpoints are never folded from"
        );
    }

    #[test]
    fn in_memory_pager_matches_the_sql_pager_contract() {
        let state = gepa_state(1, 0);
        let rows: Vec<RunCollectionRow> = (0..250u64)
            .map(|ordinal| RunCollectionRow {
                schema_version: COLLECTION_ROW_SCHEMA_VERSION.into(),
                run_id: state.run_id.clone(),
                algorithm: state.algorithm,
                collection: RunCollection::EvidenceRefs,
                item_id: format!("trace:{ordinal}"),
                ordinal,
                sequence: ordinal,
                revision: 1,
                kind: "trace_v5".into(),
                label: None,
                parent_id: None,
                score: None,
                cost_usd: None,
                status: None,
                details_version: "evidence_ref.v1".into(),
                details_deferred: false,
                details_bytes: 0,
                details: json!({ "id": ordinal }),
            })
            .collect();
        let first = page_rows_in_memory(
            &state.run_id,
            RunCollection::EvidenceRefs,
            rows.clone(),
            &RunCollectionQuery::default(),
            1,
            1,
        )
        .unwrap();
        assert_eq!(first.rows.len(), COLLECTION_PAGE_DEFAULT_ROWS as usize);
        assert_eq!(first.total, 250);
        let second = page_rows_in_memory(
            &state.run_id,
            RunCollection::EvidenceRefs,
            rows,
            &RunCollectionQuery {
                cursor: first.next_cursor.clone(),
                ..Default::default()
            },
            1,
            1,
        )
        .unwrap();
        assert_eq!(second.rows[0].ordinal, COLLECTION_PAGE_DEFAULT_ROWS as u64);
    }
}
