use serde::{Deserialize, Serialize};

use super::occurrence::OperationalFailure;
use super::projection::FailureView;
use super::repository::FailureRepository;

#[derive(Clone, Debug, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FailureQuery {
    pub code: Option<String>,
    pub domain: Option<String>,
    pub lifecycle_state: Option<String>,
    pub session_id: Option<String>,
    pub container_id: Option<String>,
    pub evaluation_id: Option<String>,
    pub rollout_id: Option<String>,
    pub visual_id: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FailureQueryResult {
    pub count: u32,
    pub failures: Vec<FailureView>,
}

impl FailureQuery {
    pub fn execute(&self, conn: &rusqlite::Connection) -> anyhow::Result<FailureQueryResult> {
        let mut sql = String::from("SELECT failure_id FROM failure_occurrences WHERE 1=1");
        let mut params: Vec<String> = Vec::new();
        if let Some(code) = &self.code {
            sql.push_str(" AND code = ?");
            params.push(code.clone());
        }
        if let Some(domain) = &self.domain {
            sql.push_str(" AND domain = ?");
            params.push(domain.clone());
        }
        if let Some(state) = &self.lifecycle_state {
            sql.push_str(" AND lifecycle_state = ?");
            params.push(state.clone());
        }
        if let Some(session_id) = &self.session_id {
            sql.push_str(" AND session_id = ?");
            params.push(session_id.clone());
        }
        if let Some(container_id) = &self.container_id {
            sql.push_str(" AND container_id = ?");
            params.push(container_id.clone());
        }
        if let Some(evaluation_id) = &self.evaluation_id {
            sql.push_str(" AND evaluation_id = ?");
            params.push(evaluation_id.clone());
        }
        if let Some(rollout_id) = &self.rollout_id {
            sql.push_str(" AND rollout_id = ?");
            params.push(rollout_id.clone());
        }
        if let Some(visual_id) = &self.visual_id {
            sql.push_str(" AND visual_id = ?");
            params.push(visual_id.clone());
        }
        if let Some(since) = &self.since {
            sql.push_str(" AND raised_at >= ?");
            params.push(since.clone());
        }
        if let Some(until) = &self.until {
            sql.push_str(" AND raised_at <= ?");
            params.push(until.clone());
        }
        sql.push_str(" ORDER BY raised_at DESC LIMIT ?");
        let limit = self.limit.unwrap_or(100).min(500) as i64;
        let mut stmt = conn.prepare(&sql)?;
        let mut bindings: Vec<&dyn rusqlite::types::ToSql> = params
            .iter()
            .map(|p| p as &dyn rusqlite::types::ToSql)
            .collect();
        bindings.push(&limit);
        let ids = stmt
            .query_map(bindings.as_slice(), |row| row.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut failures = Vec::new();
        for id in ids {
            if let Some(row) = FailureRepository::load(conn, &id)? {
                failures.push(FailureView::from_occurrence(&row));
            }
        }
        Ok(FailureQueryResult {
            count: failures.len() as u32,
            failures,
        })
    }
}

pub fn get_view(
    conn: &rusqlite::Connection,
    failure_id: &str,
) -> anyhow::Result<Option<FailureView>> {
    Ok(FailureRepository::load(conn, failure_id)?.map(|row| FailureView::from_occurrence(&row)))
}

pub fn timeline(
    conn: &rusqlite::Connection,
    failure_id: &str,
) -> anyhow::Result<Vec<serde_json::Value>> {
    let mut stmt = conn.prepare(
        "SELECT sequence, from_state, to_state, reason, actor, at
         FROM failure_transitions WHERE failure_id = ?1 ORDER BY sequence ASC",
    )?;
    let rows = stmt.query_map([failure_id], |row| {
        Ok(serde_json::json!({
            "sequence": row.get::<_, i64>(0)?,
            "from": row.get::<_, Option<String>>(1)?,
            "to": row.get::<_, String>(2)?,
            "reason": row.get::<_, String>(3)?,
            "actor": row.get::<_, String>(4)?,
            "at": row.get::<_, String>(5)?,
        }))
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

#[allow(dead_code)]
pub fn load_occurrence(
    conn: &rusqlite::Connection,
    failure_id: &str,
) -> anyhow::Result<Option<OperationalFailure>> {
    FailureRepository::load(conn, failure_id)
}
