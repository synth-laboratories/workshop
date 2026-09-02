use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::BTreeMap;
use uuid::Uuid;

use crate::contract::specta::OpaqueJson;
use crate::lineage::store::{self, load_graph};
use crate::lineage::CandidateRecord;
use crate::storage::{append_event, EventAppend, EventSource};

use super::candidates::{self, load_for_experiment};
use super::models::{
    ExperimentChildCreateRequest, ExperimentCreateRequest, ExperimentFinalizeRequest,
    ExperimentGroup, ExperimentLineageEdge, ExperimentMember, ExperimentRelateRequest,
    ExperimentUpdateRequest, ResearchJournalAppendRequest, ResearchJournalEntry, MEMBER_OPTIMIZER,
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
    let id = format!("exp_{}", Uuid::new_v4().simple());
    conn.execute(
        "INSERT INTO experiment_groups(
            id, session_id, request_id, title, created_at, updated_at, task, model
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7)",
        params![
            id,
            request.session_id,
            request.request_id,
            request.title,
            request.created_at,
            request.task,
            request.model
        ],
    )?;
    set_active(conn, &request.session_id, &id)?;
    get(conn, &id)?.ok_or_else(|| anyhow::anyhow!("experiment disappeared after create"))
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
    let relation = lineage_relation(request.relation.as_deref())?;
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
    let tags_json = serde_json::to_string(&parent.tags)?;
    conn.execute(
        "INSERT INTO experiment_groups(
            id, session_id, request_id, title, created_at, updated_at, task, model, tags_json
         ) VALUES(?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7, ?8)",
        params![
            id,
            parent.session_id,
            request.request_id,
            request.title,
            request.created_at,
            task,
            model,
            tags_json
        ],
    )?;
    insert_lineage(conn, &parent.id, &id, relation, &request.created_at)?;
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
                "relation": relation,
            }),
            remote_sequence: None,
            command_id: None,
            created_at: Some(request.created_at.clone()),
        },
    )?;
    get(conn, &id)?.ok_or_else(|| anyhow::anyhow!("experiment disappeared after create_child"))
}

pub fn relate(conn: &Connection, request: ExperimentRelateRequest) -> Result<ExperimentGroup> {
    anyhow::ensure!(
        !request.experiment_id.trim().is_empty(),
        "experimentId is required"
    );
    anyhow::ensure!(!request.source_id.trim().is_empty(), "sourceId is required");
    anyhow::ensure!(!request.target_id.trim().is_empty(), "targetId is required");
    anyhow::ensure!(
        request.source_id != request.target_id,
        "relate source and target must differ"
    );
    let relation = request.relation.trim();
    anyhow::ensure!(
        matches!(relation, "compared_with" | "promoted_to"),
        "unknown experiment relation `{relation}`"
    );
    let source_kind = request.source_kind.trim();
    let target_kind = request.target_kind.trim();
    anyhow::ensure!(
        source_kind == target_kind,
        "mixed member/candidate relate is refused"
    );
    let changed = match (source_kind, target_kind) {
        ("member", "member") => relate_members(conn, &request, relation)?,
        ("candidate", "candidate") => relate_candidates(conn, &request, relation)?,
        _ => anyhow::bail!("unknown relate kind `{source_kind}`"),
    };
    if changed {
        let session_id: String = conn.query_row(
            "SELECT session_id FROM experiment_groups WHERE id=?1",
            [&request.experiment_id],
            |row| row.get(0),
        )?;
        append_event(
            conn,
            EventAppend {
                event_id: None,
                session_id: Some(session_id),
                run_id: None,
                source: EventSource::System,
                kind: "experiment.related".into(),
                payload: serde_json::json!({
                    "experimentId": request.experiment_id,
                    "relation": relation,
                    "sourceKind": source_kind,
                    "sourceId": request.source_id,
                    "targetKind": target_kind,
                    "targetId": request.target_id,
                }),
                remote_sequence: None,
                command_id: None,
                created_at: Some(request.created_at.clone()),
            },
        )?;
    }
    get(conn, &request.experiment_id)?
        .ok_or_else(|| anyhow::anyhow!("experiment disappeared after relate"))
}

pub fn activate(
    conn: &Connection,
    session_id: &str,
    experiment_id: &str,
) -> Result<ExperimentGroup> {
    let group = get(conn, experiment_id)?.ok_or_else(|| anyhow::anyhow!("unknown experiment"))?;
    anyhow::ensure!(
        group.session_id == session_id,
        "experiment is owned by another session"
    );
    set_active(conn, session_id, experiment_id)?;
    Ok(group)
}

pub fn update(conn: &Connection, request: ExperimentUpdateRequest) -> Result<ExperimentGroup> {
    anyhow::ensure!(
        !request.experiment_id.trim().is_empty(),
        "experimentId is required"
    );
    let current =
        get(conn, &request.experiment_id)?.ok_or_else(|| anyhow::anyhow!("unknown experiment"))?;
    anyhow::ensure!(
        current.session_id == request.session_id,
        "experiment is owned by another session"
    );
    let title = request.title.unwrap_or(current.title).trim().to_owned();
    anyhow::ensure!(!title.is_empty(), "title is required");
    let tags = normalize_tags(request.tags.unwrap_or(current.tags))?;
    conn.execute(
        "UPDATE experiment_groups SET title=?2, task=?3, model=?4, tags_json=?5, updated_at=?6 WHERE id=?1",
        params![
            request.experiment_id,
            title,
            request.task.or(current.task),
            request.model.or(current.model),
            serde_json::to_string(&tags)?,
            request.updated_at,
        ],
    )?;
    get(conn, &request.experiment_id)?
        .ok_or_else(|| anyhow::anyhow!("experiment disappeared after update"))
}

fn normalize_tags(tags: Vec<String>) -> Result<Vec<String>> {
    anyhow::ensure!(tags.len() <= 24, "an experiment may have at most 24 tags");
    let mut normalized = Vec::new();
    for tag in tags {
        let tag = tag.trim();
        anyhow::ensure!(!tag.is_empty(), "experiment tags must not be empty");
        anyhow::ensure!(
            tag.chars().count() <= 48,
            "experiment tags may have at most 48 characters"
        );
        if !normalized
            .iter()
            .any(|value: &String| value.eq_ignore_ascii_case(tag))
        {
            normalized.push(tag.to_owned());
        }
    }
    Ok(normalized)
}

pub fn research_log_append(
    conn: &Connection,
    request: ResearchJournalAppendRequest,
) -> Result<ResearchJournalEntry> {
    anyhow::ensure!(!request.body.trim().is_empty(), "research log entry requires a body");
    anyhow::ensure!(
        matches!(
            request.entry_kind.as_str(),
            "observation" | "hypothesis" | "decision" | "result" | "failure" | "limitation" | "follow_up"
        ),
        "unsupported research log entry kind"
    );
    let actor_kind = request.actor_kind.unwrap_or_else(|| "agent".into());
    anyhow::ensure!(matches!(actor_kind.as_str(), "human" | "agent"), "research log actorKind must be human or agent");
    if let Some(experiment_id) = request.experiment_id.as_deref() {
        anyhow::ensure!(get(conn, experiment_id)?.is_some(), "unknown linked experiment");
    }
    if let Some(parent) = request.supersedes_entry_id.as_deref() {
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM research_journal_entries WHERE entry_id=?1)",
            [parent],
            |row| row.get(0),
        )?;
        anyhow::ensure!(exists, "research log supersession target is missing");
    }
    let tags = normalize_tags(request.tags.unwrap_or_default())?;
    let sequence: i64 = conn.query_row(
        "SELECT COALESCE(MAX(sequence),0)+1 FROM research_journal_entries",
        [],
        |row| row.get(0),
    )?;
    let now = chrono::Utc::now().to_rfc3339();
    let entry = ResearchJournalEntry {
        entry_id: format!("rlog_{}", Uuid::new_v4().simple()),
        sequence,
        occurred_at: request.occurred_at.unwrap_or_else(|| now.clone()),
        recorded_at: now,
        author: request.author.unwrap_or_else(|| "Workshop agent".into()),
        actor_kind,
        entry_kind: request.entry_kind,
        title: request.title.trim().to_owned(),
        body: request.body.trim().to_owned(),
        tags,
        links: request.links.unwrap_or_else(|| serde_json::json!([])),
        experiment_id: request.experiment_id,
        supersedes_entry_id: request.supersedes_entry_id,
        source_digest: request.source_digest,
    };
    conn.execute(
        "INSERT INTO research_journal_entries(entry_id,sequence,occurred_at,recorded_at,author,actor_kind,entry_kind,title,body,tags_json,links_json,experiment_id,supersedes_entry_id,source_digest)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
        params![entry.entry_id, entry.sequence, entry.occurred_at, entry.recorded_at, entry.author,
            entry.actor_kind, entry.entry_kind, entry.title, entry.body,
            serde_json::to_string(&entry.tags)?, serde_json::to_string(&entry.links)?,
            entry.experiment_id, entry.supersedes_entry_id, entry.source_digest],
    )?;
    Ok(entry)
}

pub fn research_log_list(
    conn: &Connection,
    query: Option<&str>,
    experiment_id: Option<&str>,
) -> Result<Vec<ResearchJournalEntry>> {
    let needle = format!("%{}%", query.unwrap_or("").trim().to_lowercase());
    let mut stmt = conn.prepare(
        "SELECT entry_id,sequence,occurred_at,recorded_at,author,actor_kind,entry_kind,title,body,tags_json,links_json,experiment_id,supersedes_entry_id,source_digest
         FROM research_journal_entries
         WHERE (?1 IS NULL OR experiment_id=?1) AND
           (lower(title) LIKE ?2 OR lower(body) LIKE ?2 OR lower(entry_kind) LIKE ?2 OR lower(tags_json) LIKE ?2)
         ORDER BY sequence DESC",
    )?;
    let rows = stmt.query_map(params![experiment_id, needle], |row| {
        Ok(ResearchJournalEntry {
            entry_id: row.get(0)?, sequence: row.get(1)?, occurred_at: row.get(2)?,
            recorded_at: row.get(3)?, author: row.get(4)?, actor_kind: row.get(5)?,
            entry_kind: row.get(6)?, title: row.get(7)?, body: row.get(8)?,
            tags: serde_json::from_str(&row.get::<_, String>(9)?).unwrap_or_default(),
            links: serde_json::from_str(&row.get::<_, String>(10)?).unwrap_or_else(|_| serde_json::json!([])),
            experiment_id: row.get(11)?, supersedes_entry_id: row.get(12)?, source_digest: row.get(13)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>().map_err(Into::into)
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
    anyhow::ensure!(
        member_kind == MEMBER_OPTIMIZER,
        "only optimizer_run members can settle experiment execution"
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

/// Attach one executed optimizer run to the session's experiment group.
pub fn attach(
    conn: &Connection,
    session_id: &str,
    member_kind: &str,
    member_id: &str,
    attached_at: &str,
    title: &str,
) -> Result<ExperimentGroup> {
    anyhow::ensure!(
        member_kind == MEMBER_OPTIMIZER,
        "executed experiment members must be optimizer_run references, not `{member_kind}`"
    );
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM optimizer_runs WHERE id=?1)",
        [member_id],
        |row| row.get(0),
    )?;
    anyhow::ensure!(exists, "optimizer run `{member_id}` does not exist");
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
         WHERE lower(title) LIKE ?1 OR lower(COALESCE(task,'')) LIKE ?1 OR lower(COALESCE(model,'')) LIKE ?1 OR lower(status) LIKE ?1 OR lower(COALESCE(tags_json,'[]')) LIKE ?1
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
            "SELECT id, session_id, title, created_at, COALESCE(updated_at, created_at), status, task, model, best_result_json, COALESCE(tags_json,'[]') FROM experiment_groups WHERE id = ?1",
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
                    tags: serde_json::from_str(&row.get::<_, String>(9)?).unwrap_or_default(),
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
    attach_candidates(conn, &mut group)?;
    Ok(Some(group))
}

fn attach_candidates(conn: &Connection, group: &mut ExperimentGroup) -> Result<()> {
    let rows = load_for_experiment(conn, &group.id)?;
    let mut by_run: BTreeMap<String, Vec<CandidateRecord>> = BTreeMap::new();
    for row in rows {
        by_run
            .entry(row.optimizer_run_id.clone())
            .or_default()
            .push(row);
    }
    for node in &mut group.nodes {
        if node.kind != MEMBER_OPTIMIZER {
            continue;
        }
        let member_id = node
            .config
            .0
            .get("memberId")
            .and_then(|value| value.as_str())
            .map(str::to_owned);
        if let Some(member_id) = member_id {
            node.candidates = by_run.remove(&member_id).unwrap_or_default();
        }
    }
    Ok(())
}

fn load_by_request_id(conn: &Connection, request_id: &str) -> Result<Option<ExperimentGroup>> {
    let experiment_id: Option<String> = conn
        .query_row(
            "SELECT id FROM experiment_groups WHERE request_id=?1",
            [request_id],
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
    anyhow::ensure!(
        member_kind == MEMBER_OPTIMIZER,
        "experiment members must use canonical optimizer_run identity"
    );
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
                "templateId": "synth.optimizer_run.v1",
            }),
            remote_sequence: None,
            command_id: None,
            created_at: Some(attached_at.to_owned()),
        },
    )?;
    Ok(())
}

pub(super) fn insert_lineage(
    conn: &Connection,
    source_experiment_id: &str,
    target_experiment_id: &str,
    relation: &str,
    at: &str,
) -> Result<()> {
    lineage_relation(Some(relation))?;
    let id = format!("lin:{source_experiment_id}:{relation}:{target_experiment_id}");
    conn.execute(
        "INSERT OR IGNORE INTO experiment_lineage(id, source_experiment_id, target_experiment_id, relation, created_at)
         VALUES(?1, ?2, ?3, ?4, ?5)",
        params![id, source_experiment_id, target_experiment_id, relation, at],
    )?;
    Ok(())
}

fn lineage_relation(value: Option<&str>) -> Result<&'static str> {
    match value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("follow_up")
    {
        "follow_up" => Ok("follow_up"),
        "forked_from" => Ok("forked_from"),
        "rerun_of" => Ok("rerun_of"),
        other => anyhow::bail!("unknown experiment lineage relation `{other}`"),
    }
}

fn relate_members(
    conn: &Connection,
    request: &ExperimentRelateRequest,
    relation: &str,
) -> Result<bool> {
    ensure_node_in_experiment(conn, &request.experiment_id, &request.source_id)?;
    ensure_node_in_experiment(conn, &request.experiment_id, &request.target_id)?;
    store::insert_edge(
        conn,
        &request.experiment_id,
        &request.source_id,
        &request.target_id,
        relation,
        &request.created_at,
    )?;
    Ok(conn.changes() > 0)
}

fn relate_candidates(
    conn: &Connection,
    request: &ExperimentRelateRequest,
    relation: &str,
) -> Result<bool> {
    let source = candidates::load_by_id(conn, &request.source_id)?
        .ok_or_else(|| anyhow::anyhow!("unknown candidate {}", request.source_id))?;
    let target = candidates::load_by_id(conn, &request.target_id)?
        .ok_or_else(|| anyhow::anyhow!("unknown candidate {}", request.target_id))?;
    anyhow::ensure!(
        source.experiment_id == request.experiment_id
            && target.experiment_id == request.experiment_id,
        "candidates must belong to the same experiment"
    );
    match relation {
        "compared_with" => {
            let left = candidates::append_compared_with(
                conn,
                &source.id,
                &target.id,
                &request.created_at,
            )?;
            let right = candidates::append_compared_with(
                conn,
                &target.id,
                &source.id,
                &request.created_at,
            )?;
            Ok(left || right)
        }
        "promoted_to" => {
            candidates::set_promoted_to(conn, &source.id, &target.id, &request.created_at)
        }
        _ => anyhow::bail!("unknown experiment relation `{relation}`"),
    }
}

fn ensure_node_in_experiment(conn: &Connection, experiment_id: &str, node_id: &str) -> Result<()> {
    let exists: Option<String> = conn
        .query_row(
            "SELECT id FROM experiment_nodes WHERE id=?1 AND experiment_id=?2",
            params![node_id, experiment_id],
            |row| row.get(0),
        )
        .optional()?;
    anyhow::ensure!(exists.is_some(), "unknown experiment member `{node_id}`");
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
