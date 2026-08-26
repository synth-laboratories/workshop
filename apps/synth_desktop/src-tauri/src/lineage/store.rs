//! SQLite load/project for lineage nodes and edges.
//!
//! Attach writes one node per member. Edges are typed facts (`evaluated` when
//! an optimizer and an eval share an experiment). The canvas consumes `DagView`;
//! it does not invent kinds.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};

use crate::contract::specta::OpaqueJson;

use super::models::{DagView, ExperimentEdge, ExperimentEvidenceRef, ExperimentNode};

pub fn member_node_id(experiment_id: &str, member_kind: &str, member_id: &str) -> String {
    format!("{experiment_id}:{member_kind}:{member_id}")
}

pub fn load_graph(conn: &Connection, experiment_id: &str) -> Result<DagView> {
    Ok(DagView {
        nodes: load_nodes(conn, experiment_id)?,
        edges: load_edges(conn, experiment_id)?,
    })
}

pub fn load_nodes(conn: &Connection, experiment_id: &str) -> Result<Vec<ExperimentNode>> {
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

pub fn load_edges(conn: &Connection, experiment_id: &str) -> Result<Vec<ExperimentEdge>> {
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

pub fn latest_node_id(conn: &Connection, experiment_id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT id FROM experiment_nodes WHERE experiment_id=?1 ORDER BY created_at DESC,id DESC LIMIT 1",
            [experiment_id],
            |row| row.get(0),
        )
        .optional()?)
}

pub fn resolve_member_node_id(
    conn: &Connection,
    experiment_id: &str,
    member_kind: &str,
    member_id: &str,
) -> Result<Option<String>> {
    let id = member_node_id(experiment_id, member_kind, member_id);
    let exists: Option<String> = conn
        .query_row(
            "SELECT id FROM experiment_nodes WHERE id=?1 AND experiment_id=?2",
            params![id, experiment_id],
            |row| row.get(0),
        )
        .optional()?;
    if exists.is_some() {
        return Ok(exists);
    }
    latest_node_id(conn, experiment_id)
}

/// One node per attached member. When an optimizer and an eval share the
/// experiment, write `evaluated` (optimizer → eval). No synthetic kinds.
pub fn project_member(
    conn: &Connection,
    experiment_id: &str,
    member_kind: &str,
    member_id: &str,
    title: &str,
    at: &str,
) -> Result<()> {
    let id = member_node_id(experiment_id, member_kind, member_id);
    let config = serde_json::json!({"memberKind":member_kind,"memberId":member_id});
    conn.execute(
        "INSERT OR IGNORE INTO experiment_nodes(id,experiment_id,kind,title,status,config_json,created_at,updated_at) VALUES(?1,?2,?3,?4,'running',?5,?6,?6)",
        params![id, experiment_id, member_kind, title, config.to_string(), at],
    )?;
    link_eval_to_optimizers(conn, experiment_id, member_kind, &id, at)?;
    Ok(())
}

fn link_eval_to_optimizers(
    conn: &Connection,
    experiment_id: &str,
    member_kind: &str,
    node_id: &str,
    at: &str,
) -> Result<()> {
    let (source_kind, target_kind, source_is_new) = match member_kind {
        "eval_campaign" => ("optimizer_run", "eval_campaign", false),
        "optimizer_run" => ("optimizer_run", "eval_campaign", true),
        _ => return Ok(()),
    };
    let counterparts = if source_is_new {
        load_ids_of_kind(conn, experiment_id, target_kind)?
    } else {
        load_ids_of_kind(conn, experiment_id, source_kind)?
    };
    for counterpart in counterparts {
        if counterpart == node_id {
            continue;
        }
        let (source, target) = if source_is_new {
            (node_id, counterpart.as_str())
        } else {
            (counterpart.as_str(), node_id)
        };
        insert_edge(conn, experiment_id, source, target, "evaluated", at)?;
    }
    Ok(())
}

fn load_ids_of_kind(conn: &Connection, experiment_id: &str, kind: &str) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT id FROM experiment_nodes WHERE experiment_id=?1 AND kind=?2")?;
    let rows = stmt
        .query_map(params![experiment_id, kind], |row| row.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn insert_edge(
    conn: &Connection,
    experiment_id: &str,
    source_node_id: &str,
    target_node_id: &str,
    relation: &str,
    at: &str,
) -> Result<()> {
    let edge_id = format!("edge:{source_node_id}:{relation}:{target_node_id}");
    conn.execute(
        "INSERT OR IGNORE INTO experiment_edges(id,experiment_id,source_node_id,target_node_id,relation,created_at) VALUES(?1,?2,?3,?4,?5,?6)",
        params![edge_id, experiment_id, source_node_id, target_node_id, relation, at],
    )?;
    Ok(())
}

pub fn settle_member_nodes(
    conn: &Connection,
    experiment_id: &str,
    member_kind: &str,
    member_id: &str,
    group_status: &str,
    task: &str,
    model: Option<&str>,
    result: &serde_json::Value,
    trace_refs: &[String],
    at: &str,
) -> Result<()> {
    let id = member_node_id(experiment_id, member_kind, member_id);
    conn.execute(
        "UPDATE experiment_nodes SET status=?2, metrics_json=?3, trace_refs_json=?4, provenance_json=?5, updated_at=?6 WHERE id=?1",
        params![
            id,
            group_status,
            result.to_string(),
            serde_json::to_string(trace_refs)?,
            serde_json::json!({"memberKind":member_kind,"memberId":member_id,"task":task,"model":model}).to_string(),
            at
        ],
    )?;
    Ok(())
}

pub fn update_evidence_refs(
    conn: &Connection,
    node_id: &str,
    refs: &[ExperimentEvidenceRef],
    at: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE experiment_nodes SET evidence_refs_json=?2,updated_at=?3 WHERE id=?1",
        params![node_id, serde_json::to_string(refs)?, at],
    )?;
    Ok(())
}

pub fn load_node_evidence(
    conn: &Connection,
    node_id: &str,
    experiment_id: &str,
) -> Result<(String, Vec<ExperimentEvidenceRef>)> {
    let (session_id, raw): (String, String) = conn.query_row(
        "SELECT g.session_id,n.evidence_refs_json FROM experiment_nodes n JOIN experiment_groups g ON g.id=n.experiment_id WHERE n.id=?1 AND n.experiment_id=?2",
        params![node_id, experiment_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let refs = serde_json::from_str(&raw).unwrap_or_default();
    Ok((session_id, refs))
}
