//! Bounded v0.5 experiment grouping: one session-owned DAG of campaigns and
//! optimizer runs. Not the v0.6 canvas or reports engine.
//!
//! A chat that starts an evaluation campaign and a GEPA run should be able to
//! name both as members of the same experiment without either leaking into
//! another task's right pane.

use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::storage::{append_event, EventAppend, EventSource};

pub const MEMBER_CAMPAIGN: &str = "eval_campaign";
pub const MEMBER_OPTIMIZER: &str = "optimizer_run";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentMember {
    pub member_kind: String,
    pub member_id: String,
    pub title: String,
    pub attached_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExperimentGroup {
    pub id: String,
    pub session_id: String,
    pub title: String,
    pub created_at: String,
    pub members: Vec<ExperimentMember>,
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
            "SELECT id, session_id, title, created_at FROM experiment_groups WHERE session_id = ?1",
            params![session_id],
            |row| {
                Ok(ExperimentGroup {
                    id: row.get(0)?,
                    session_id: row.get(1)?,
                    title: row.get(2)?,
                    created_at: row.get(3)?,
                    members: Vec::new(),
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
    Ok(Some(group))
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
        "INSERT INTO experiment_groups(id, session_id, title, created_at)
         VALUES(?1, ?2, ?3, ?4)",
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
    }
}
