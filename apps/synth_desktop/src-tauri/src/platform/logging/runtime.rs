use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use std::sync::Arc;

use super::emergency_sink;
use super::query::{LogQuery, LogQueryResult};
use super::record::{LogLevel, LogRecord};
use super::repository;
use crate::platform::failure::{FailureId, PersistenceFailure};
use crate::platform::operations::OperationId;
use crate::storage::Database;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservabilityMode {
    Durable,
    EmergencyFile,
}

#[derive(Clone)]
pub struct LogRuntime {
    db: Option<Arc<Database>>,
    data_root: std::path::PathBuf,
    mode: ObservabilityMode,
}

impl LogRuntime {
    pub fn durable(db: Arc<Database>, data_root: std::path::PathBuf) -> Self {
        Self {
            db: Some(db),
            data_root,
            mode: ObservabilityMode::Durable,
        }
    }

    pub fn emergency(data_root: std::path::PathBuf) -> Self {
        Self {
            db: None,
            data_root,
            mode: ObservabilityMode::EmergencyFile,
        }
    }

    pub fn mode(&self) -> ObservabilityMode {
        self.mode
    }

    pub fn emit(&self, record: LogRecord) -> Result<()> {
        match self.mode {
            ObservabilityMode::Durable => {
                if let Some(db) = &self.db {
                    db.with_conn(|conn| repository::insert(conn, &record))?;
                }
                Ok(())
            }
            ObservabilityMode::EmergencyFile => {
                emergency_sink::write_record(&self.data_root, &record)
            }
        }
    }

    pub fn info(&self, component: &str, event: &str, message: impl Into<String>) {
        let _ = self.emit(LogRecord::new(LogLevel::Info, component, event, message.into()));
    }

    pub fn error(&self, component: &str, event: &str, message: impl Into<String>) {
        let _ = self.emit(LogRecord::new(LogLevel::Error, component, event, message.into()));
    }

    pub fn query(&self, query: LogQuery) -> Result<LogQueryResult> {
        match &self.db {
            Some(db) => db.with_conn(|conn| super::query::execute(conn, &query)),
            None => Ok(LogQueryResult {
                count: 0,
                records: Vec::new(),
            }),
        }
    }

    pub fn import_emergency(&self, conn: &Connection) -> Result<Option<String>> {
        let lines = emergency_sink::load_lines(&self.data_root)?;
        if lines.is_empty() {
            return Ok(None);
        }
        let import_id = format!("import_{}", uuid::Uuid::new_v4().simple());
        let now = Utc::now().to_rfc3339();
        for line in &lines {
            let record = LogRecord {
                log_id: line
                    .get("logId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("log_emergency")
                    .to_owned(),
                level: LogLevel::parse(line.get("level").and_then(|v| v.as_str()).unwrap_or("error")),
                component: line
                    .get("component")
                    .and_then(|v| v.as_str())
                    .unwrap_or("bootstrap")
                    .to_owned(),
                event: line
                    .get("event")
                    .and_then(|v| v.as_str())
                    .unwrap_or("emergency")
                    .to_owned(),
                message: line
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned(),
                operation_id: line
                    .get("operationId")
                    .and_then(|v| v.as_str())
                    .map(|id| OperationId(id.to_owned())),
                failure_id: line
                    .get("failureId")
                    .and_then(|v| v.as_str())
                    .map(|id| FailureId(id.to_owned())),
                fields: line.get("fields").cloned().unwrap_or(serde_json::Value::Null),
                at: chrono::DateTime::parse_from_rfc3339(
                    line.get("at").and_then(|v| v.as_str()).unwrap_or(&now),
                )
                .map(|t| t.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| Utc::now()),
            };
            repository::insert(conn, &record)?;
        }
        crate::platform::failure::FailureRuntime::raise_in_tx(
            conn,
            crate::platform::failure::FailureKind::Persistence(PersistenceFailure::SqliteUnavailable {
                detail: format!("imported {} emergency records", lines.len()),
            }),
            crate::platform::operations::OperationContext::bootstrap("import"),
            crate::platform::operations::OperationKind::Bootstrap,
            crate::platform::operations::OperationPhase::Recover,
            None,
            "emergency_import",
        )?;
        emergency_sink::rotate(&self.data_root)?;
        Ok(Some(import_id))
    }
}
