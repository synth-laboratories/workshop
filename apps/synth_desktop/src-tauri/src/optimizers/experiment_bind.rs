//! Bind optimizer runs to the CoreRuntime experiment registry.
//!
//! Campaigns already attach and settle themselves. Optimizer persist paths
//! call these helpers from the same sqlite transaction as the durable write.
//! `optimizer_relationships.started_from` stays the optimizer's own session
//! link; it is not an experiment edge.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use crate::contract::specta::OpaqueJson;
use crate::experiments::{
    attach, attach_member_evidence, settle_member, upsert_candidate, CandidateUpsert,
    ExperimentEvidenceAttachRequest, MEMBER_OPTIMIZER,
};
use crate::lineage::store;

use super::models::{OptimizerEventEnvelope, OptimizerRunRecord, OptimizerRunStatus};

pub fn attach_run(conn: &Connection, run: &OptimizerRunRecord) -> Result<()> {
    let Some(session_id) = run
        .session_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(());
    };
    attach(
        conn,
        session_id,
        MEMBER_OPTIMIZER,
        &run.id,
        &run.created_at,
        &run_title(run),
    )?;
    Ok(())
}

pub fn settle_run(conn: &Connection, run: &OptimizerRunRecord) -> Result<()> {
    if !OptimizerRunStatus::str_is_terminal(&run.status) {
        return Ok(());
    }
    let at = run
        .finished_at
        .as_deref()
        .unwrap_or(run.created_at.as_str());
    let result = settle_result(run);
    settle_member(
        conn,
        MEMBER_OPTIMIZER,
        &run.id,
        &run.status,
        run.objective
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(run.algorithm_id.as_str()),
        run.summary
            .get("model")
            .and_then(serde_json::Value::as_str)
            .or(run.algorithm_version.as_deref()),
        &result,
        &[],
        at,
    )?;
    if let Some(cost) = run.usage.cost_usd {
        let experiment_id: Option<String> = conn
            .query_row(
                "SELECT group_id FROM experiment_group_members WHERE member_kind=?1 AND member_id=?2",
                params![MEMBER_OPTIMIZER, run.id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(experiment_id) = experiment_id {
            let node_id = store::member_node_id(&experiment_id, MEMBER_OPTIMIZER, &run.id);
            conn.execute(
                "UPDATE experiment_nodes SET cost_usd=?2 WHERE id=?1",
                params![node_id, cost],
            )?;
        }
    }
    attach_visual_evidence(conn, run, at)?;
    Ok(())
}

/// Project producer candidate identity from an already-parsed optimizer event
/// onto the run's experiment member. No-op when the run is not attached or the
/// event has no `candidate_id` / `best_candidate_id`.
pub fn fold_candidate(conn: &Connection, event: &OptimizerEventEnvelope) -> Result<()> {
    let Some(producer_candidate_id) = producer_candidate_id(event) else {
        return Ok(());
    };
    upsert_candidate(
        conn,
        CandidateUpsert {
            optimizer_run_id: event.optimizer_run_id.clone(),
            producer_candidate_id,
            kind: first_str(event, &["kind", "candidate_kind"]),
            protocol_id: first_str(event, &["protocol_id"]),
            status: status_from_event(event),
            parent_ids: parent_ids(event),
            metrics: metrics_from_event(event),
            content_digest: first_str(event, &["content_digest", "digest"]),
            compared_with: None,
            promoted_to: None,
            at: event.occurred_at.clone(),
        },
    )
}

fn producer_candidate_id(event: &OptimizerEventEnvelope) -> Option<String> {
    event
        .delta
        .get("candidate_id")
        .and_then(Value::as_str)
        .or_else(|| {
            event
                .item
                .as_ref()
                .and_then(|item| item.get("id"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            event
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.get("best_candidate_id"))
                .and_then(Value::as_str)
        })
        .or_else(|| event.delta.get("best_candidate_id").and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn first_str(event: &OptimizerEventEnvelope, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = event.delta.get(*key).and_then(Value::as_str) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
        if let Some(value) = event
            .item
            .as_ref()
            .and_then(|item| item.get(*key))
            .and_then(Value::as_str)
        {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
        if let Some(value) = event
            .snapshot
            .as_ref()
            .and_then(|snapshot| snapshot.get(*key))
            .and_then(Value::as_str)
        {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn parent_ids(event: &OptimizerEventEnvelope) -> Vec<String> {
    let mut ids = Vec::new();
    for key in ["parent_id", "parentId"] {
        if let Some(value) = first_str(event, &[key]) {
            if !ids.contains(&value) {
                ids.push(value);
            }
        }
    }
    for key in ["parent_ids", "parent_candidate_ids"] {
        for source in [
            event.delta.get(key),
            event.item.as_ref().and_then(|item| item.get(key)),
            event
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.get(key)),
        ]
        .into_iter()
        .flatten()
        {
            if let Some(values) = source.as_array() {
                for value in values {
                    if let Some(id) = value.as_str().map(str::trim).filter(|id| !id.is_empty()) {
                        let id = id.to_string();
                        if !ids.contains(&id) {
                            ids.push(id);
                        }
                    }
                }
            }
        }
    }
    ids
}

fn status_from_event(event: &OptimizerEventEnvelope) -> Option<String> {
    match event.event_type.as_str() {
        "candidate.registered" => Some("registered".into()),
        "candidate.accepted" => Some("accepted".into()),
        "candidate.rejected" => Some("rejected".into()),
        "candidate.evaluated"
        | "candidate.minibatch_evaluated"
        | "candidate.full_train_evaluated" => Some("evaluated".into()),
        "gepa.candidate.updated" => Some("updated".into()),
        _ => None,
    }
}

fn metrics_from_event(event: &OptimizerEventEnvelope) -> Option<Value> {
    let mut metrics = serde_json::Map::new();
    for key in [
        "reward",
        "best_train_reward",
        "train_reward",
        "heldout_reward",
        "coverage",
        "generation",
    ] {
        if let Some(value) = event
            .delta
            .get(key)
            .or_else(|| {
                event
                    .snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.get(key))
            })
            .or_else(|| event.item.as_ref().and_then(|item| item.get(key)))
        {
            if !value.is_null() {
                metrics.insert(key.to_string(), value.clone());
            }
        }
    }
    if metrics.is_empty() {
        None
    } else {
        Some(Value::Object(metrics))
    }
}

fn run_title(run: &OptimizerRunRecord) -> String {
    run.objective
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| run.algorithm_id.clone())
}

fn settle_result(run: &OptimizerRunRecord) -> serde_json::Value {
    serde_json::json!({
        "algorithmId": run.algorithm_id,
        "status": run.status,
        "costUsd": run.usage.cost_usd,
        "terminalManifest": run.summary.get("terminalManifest"),
    })
}

fn attach_visual_evidence(conn: &Connection, run: &OptimizerRunRecord, at: &str) -> Result<()> {
    for reference in &run.visual_refs {
        attach_member_evidence(
            conn,
            MEMBER_OPTIMIZER,
            &run.id,
            ExperimentEvidenceAttachRequest {
                experiment_id: String::new(),
                session_id: run.session_ref.clone(),
                node_id: None,
                evidence_id: format!("visual:{}", reference.id),
                kind: "visual".into(),
                label: reference
                    .title
                    .clone()
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| reference.id.clone()),
                digest: reference.digest.clone(),
                container_id: None,
                rollout_id: None,
                trace_id: None,
                visual_id: Some(reference.id.clone()),
                artifact_uri: None,
                metadata: Some(OpaqueJson(reference.metadata.clone())),
                attached_at: at.to_owned(),
            },
        )?;
    }
    Ok(())
}

