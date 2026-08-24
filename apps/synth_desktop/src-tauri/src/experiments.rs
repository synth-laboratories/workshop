//! Local-first experiment registry and explicit lineage projection.
//!
//! A chat that starts an evaluation campaign and a GEPA run should be able to
//! name both as members of the same experiment without either leaking into
//! another task's right pane.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::storage::{append_event, EventAppend, EventSource};
use crate::contract::specta::OpaqueJson;

pub const MEMBER_CAMPAIGN: &str = "eval_campaign";
pub const MEMBER_OPTIMIZER: &str = "optimizer_run";

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
    pub provenance: OpaqueJson,
    pub created_at: String,
    pub updated_at: String,
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
    let session_id: Option<String> = conn.query_row(
        "SELECT session_id FROM experiment_groups WHERE id=?1",
        [id], |row| row.get(0),
    ).optional()?;
    match session_id { Some(id) => load_for_session(conn, &id), None => Ok(None) }
}

pub fn list(conn: &Connection, query: Option<&str>) -> Result<Vec<ExperimentGroup>> {
    let needle = format!("%{}%", query.unwrap_or("").trim().to_lowercase());
    let mut stmt = conn.prepare(
        "SELECT session_id FROM experiment_groups
         WHERE lower(title) LIKE ?1 OR lower(COALESCE(task,'')) LIKE ?1 OR lower(COALESCE(model,'')) LIKE ?1 OR lower(status) LIKE ?1
         ORDER BY COALESCE(updated_at, created_at) DESC, id",
    )?;
    let ids = stmt.query_map([needle], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ids.into_iter().filter_map(|id| load_for_session(conn, &id).transpose()).collect()
}

fn project_member_lineage(conn: &Connection, experiment_id: &str, member_kind: &str, member_id: &str, title: &str, at: &str) -> Result<()> {
    let baseline = format!("{experiment_id}:baseline");
    let variant = format!("{experiment_id}:variant:{member_id}");
    let result = format!("{experiment_id}:result:{member_id}");
    for (id, kind, label, status, config) in [
        (&baseline, "baseline", "Current baseline", "completed", serde_json::json!({"role":"baseline"})),
        (&variant, "variant", title, "running", serde_json::json!({"memberKind":member_kind,"memberId":member_id})),
        (&result, "result", "Comparison result", "running", serde_json::json!({"memberKind":member_kind,"memberId":member_id})),
    ] {
        conn.execute("INSERT OR IGNORE INTO experiment_nodes(id,experiment_id,kind,title,status,config_json,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?7)", params![id, experiment_id, kind, label, status, config.to_string(), at])?;
    }
    for (source, target, relation) in [(&baseline, &variant, "forked_from"), (&variant, &result, "evaluated")] {
        let edge_id = format!("edge:{source}:{relation}:{target}");
        conn.execute("INSERT OR IGNORE INTO experiment_edges(id,experiment_id,source_node_id,target_node_id,relation,created_at) VALUES(?1,?2,?3,?4,?5,?6)", params![edge_id, experiment_id, source, target, relation, at])?;
    }
    Ok(())
}

fn load_nodes(conn: &Connection, experiment_id: &str) -> Result<Vec<ExperimentNode>> {
    let mut stmt = conn.prepare("SELECT id,kind,title,status,config_json,metrics_json,cost_usd,artifact_refs_json,trace_refs_json,provenance_json,created_at,updated_at FROM experiment_nodes WHERE experiment_id=?1 ORDER BY created_at,id")?;
    let rows = stmt.query_map([experiment_id], |row| Ok(ExperimentNode {
        id: row.get(0)?, kind: row.get(1)?, title: row.get(2)?, status: row.get(3)?,
        config: OpaqueJson(serde_json::from_str(&row.get::<_, String>(4)?).unwrap_or_default()),
        metrics: row.get::<_, Option<String>>(5)?.and_then(|v| serde_json::from_str(&v).ok()).map(OpaqueJson), cost_usd: row.get(6)?,
        artifact_refs: serde_json::from_str(&row.get::<_, String>(7)?).unwrap_or_default(), trace_refs: serde_json::from_str(&row.get::<_, String>(8)?).unwrap_or_default(),
        provenance: OpaqueJson(serde_json::from_str(&row.get::<_, String>(9)?).unwrap_or_default()), created_at: row.get(10)?, updated_at: row.get(11)?,
    }))?.collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

fn load_edges(conn: &Connection, experiment_id: &str) -> Result<Vec<ExperimentEdge>> {
    let mut stmt = conn.prepare("SELECT id,source_node_id,target_node_id,relation,created_at FROM experiment_edges WHERE experiment_id=?1 ORDER BY created_at,id")?;
    let rows = stmt.query_map([experiment_id], |row| Ok(ExperimentEdge { id: row.get(0)?, source_node_id: row.get(1)?, target_node_id: row.get(2)?, relation: row.get(3)?, created_at: row.get(4)? }))?.collect::<rusqlite::Result<_>>()?;
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
    fn search_and_reopen_return_the_same_durable_identity() {
        let conn = database();
        let created = attach(&conn, "task_craftax", MEMBER_CAMPAIGN, "run_1", "2026-08-24T12:00:00Z", "Craftax prompt variant").unwrap();
        let found = list(&conn, Some("craftax")).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, created.id);
        assert_eq!(get(&conn, &created.id).unwrap().unwrap().id, created.id);
    }
}
