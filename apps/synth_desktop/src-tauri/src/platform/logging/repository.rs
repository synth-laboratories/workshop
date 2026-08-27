use anyhow::Result;
use rusqlite::{params, Connection};

use super::record::LogRecord;
use crate::platform::failure::redaction::{redact_text, redact_value};

pub fn insert(conn: &Connection, record: &LogRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO log_records(
            log_id, level, component, event, message, operation_id, failure_id, fields_json, at
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            record.log_id,
            record.level.as_str(),
            record.component,
            record.event,
            redact_text(&record.message),
            record.operation_id.as_ref().map(|id| id.as_str().to_owned()),
            record.failure_id.as_ref().map(|id| id.as_str().to_owned()),
            redact_value(record.fields.clone()).to_string(),
            record.at.to_rfc3339(),
        ],
    )?;
    Ok(())
}
