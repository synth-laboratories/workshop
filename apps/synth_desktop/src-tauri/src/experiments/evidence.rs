use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::contract::specta::OpaqueJson;
use crate::lineage::store;
use crate::lineage::ExperimentEvidenceRef;
use crate::storage::{append_event, EventAppend, EventSource};

use super::registry::get;
use super::ExperimentGroup;

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentEvidenceAttachRequest {
    pub experiment_id: String,
    pub session_id: Option<String>,
    pub node_id: Option<String>,
    pub evidence_id: String,
    pub kind: String,
    pub label: String,
    pub digest: Option<String>,
    pub container_id: Option<String>,
    pub rollout_id: Option<String>,
    pub trace_id: Option<String>,
    pub visual_id: Option<String>,
    pub artifact_uri: Option<String>,
    pub metadata: Option<OpaqueJson>,
    pub attached_at: String,
}

pub fn attach_member_evidence(
    conn: &Connection,
    member_kind: &str,
    member_id: &str,
    mut request: ExperimentEvidenceAttachRequest,
) -> Result<()> {
    anyhow::ensure!(
        member_kind == super::MEMBER_OPTIMIZER,
        "execution evidence attaches only to optimizer_run members"
    );
    let experiment_id: Option<String> = conn
        .query_row(
            "SELECT group_id FROM experiment_group_members WHERE member_kind=?1 AND member_id=?2",
            params![member_kind, member_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(experiment_id) = experiment_id else {
        return Ok(());
    };
    request.experiment_id = experiment_id.clone();
    request.node_id = store::resolve_member_node_id(conn, &experiment_id, member_kind, member_id)?;
    attach_evidence(conn, request)?;
    Ok(())
}

pub fn attach_evidence(
    conn: &Connection,
    request: ExperimentEvidenceAttachRequest,
) -> Result<ExperimentGroup> {
    anyhow::ensure!(
        ["trace", "visual", "artifact"].contains(&request.kind.as_str()),
        "unsupported experiment evidence kind {}",
        request.kind
    );
    anyhow::ensure!(
        !request.evidence_id.trim().is_empty(),
        "evidenceId is required"
    );
    match request.kind.as_str() {
        "trace" => anyhow::ensure!(
            request.trace_id.is_some()
                || (request.container_id.is_some() && request.rollout_id.is_some()),
            "trace evidence requires traceId or containerId + rolloutId"
        ),
        "visual" => anyhow::ensure!(
            request.visual_id.is_some(),
            "visual evidence requires visualId"
        ),
        "artifact" => anyhow::ensure!(
            request.artifact_uri.is_some(),
            "artifact evidence requires artifactUri"
        ),
        _ => unreachable!(),
    }
    let node_id = match request.node_id.clone().filter(|id| !id.trim().is_empty()) {
        Some(id) => id,
        None => store::latest_node_id(conn, &request.experiment_id)?
            .ok_or_else(|| anyhow::anyhow!("experiment has no member node"))?,
    };
    let (session_id, mut refs) = store::load_node_evidence(conn, &node_id, &request.experiment_id)?;
    if let Some(claimed) = request.session_id.as_deref() {
        anyhow::ensure!(
            claimed == session_id,
            "experiment is owned by another session"
        );
    }
    let evidence = ExperimentEvidenceRef {
        evidence_id: request.evidence_id.clone(),
        kind: request.kind.clone(),
        label: request.label.clone(),
        digest: request.digest.clone(),
        container_id: request.container_id.clone(),
        rollout_id: request.rollout_id.clone(),
        trace_id: request.trace_id.clone(),
        visual_id: request.visual_id.clone(),
        artifact_uri: request.artifact_uri.clone(),
        metadata: request
            .metadata
            .unwrap_or_else(|| OpaqueJson(serde_json::json!({}))),
        attached_at: request.attached_at.clone(),
    };
    let inserted = if let Some(existing) = refs
        .iter()
        .find(|item| item.evidence_id == evidence.evidence_id)
    {
        let mut replay = evidence.clone();
        replay.attached_at = existing.attached_at.clone();
        anyhow::ensure!(
            serde_json::to_value(existing)? == serde_json::to_value(&replay)?,
            "evidenceId already exists with different content"
        );
        false
    } else {
        refs.push(evidence.clone());
        refs.sort_by(|left, right| {
            left.attached_at
                .cmp(&right.attached_at)
                .then(left.evidence_id.cmp(&right.evidence_id))
        });
        store::update_evidence_refs(conn, &node_id, &refs, &request.attached_at)?;
        conn.execute(
            "UPDATE experiment_groups SET updated_at=?2 WHERE id=?1",
            params![request.experiment_id, request.attached_at],
        )?;
        true
    };
    if inserted {
        append_event(
            conn,
            EventAppend {
                event_id: None,
                session_id: Some(session_id),
                run_id: None,
                source: EventSource::System,
                kind: "experiment.evidence.attached".into(),
                payload: serde_json::json!({"experimentId":request.experiment_id,"nodeId":node_id,"evidence":evidence}),
                remote_sequence: None,
                command_id: None,
                created_at: Some(request.attached_at),
            },
        )?;
    }
    get(conn, &request.experiment_id)?
        .ok_or_else(|| anyhow::anyhow!("experiment disappeared after evidence attach"))
}
