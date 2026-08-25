//! Local-first experiment registry and explicit lineage projection.
//!
//! A chat that starts an evaluation campaign and a GEPA run should be able to
//! name both as members of the same experiment without either leaking into
//! another task's right pane.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::contract::specta::OpaqueJson;
use crate::storage::{append_event, EventAppend, EventSource};

pub const MEMBER_CAMPAIGN: &str = "eval_campaign";
pub const MEMBER_OPTIMIZER: &str = "optimizer_run";
pub const MEMBER_DIRECT: &str = "direct_evaluation";

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentCreateRequest {
    pub session_id: String,
    pub request_id: String,
    pub title: String,
    pub task: Option<String>,
    pub model: Option<String>,
    pub created_at: String,
}

pub fn create(conn: &Connection, request: ExperimentCreateRequest) -> Result<ExperimentGroup> {
    anyhow::ensure!(!request.session_id.trim().is_empty(), "sessionId is required");
    anyhow::ensure!(!request.request_id.trim().is_empty(), "requestId is required");
    anyhow::ensure!(!request.title.trim().is_empty(), "title is required");
    let group = attach(conn, &request.session_id, MEMBER_DIRECT, &request.request_id, &request.created_at, &request.title)?;
    conn.execute(
        "UPDATE experiment_groups SET title=?2,task=COALESCE(?3,task),model=COALESCE(?4,model),updated_at=?5 WHERE id=?1",
        params![group.id, request.title, request.task, request.model, request.created_at],
    )?;
    load_for_session(conn, &request.session_id)?.ok_or_else(|| anyhow::anyhow!("experiment disappeared after create"))
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentFinalizeRequest {
    pub experiment_id: String,
    pub session_id: String,
    pub status: String,
    pub result: OpaqueJson,
    pub assessment: Option<OpaqueJson>,
    pub finalized_at: String,
}

pub fn finalize(conn: &Connection, request: ExperimentFinalizeRequest) -> Result<ExperimentGroup> {
    let owner: String = conn.query_row("SELECT session_id FROM experiment_groups WHERE id=?1", [&request.experiment_id], |row| row.get(0))?;
    anyhow::ensure!(owner == request.session_id, "experiment is owned by another session");
    anyhow::ensure!(["completed", "failed", "partial"].contains(&request.status.as_str()), "invalid terminal experiment status");
    let result_id: String = conn.query_row(
        "SELECT id FROM experiment_nodes WHERE experiment_id=?1 AND kind='result' ORDER BY created_at DESC,id DESC LIMIT 1",
        [&request.experiment_id], |row| row.get(0),
    )?;
    let provenance = serde_json::json!({"assessment": request.assessment.map(|value| value.0), "authority":"agent_recorded"});
    conn.execute("UPDATE experiment_nodes SET status=?2,metrics_json=?3,provenance_json=?4,updated_at=?5 WHERE id=?1", params![result_id, request.status, request.result.0.to_string(), provenance.to_string(), request.finalized_at])?;
    conn.execute("UPDATE experiment_nodes SET status=?2,updated_at=?3 WHERE experiment_id=?1 AND kind IN ('baseline','variant')", params![request.experiment_id, request.status, request.finalized_at])?;
    conn.execute("UPDATE experiment_groups SET status=?2,best_result_json=?3,updated_at=?4 WHERE id=?1", params![request.experiment_id, request.status, request.result.0.to_string(), request.finalized_at])?;
    get(conn, &request.experiment_id)?.ok_or_else(|| anyhow::anyhow!("experiment disappeared after finalize"))
}

pub fn settle_member(
    conn: &Connection,
    member_kind: &str,
    member_id: &str,
    status: &str,
    task: &str,
    model: Option<&str>,
    result: &serde_json::Value,
    trace_refs: &[String],
    at: &str,
) -> Result<()> {
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
    let group_status = match status {
        "complete" => "completed",
        "failed" => "failed",
        _ => status,
    };
    conn.execute(
        "UPDATE experiment_groups SET status=?2, task=?3, model=?4, best_result_json=?5, updated_at=?6 WHERE id=?1",
        params![experiment_id, group_status, task, model, result.to_string(), at],
    )?;
    let variant = format!("{experiment_id}:variant:{member_id}");
    let result_id = format!("{experiment_id}:result:{member_id}");
    conn.execute(
        "UPDATE experiment_nodes SET status=?2, provenance_json=?3, updated_at=?4 WHERE id=?1",
        params![variant, group_status, serde_json::json!({"memberKind":member_kind,"memberId":member_id,"task":task,"model":model}).to_string(), at],
    )?;
    conn.execute(
        "UPDATE experiment_nodes SET status=?2, metrics_json=?3, trace_refs_json=?4, provenance_json=?5, updated_at=?6 WHERE id=?1",
        params![result_id, group_status, result.to_string(), serde_json::to_string(trace_refs)?, serde_json::json!({"memberKind":member_kind,"memberId":member_id,"task":task,"model":model}).to_string(), at],
    )?;
    Ok(())
}

pub fn attach_member_evidence(
    conn: &Connection,
    member_kind: &str,
    member_id: &str,
    mut request: ExperimentEvidenceAttachRequest,
) -> Result<()> {
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
    request.experiment_id = experiment_id;
    attach_evidence(conn, request)?;
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentMember {
    pub member_kind: String,
    pub member_id: String,
    pub title: String,
    pub attached_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentGroup {
    pub id: String,
    pub session_id: String,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
    pub status: String,
    pub task: Option<String>,
    pub model: Option<String>,
    pub best_result: Option<OpaqueJson>,
    pub members: Vec<ExperimentMember>,
    pub nodes: Vec<ExperimentNode>,
    pub edges: Vec<ExperimentEdge>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentNode {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub status: String,
    pub config: OpaqueJson,
    pub metrics: Option<OpaqueJson>,
    pub cost_usd: Option<f64>,
    pub artifact_refs: Vec<String>,
    pub trace_refs: Vec<String>,
    pub evidence_refs: Vec<ExperimentEvidenceRef>,
    pub provenance: OpaqueJson,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentEvidenceRef {
    pub evidence_id: String,
    pub kind: String,
    pub label: String,
    pub digest: Option<String>,
    pub container_id: Option<String>,
    pub rollout_id: Option<String>,
    pub trace_id: Option<String>,
    pub visual_id: Option<String>,
    pub artifact_uri: Option<String>,
    pub metadata: OpaqueJson,
    pub attached_at: String,
}

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

pub fn attach_evidence(
    conn: &Connection,
    request: ExperimentEvidenceAttachRequest,
) -> Result<ExperimentGroup> {
    anyhow::ensure!(
        ["trace", "visual", "artifact", "container"].contains(&request.kind.as_str()),
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
        "container" => anyhow::ensure!(
            request.container_id.is_some(),
            "container evidence requires containerId"
        ),
        _ => unreachable!(),
    }
    let node_id = match request.node_id.clone() {
        Some(id) => id,
        None => conn.query_row(
            "SELECT id FROM experiment_nodes WHERE experiment_id=?1 AND kind='result' ORDER BY created_at DESC,id DESC LIMIT 1",
            [&request.experiment_id], |row| row.get(0),
        ).optional()?.ok_or_else(|| anyhow::anyhow!("experiment has no result node"))?,
    };
    let (session_id, raw): (String, String) = conn.query_row(
        "SELECT g.session_id,n.evidence_refs_json FROM experiment_nodes n JOIN experiment_groups g ON g.id=n.experiment_id WHERE n.id=?1 AND n.experiment_id=?2",
        params![node_id, request.experiment_id], |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if let Some(claimed) = request.session_id.as_deref() {
        anyhow::ensure!(claimed == session_id, "experiment is owned by another session");
    }
    let mut refs: Vec<ExperimentEvidenceRef> = serde_json::from_str(&raw).unwrap_or_default();
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
        conn.execute(
            "UPDATE experiment_nodes SET evidence_refs_json=?2,updated_at=?3 WHERE id=?1",
            params![node_id, serde_json::to_string(&refs)?, request.attached_at],
        )?;
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

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentEdge {
    pub id: String,
    pub source_node_id: String,
    pub target_node_id: String,
    pub relation: String,
    pub created_at: String,
}

/// Attach a campaign or optimizer run to the session's experiment group,
/// creating the group on first use. Idempotent on `(kind, id)`.
pub fn attach(
    conn: &Connection,
    session_id: &str,
    member_kind: &str,
    member_id: &str,
    attached_at: &str,
    title: &str,
) -> Result<ExperimentGroup> {
    let group = ensure_group(conn, session_id, title, attached_at)?;
    let inserted = conn.execute(
        "INSERT OR IGNORE INTO experiment_group_members(group_id, member_kind, member_id, title, attached_at)
         VALUES(?1, ?2, ?3, ?4, ?5)",
        params![group.id, member_kind, member_id, title, attached_at],
    )?;
    if inserted > 0 {
        project_member_lineage(conn, &group.id, member_kind, member_id, title, attached_at)?;
        conn.execute(
            "UPDATE experiment_groups SET updated_at=?2, status='running' WHERE id=?1",
            params![group.id, attached_at],
        )?;
        let _ = append_event(
            conn,
            EventAppend {
                event_id: None,
                session_id: Some(session_id.to_owned()),
                run_id: None,
                source: EventSource::System,
                kind: "experiment.member.attached".into(),
                payload: serde_json::json!({
                    "experimentId": group.id,
                    "sessionId": session_id,
                    "memberKind": member_kind,
                    "memberId": member_id,
                    "title": title,
                    "templateId": match member_kind {
                        MEMBER_CAMPAIGN => "synth.eval_campaign.v1",
                        MEMBER_OPTIMIZER => "synth.optimizer_run.v1",
                        _ => "synth.experiment.v1",
                    },
                }),
                remote_sequence: None,
                command_id: None,
                created_at: Some(attached_at.to_owned()),
            },
        )?;
    }
    load_for_session(conn, session_id)?.ok_or_else(|| {
        anyhow::anyhow!("experiment group for {session_id} disappeared after attach")
    })
}

pub fn load_for_session(conn: &Connection, session_id: &str) -> Result<Option<ExperimentGroup>> {
    let Some(mut group) = conn
        .query_row(
            "SELECT id, session_id, title, created_at, COALESCE(updated_at, created_at), status, task, model, best_result_json FROM experiment_groups WHERE session_id = ?1",
            params![session_id],
            |row| {
                Ok(ExperimentGroup {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    title: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                    status: row.get(5)?,
                    task: row.get(6)?,
                    model: row.get(7)?,
                    best_result: row.get::<_, Option<String>>(8)?.and_then(|value| serde_json::from_str(&value).ok()).map(OpaqueJson),
                    members: Vec::new(),
                    nodes: Vec::new(),
                    edges: Vec::new(),
                })
            },
        )
        .optional()?
    else {
        return Ok(None);
    };
    let mut stmt = conn.prepare(
        "SELECT member_kind, member_id, title, attached_at
           FROM experiment_group_members
          WHERE group_id = ?1
          ORDER BY attached_at, member_id",
    )?;
    group.members = stmt
        .query_map(params![group.id], |row| {
            Ok(ExperimentMember {
                member_kind: row.get(0)?,
                member_id: row.get(1)?,
                title: row.get(2)?,
                attached_at: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    group.nodes = load_nodes(conn, &group.id)?;
    group.edges = load_edges(conn, &group.id)?;
    Ok(Some(group))
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<ExperimentGroup>> {
    let session_id: Option<String> = conn
        .query_row(
            "SELECT session_id FROM experiment_groups WHERE id=?1",
            [id],
            |row| row.get(0),
        )
        .optional()?;
    match session_id {
        Some(id) => load_for_session(conn, &id),
        None => Ok(None),
    }
}

pub fn list(conn: &Connection, query: Option<&str>) -> Result<Vec<ExperimentGroup>> {
    let needle = format!("%{}%", query.unwrap_or("").trim().to_lowercase());
    let mut stmt = conn.prepare(
        "SELECT session_id FROM experiment_groups
         WHERE lower(title) LIKE ?1 OR lower(COALESCE(task,'')) LIKE ?1 OR lower(COALESCE(model,'')) LIKE ?1 OR lower(status) LIKE ?1
         ORDER BY COALESCE(updated_at, created_at) DESC, id",
    )?;
    let ids = stmt
        .query_map([needle], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ids.into_iter()
        .filter_map(|id| load_for_session(conn, &id).transpose())
        .collect()
}

fn project_member_lineage(
    conn: &Connection,
    experiment_id: &str,
    member_kind: &str,
    member_id: &str,
    title: &str,
    at: &str,
) -> Result<()> {
    let baseline = format!("{experiment_id}:baseline");
    let variant = format!("{experiment_id}:variant:{member_id}");
    let result = format!("{experiment_id}:result:{member_id}");
    for (id, kind, label, status, config) in [
        (
            &baseline,
            "baseline",
            "Current baseline",
            "completed",
            serde_json::json!({"role":"baseline"}),
        ),
        (
            &variant,
            "variant",
            title,
            "running",
            serde_json::json!({"memberKind":member_kind,"memberId":member_id}),
        ),
        (
            &result,
            "result",
            "Comparison result",
            "running",
            serde_json::json!({"memberKind":member_kind,"memberId":member_id}),
        ),
    ] {
        conn.execute("INSERT OR IGNORE INTO experiment_nodes(id,experiment_id,kind,title,status,config_json,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?7)", params![id, experiment_id, kind, label, status, config.to_string(), at])?;
    }
    for (source, target, relation) in [
        (&baseline, &variant, "forked_from"),
        (&variant, &result, "evaluated"),
    ] {
        let edge_id = format!("edge:{source}:{relation}:{target}");
        conn.execute("INSERT OR IGNORE INTO experiment_edges(id,experiment_id,source_node_id,target_node_id,relation,created_at) VALUES(?1,?2,?3,?4,?5,?6)", params![edge_id, experiment_id, source, target, relation, at])?;
    }
    Ok(())
}

fn load_nodes(conn: &Connection, experiment_id: &str) -> Result<Vec<ExperimentNode>> {
    let mut stmt = conn.prepare("SELECT id,kind,title,status,config_json,metrics_json,cost_usd,artifact_refs_json,trace_refs_json,provenance_json,created_at,updated_at,evidence_refs_json FROM experiment_nodes WHERE experiment_id=?1 ORDER BY created_at,id")?;
    let rows = stmt
        .query_map([experiment_id], |row| {
            Ok(ExperimentNode {
                id: row.get(0)?,
                kind: row.get(1)?,
                title: row.get(2)?,
                status: row.get(3)?,
                config: OpaqueJson(
                    serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or_default(),
                ),
                metrics: row
                    .get::<_, Option<String>>(5)?
                    .and_then(|v| serde_json::from_str(&v).ok())
                    .map(OpaqueJson),
                cost_usd: row.get(6)?,
                artifact_refs: serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_default(),
                trace_refs: serde_json::from_str(&row.get::<_, String>(8)?).unwrap_or_default(),
                provenance: OpaqueJson(
                    serde_json::from_str(&row.get::<_, String>(9)?).unwrap_or_default(),
                ),
                created_at: row.get(10)?,
                updated_at: row.get(11)?,
                evidence_refs: serde_json::from_str(&row.get::<_, String>(12)?).unwrap_or_default(),
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

fn load_edges(conn: &Connection, experiment_id: &str) -> Result<Vec<ExperimentEdge>> {
    let mut stmt = conn.prepare("SELECT id,source_node_id,target_node_id,relation,created_at FROM experiment_edges WHERE experiment_id=?1 ORDER BY created_at,id")?;
    let rows = stmt
        .query_map([experiment_id], |row| {
            Ok(ExperimentEdge {
                id: row.get(0)?,
                source_node_id: row.get(1)?,
                target_node_id: row.get(2)?,
                relation: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

fn ensure_group(
    conn: &Connection,
    session_id: &str,
    title: &str,
    created_at: &str,
) -> Result<ExperimentGroup> {
    if let Some(existing) = load_for_session(conn, session_id)? {
        return Ok(existing);
    }
    let id = format!("exp_{}", Uuid::new_v4().simple());
    conn.execute(
        "INSERT INTO experiment_groups(id, session_id, title, created_at, updated_at)
         VALUES(?1, ?2, ?3, ?4, ?4)",
        params![id, session_id, format!("{title} experiment"), created_at],
    )?;
    load_for_session(conn, session_id)?
        .ok_or_else(|| anyhow::anyhow!("failed to create experiment group for {session_id}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn database() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::storage::migrations::apply_migrations(&conn).unwrap();
        conn
    }

    #[test]
    fn five_concurrent_workflow_identities_stay_isolated() {
        let conn = database();
        for index in 1..=5 {
            let session = format!("session_{index}");
            let campaign = format!("camp_{index}");
            let optimizer = format!("opt_{index}");
            attach(
                &conn,
                &session,
                MEMBER_CAMPAIGN,
                &campaign,
                "2026-08-17T00:00:00Z",
                &format!("Eval {index}"),
            )
            .unwrap();
            attach(
                &conn,
                &session,
                MEMBER_OPTIMIZER,
                &optimizer,
                "2026-08-17T00:00:01Z",
                &format!("GEPA {index}"),
            )
            .unwrap();
        }
        for index in 1..=5 {
            let group = load_for_session(&conn, &format!("session_{index}"))
                .unwrap()
                .expect("each task owns an experiment group");
            assert_eq!(group.session_id, format!("session_{index}"));
            assert_eq!(group.members.len(), 2);
            assert!(group
                .members
                .iter()
                .all(|member| member.member_id.ends_with(&index.to_string())));
            assert!(!group.members.iter().any(|member| member
                .member_id
                .contains(&(index % 5 + 1).to_string())
                && member.member_id != format!("camp_{index}")
                && member.member_id != format!("opt_{index}")));
        }
        let session_1 = load_for_session(&conn, "session_1").unwrap().unwrap();
        assert!(!session_1
            .members
            .iter()
            .any(|member| member.member_id == "camp_2" || member.member_id == "opt_2"));
    }

    #[test]
    fn attaching_the_same_member_twice_is_idempotent() {
        let conn = database();
        attach(
            &conn,
            "session_1",
            MEMBER_CAMPAIGN,
            "camp_1",
            "2026-08-17T00:00:00Z",
            "Eval",
        )
        .unwrap();
        let again = attach(
            &conn,
            "session_1",
            MEMBER_CAMPAIGN,
            "camp_1",
            "2026-08-17T00:00:02Z",
            "Eval",
        )
        .unwrap();
        assert_eq!(again.members.len(), 1);
        assert_eq!(again.nodes.len(), 3);
        assert_eq!(again.edges.len(), 2);
        assert_eq!(again.edges[0].relation, "forked_from");
        assert_eq!(again.edges[1].relation, "evaluated");
        assert!(again.nodes.iter().all(|node| node.cost_usd.is_none()));
        assert!(again.nodes.iter().all(|node| node.metrics.is_none()));
    }

    #[test]
    fn settling_a_member_updates_summary_nodes_and_preserves_missing_cost() {
        let conn = database();
        attach(
            &conn,
            "session_1",
            MEMBER_CAMPAIGN,
            "camp_1",
            "2026-08-17T00:00:00Z",
            "Craftax compare",
        )
        .unwrap();
        settle_member(
            &conn,
            MEMBER_CAMPAIGN,
            "camp_1",
            "complete",
            "CRAFTAX-EMBER-0824",
            Some("gpt-5.6-luna"),
            &serde_json::json!({"reward":{"mean":null},"sampleSize":2}),
            &["/rollouts/a/trace".into(), "/rollouts/b/trace".into()],
            "2026-08-17T00:01:00Z",
        )
        .unwrap();
        let group = load_for_session(&conn, "session_1").unwrap().unwrap();
        assert_eq!(group.status, "completed");
        assert_eq!(group.task.as_deref(), Some("CRAFTAX-EMBER-0824"));
        assert_eq!(group.model.as_deref(), Some("gpt-5.6-luna"));
        let result = group
            .nodes
            .iter()
            .find(|node| node.kind == "result")
            .unwrap();
        assert_eq!(result.status, "completed");
        assert_eq!(result.trace_refs.len(), 2);
        assert!(result.cost_usd.is_none());
        assert_eq!(
            result.metrics.as_ref().unwrap().0["reward"]["mean"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn search_and_reopen_return_the_same_durable_identity() {
        let conn = database();
        let created = attach(
            &conn,
            "task_craftax",
            MEMBER_CAMPAIGN,
            "run_1",
            "2026-08-24T12:00:00Z",
            "Craftax prompt variant",
        )
        .unwrap();
        let found = list(&conn, Some("craftax")).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, created.id);
        assert_eq!(get(&conn, &created.id).unwrap().unwrap().id, created.id);
    }

    #[test]
    fn evidence_references_are_typed_idempotent_and_durable() {
        let conn = database();
        let experiment = attach(
            &conn,
            "task_1",
            MEMBER_CAMPAIGN,
            "camp_1",
            "2026-08-24T12:00:00Z",
            "Craftax",
        )
        .unwrap();
        let request = ExperimentEvidenceAttachRequest {
            experiment_id: experiment.id.clone(),
            session_id: Some("task_1".into()),
            node_id: None,
            evidence_id: "trace:craftax:rollout_1".into(),
            kind: "trace".into(),
            label: "Seed 0 trace".into(),
            digest: Some("sha256:seal".into()),
            container_id: Some("craftax".into()),
            rollout_id: Some("rollout_1".into()),
            trace_id: None,
            visual_id: None,
            artifact_uri: None,
            metadata: Some(OpaqueJson(serde_json::json!({"eventCount":75}))),
            attached_at: "2026-08-24T12:01:00Z".into(),
        };
        attach_evidence(&conn, request.clone()).unwrap();
        let replay = ExperimentEvidenceAttachRequest {
            attached_at: "2026-08-24T12:02:00Z".into(),
            ..request.clone()
        };
        let group = attach_evidence(&conn, replay).unwrap();
        let result = group
            .nodes
            .iter()
            .find(|node| node.kind == "result")
            .unwrap();
        assert_eq!(result.evidence_refs.len(), 1);
        assert_eq!(
            result.evidence_refs[0].rollout_id.as_deref(),
            Some("rollout_1")
        );
        assert_eq!(result.evidence_refs[0].metadata.0["eventCount"], 75);
        let mut conflict = request;
        conflict.trace_id = Some("different".into());
        assert!(attach_evidence(&conn, conflict)
            .unwrap_err()
            .to_string()
            .contains("different content"));
        let events: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE kind='experiment.evidence.attached'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(events, 1);
    }

    #[test]
    fn container_evidence_records_an_admitted_container() {
        let conn = database();
        let experiment = attach(
            &conn,
            "task_1",
            MEMBER_CAMPAIGN,
            "camp_1",
            "2026-08-25T12:00:00Z",
            "Craftax",
        )
        .unwrap();
        let group = attach_evidence(
            &conn,
            ExperimentEvidenceAttachRequest {
                experiment_id: experiment.id,
                session_id: Some("task_1".into()),
                node_id: None,
                evidence_id: "container:craftax".into(),
                kind: "container".into(),
                label: "Admitted Craftax container".into(),
                digest: Some("sha256:manifest".into()),
                container_id: Some("ctr_craftax".into()),
                rollout_id: None,
                trace_id: None,
                visual_id: None,
                artifact_uri: None,
                metadata: Some(OpaqueJson(serde_json::json!({"source": "container_catalog"}))),
                attached_at: "2026-08-25T12:01:00Z".into(),
            },
        )
        .unwrap();
        let result = group
            .nodes
            .iter()
            .find(|node| node.kind == "result")
            .unwrap();
        assert_eq!(result.evidence_refs[0].kind, "container");
        assert_eq!(
            result.evidence_refs[0].container_id.as_deref(),
            Some("ctr_craftax")
        );
    }

    #[test]
    fn direct_agent_lifecycle_is_indexed_idempotent_and_session_owned() {
        let conn = database();
        let request = ExperimentCreateRequest { session_id:"task_direct".into(), request_id:"CRAFTAX-EMBER-0824".into(), title:"Craftax survival comparison".into(), task:Some("CRAFTAX-EMBER-0824".into()), model:Some("gpt-5.6-luna".into()), created_at:"2026-08-24T12:00:00Z".into() };
        let first = create(&conn, request.clone()).unwrap();
        let replay = create(&conn, ExperimentCreateRequest { created_at:"2026-08-24T12:01:00Z".into(), ..request }).unwrap();
        assert_eq!(first.id, replay.id);
        assert_eq!(replay.nodes.len(), 3);
        let done = finalize(&conn, ExperimentFinalizeRequest { experiment_id:first.id.clone(), session_id:"task_direct".into(), status:"completed".into(), result:OpaqueJson(serde_json::json!({"baselineReward":0.5,"variantReward":0.5,"delta":0})), assessment:Some(OpaqueJson(serde_json::json!({"verdict":"insufficient evidence"}))), finalized_at:"2026-08-24T12:02:00Z".into() }).unwrap();
        assert_eq!(done.status, "completed");
        assert_eq!(list(&conn, Some("craftax")).unwrap().len(), 1);
        assert!(finalize(&conn, ExperimentFinalizeRequest { experiment_id:first.id, session_id:"another_task".into(), status:"failed".into(), result:OpaqueJson(serde_json::json!({})), assessment:None, finalized_at:"2026-08-24T12:03:00Z".into() }).is_err());
    }
}
