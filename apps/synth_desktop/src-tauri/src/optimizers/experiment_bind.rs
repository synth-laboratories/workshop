//! Bind optimizer runs to the CoreRuntime experiment registry.
//!
//! Campaigns already attach and settle themselves. Optimizer persist paths
//! call these helpers from the same sqlite transaction as the durable write.
//! `optimizer_relationships.started_from` stays the optimizer's own session
//! link; it is not an experiment edge.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

use crate::contract::specta::OpaqueJson;
use crate::experiments::{
    attach, attach_member_evidence, settle_member, ExperimentEvidenceAttachRequest, MEMBER_OPTIMIZER,
};
use crate::lineage::store;

use super::models::{OptimizerRunRecord, OptimizerRunStatus};

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
