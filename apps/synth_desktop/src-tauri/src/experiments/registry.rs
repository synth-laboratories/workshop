use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::contract::specta::OpaqueJson;
use crate::lineage::store::{self, load_graph};
use crate::storage::{append_event, EventAppend, EventSource};

use super::models::{
    ExperimentChildCreateRequest, ExperimentCreateRequest, ExperimentFinalizeRequest,
    ExperimentGroup, ExperimentLineageEdge, ExperimentMember, MEMBER_CAMPAIGN, MEMBER_DIRECT,
    MEMBER_OPTIMIZER,
};

pub fn create(conn: &Connection, request: ExperimentCreateRequest) -> Result<ExperimentGroup> {
    anyhow::ensure!(
        !request.session_id.trim().is_empty(),
        "sessionId is required"
    );
    anyhow::ensure!(
        !request.request_id.trim().is_empty(),
        "requestId is required"
    );
    anyhow::ensure!(!request.title.trim().is_empty(), "title is required");
    if let Some(existing) = load_by_request_id(conn, &request.request_id)? {
        return Ok(existing);
    }
    let group = attach(
        conn,
        &request.session_id,
        super::MEMBER_DIRECT,
        &request.request_id,
        &request.created_at,
        &request.title,
    )?;
    conn.execute(
        "UPDATE experiment_groups SET title=?2,task=COALESCE(?3,task),model=COALESCE(?4,model),updated_at=?5 WHERE id=?1",
        params![group.id, request.title, request.task, request.model, request.created_at],
    )?;
    get(conn, &group.id)?.ok_or_else(|| anyhow::anyhow!("experiment disappeared after create"))
}

pub fn create_child(
    conn: &Connection,
    request: ExperimentChildCreateRequest,
) -> Result<ExperimentGroup> {
    anyhow::ensure!(
        !request.parent_experiment_id.trim().is_empty(),
        "parentExperimentId is required"
    );
    anyhow::ensure!(
        !request.request_id.trim().is_empty(),
        "requestId is required"
    );
    anyhow::ensure!(!request.title.trim().is_empty(), "title is required");
    if let Some(existing) = load_by_request_id(conn, &request.request_id)? {
        return Ok(existing);
    }
    let parent = get(conn, &request.parent_experiment_id)?
        .ok_or_else(|| anyhow::anyhow!("unknown parent experiment"))?;
    if let Some(claimed) = request
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        anyhow::ensure!(
            claimed == parent.session_id,
            "experiment is owned by another session"
        );
    }
    let id = format!("exp_{}", Uuid::new_v4().simple());
    let task = request.task.or(parent.task.clone());
    let model = request.model.or(parent.model.clone());
    conn.execute(
        "INSERT INTO experiment_groups(id, session_id, title, created_at, updated_at, task, model)
         VALUES(?1, ?2, ?3, ?4, ?4, ?5, ?6)",
        params![
            id,
            parent.session_id,
            request.title,
            request.created_at,
            task,
            model
        ],
    )?;
    attach_member_row(
        conn,
        &id,
        MEMBER_DIRECT,
        &request.request_id,
        &request.created_at,
        &request.title,
        &parent.session_id,
    )?;
    insert_lineage(
        conn,
        &parent.id,
        &id,
        "follow_up",
        &request.created_at,
    )?;
    set_active(conn, &parent.session_id, &id)?;
    append_event(
        conn,
        EventAppend {
            event_id: None,
            session_id: Some(parent.session_id.clone()),
            run_id: None,
            source: EventSource::System,
            kind: "experiment.child.created".into(),
            payload: serde_json::json!({
                "experimentId": id,
                "parentExperimentId": parent.id,
                "sessionId": parent.session_id,
                "requestId": request.request_id,
                "relation": "follow_up",
            }),
            remote_sequence: None,
            command_id: None,
            created_at: Some(request.created_at.clone()),
        },
    )?;
    get(conn, &id)?.ok_or_else(|| anyhow::anyhow!("experiment disappeared after create_child"))
}

pub fn activate(
    conn: &Connection,
    session_id: &str,
    experiment_id: &str,
) -> Result<ExperimentGroup> {
    let group = get(conn, experiment_id)?
        .ok_or_else(|| anyhow::anyhow!("unknown experiment"))?;
    anyhow::ensure!(
        group.session_id == session_id,
        "experiment is owned by another session"
    );
    set_active(conn, session_id, experiment_id)?;
    Ok(group)
}

pub fn finalize(conn: &Connection, request: ExperimentFinalizeRequest) -> Result<ExperimentGroup> {
    let owner: String = conn.query_row(
        "SELECT session_id FROM experiment_groups WHERE id=?1",
        [&request.experiment_id],
        |row| row.get(0),
    )?;
    anyhow::ensure!(
        owner == request.session_id,
        "experiment is owned by another session"
    );
    anyhow::ensure!(
        ["completed", "failed", "partial"].contains(&request.status.as_str()),
        "invalid terminal experiment status"
    );
    let node_id = store::latest_node_id(conn, &request.experiment_id)?
        .ok_or_else(|| anyhow::anyhow!("experiment has no member node"))?;
    let provenance = serde_json::json!({"assessment": request.assessment.map(|value| value.0), "authority":"agent_recorded"});
    conn.execute(
        "UPDATE experiment_nodes SET status=?2,metrics_json=?3,provenance_json=?4,updated_at=?5 WHERE id=?1",
        params![node_id, request.status, request.result.0.to_string(), provenance.to_string(), request.finalized_at],
    )?;
    conn.execute(
        "UPDATE experiment_nodes SET status=?2,updated_at=?3 WHERE experiment_id=?1",
        params![request.experiment_id, request.status, request.finalized_at],
    )?;
    conn.execute(
        "UPDATE experiment_groups SET status=?2,best_result_json=?3,updated_at=?4 WHERE id=?1",
        params![
            request.experiment_id,
            request.status,
            request.result.0.to_string(),
            request.finalized_at
        ],
    )?;
    get(conn, &request.experiment_id)?
        .ok_or_else(|| anyhow::anyhow!("experiment disappeared after finalize"))
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
    store::settle_member_nodes(
        conn,
        &experiment_id,
        member_kind,
        member_id,
        group_status,
        task,
        model,
        result,
        trace_refs,
        at,
    )?;
    Ok(())
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
    attach_member_row(
        conn,
        &group.id,
        member_kind,
        member_id,
        attached_at,
        title,
        session_id,
    )?;
    load_group(conn, &group.id)?.ok_or_else(|| {
        anyhow::anyhow!("experiment group for {session_id} disappeared after attach")
    })
}

pub fn load_for_session(conn: &Connection, session_id: &str) -> Result<Option<ExperimentGroup>> {
    let Some(id) = resolve_session_experiment_id(conn, session_id)? else {
        return Ok(None);
    };
    load_group(conn, &id)
}

pub fn get(conn: &Connection, id: &str) -> Result<Option<ExperimentGroup>> {
    load_group(conn, id)
}

pub fn list(conn: &Connection, query: Option<&str>) -> Result<Vec<ExperimentGroup>> {
    let needle = format!("%{}%", query.unwrap_or("").trim().to_lowercase());
    let mut stmt = conn.prepare(
        "SELECT id FROM experiment_groups
         WHERE lower(title) LIKE ?1 OR lower(COALESCE(task,'')) LIKE ?1 OR lower(COALESCE(model,'')) LIKE ?1 OR lower(status) LIKE ?1
         ORDER BY COALESCE(updated_at, created_at) DESC, id",
    )?;
    let ids = stmt
        .query_map([needle], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ids.into_iter()
        .filter_map(|id| load_group(conn, &id).transpose())
        .collect()
}

fn load_group(conn: &Connection, id: &str) -> Result<Option<ExperimentGroup>> {
    let Some(mut group) = conn
        .query_row(
            "SELECT id, session_id, title, created_at, COALESCE(updated_at, created_at), status, task, model, best_result_json FROM experiment_groups WHERE id = ?1",
            params![id],
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
                    lineage: Vec::new(),
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
    let graph = load_graph(conn, &group.id)?;
    group.nodes = graph.nodes;
    group.edges = graph.edges;
    group.lineage = load_outgoing_lineage(conn, &group.id)?;
    Ok(Some(group))
}

fn load_by_request_id(conn: &Connection, request_id: &str) -> Result<Option<ExperimentGroup>> {
    let experiment_id: Option<String> = conn
        .query_row(
            "SELECT group_id FROM experiment_group_members WHERE member_kind=?1 AND member_id=?2",
            params![MEMBER_DIRECT, request_id],
            |row| row.get(0),
        )
        .optional()?;
    match experiment_id {
        Some(id) => load_group(conn, &id),
        None => Ok(None),
    }
}

fn resolve_session_experiment_id(conn: &Connection, session_id: &str) -> Result<Option<String>> {
    let cursor: Option<String> = conn
        .query_row(
            "SELECT active_experiment_id FROM experiment_session_cursor WHERE session_id=?1",
            params![session_id],
            |row| row.get(0),
        )
        .optional()?;
    if cursor.is_some() {
        return Ok(cursor);
    }
    let from_session: Option<String> = conn
        .query_row(
            "SELECT active_experiment_id FROM sessions WHERE id=?1 AND active_experiment_id IS NOT NULL",
            params![session_id],
            |row| row.get(0),
        )
        .optional()?;
    if from_session.is_some() {
        return Ok(from_session);
    }
    Ok(conn
        .query_row(
            "SELECT id FROM experiment_groups WHERE session_id=?1 ORDER BY created_at, id LIMIT 1",
            params![session_id],
            |row| row.get(0),
        )
        .optional()?)
}

fn set_active(conn: &Connection, session_id: &str, experiment_id: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO experiment_session_cursor(session_id, active_experiment_id) VALUES(?1, ?2)
         ON CONFLICT(session_id) DO UPDATE SET active_experiment_id=excluded.active_experiment_id",
        params![session_id, experiment_id],
    )?;
    conn.execute(
        "UPDATE sessions SET active_experiment_id=?2 WHERE id=?1",
        params![session_id, experiment_id],
    )?;
    Ok(())
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
    set_active(conn, session_id, &id)?;
    load_group(conn, &id)?
        .ok_or_else(|| anyhow::anyhow!("failed to create experiment group for {session_id}"))
}

fn attach_member_row(
    conn: &Connection,
    group_id: &str,
    member_kind: &str,
    member_id: &str,
    attached_at: &str,
    title: &str,
    session_id: &str,
) -> Result<()> {
    let inserted = conn.execute(
        "INSERT OR IGNORE INTO experiment_group_members(group_id, member_kind, member_id, title, attached_at)
         VALUES(?1, ?2, ?3, ?4, ?5)",
        params![group_id, member_kind, member_id, title, attached_at],
    )?;
    if inserted == 0 {
        return Ok(());
    }
    store::project_member(conn, group_id, member_kind, member_id, title, attached_at)?;
    conn.execute(
        "UPDATE experiment_groups SET updated_at=?2, status='running' WHERE id=?1",
        params![group_id, attached_at],
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
                "experimentId": group_id,
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
    Ok(())
}

fn insert_lineage(
    conn: &Connection,
    source_experiment_id: &str,
    target_experiment_id: &str,
    relation: &str,
    at: &str,
) -> Result<()> {
    let id = format!("lin:{source_experiment_id}:{relation}:{target_experiment_id}");
    conn.execute(
        "INSERT OR IGNORE INTO experiment_lineage(id, source_experiment_id, target_experiment_id, relation, created_at)
         VALUES(?1, ?2, ?3, ?4, ?5)",
        params![id, source_experiment_id, target_experiment_id, relation, at],
    )?;
    Ok(())
}

fn load_outgoing_lineage(
    conn: &Connection,
    experiment_id: &str,
) -> Result<Vec<ExperimentLineageEdge>> {
    let mut stmt = conn.prepare(
        "SELECT id, source_experiment_id, target_experiment_id, relation, created_at
           FROM experiment_lineage WHERE source_experiment_id=?1 ORDER BY created_at, id",
    )?;
    let rows = stmt
        .query_map(params![experiment_id], |row| {
            Ok(ExperimentLineageEdge {
                id: row.get(0)?,
                source_experiment_id: row.get(1)?,
                target_experiment_id: row.get(2)?,
                relation: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}
