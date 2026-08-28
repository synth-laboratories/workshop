use serde::{Deserialize, Serialize};

use super::record::{LogLevel, LogRecord};
use crate::platform::failure::definition::FailureId;
use crate::platform::operations::OperationId;

#[derive(Clone, Debug, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LogQuery {
    pub level: Option<String>,
    pub component: Option<String>,
    pub operation_id: Option<String>,
    pub failure_id: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LogQueryResult {
    pub count: u32,
    pub records: Vec<LogView>,
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LogView {
    pub log_id: String,
    pub level: String,
    pub component: String,
    pub event: String,
    pub message: String,
    pub operation_id: Option<String>,
    pub failure_id: Option<String>,
    pub at: String,
}

pub fn execute(conn: &rusqlite::Connection, query: &LogQuery) -> anyhow::Result<LogQueryResult> {
    let mut sql = String::from("SELECT log_id, level, component, event, message, operation_id, failure_id, at FROM log_records WHERE 1=1");
    let mut params: Vec<String> = Vec::new();
    if let Some(level) = &query.level {
        sql.push_str(" AND level = ?");
        params.push(level.clone());
    }
    if let Some(component) = &query.component {
        sql.push_str(" AND component = ?");
        params.push(component.clone());
    }
    if let Some(operation_id) = &query.operation_id {
        sql.push_str(" AND operation_id = ?");
        params.push(operation_id.clone());
    }
    if let Some(failure_id) = &query.failure_id {
        sql.push_str(" AND failure_id = ?");
        params.push(failure_id.clone());
    }
    if let Some(since) = &query.since {
        sql.push_str(" AND at >= ?");
        params.push(since.clone());
    }
    if let Some(until) = &query.until {
        sql.push_str(" AND at <= ?");
        params.push(until.clone());
    }
    sql.push_str(" ORDER BY at DESC LIMIT ?");
    let limit = query.limit.unwrap_or(200).min(1000) as i64;
    let mut stmt = conn.prepare(&sql)?;
    let mut bindings: Vec<&dyn rusqlite::types::ToSql> = params
        .iter()
        .map(|p| p as &dyn rusqlite::types::ToSql)
        .collect();
    bindings.push(&limit);
    let records = stmt
        .query_map(bindings.as_slice(), |row| {
            Ok(LogView {
                log_id: row.get(0)?,
                level: row.get(1)?,
                component: row.get(2)?,
                event: row.get(3)?,
                message: row.get(4)?,
                operation_id: row.get(5)?,
                failure_id: row.get(6)?,
                at: row.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(LogQueryResult {
        count: records.len() as u32,
        records,
    })
}

pub fn for_failure(conn: &rusqlite::Connection, failure_id: &str) -> anyhow::Result<Vec<LogView>> {
    execute(
        conn,
        &LogQuery {
            failure_id: Some(failure_id.into()),
            limit: Some(200),
            ..LogQuery::default()
        },
    )
    .map(|r| r.records)
}

#[allow(dead_code)]
pub fn hydrate(record: &LogRecord) -> LogView {
    LogView {
        log_id: record.log_id.clone(),
        level: record.level.as_str().into(),
        component: record.component.clone(),
        event: record.event.clone(),
        message: record.message.clone(),
        operation_id: record.operation_id.as_ref().map(|id| id.0.clone()),
        failure_id: record.failure_id.as_ref().map(|id| id.0.clone()),
        at: record.at.to_rfc3339(),
    }
}

#[allow(dead_code)]
pub fn parse_ids(record: &LogRecord) -> (Option<OperationId>, Option<FailureId>) {
    (record.operation_id.clone(), record.failure_id.clone())
}

#[allow(dead_code)]
pub fn parse_level(value: &str) -> LogLevel {
    LogLevel::parse(value)
}
