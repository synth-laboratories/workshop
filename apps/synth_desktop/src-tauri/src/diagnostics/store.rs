//! Journal-backed diagnostic store.
//!
//! The SQLite journal is authoritative for diagnostics exactly as it is for
//! every other event. VictoriaLogs is a search index over these rows: it
//! returns journal sequences, and the records themselves are always read back
//! from here. That split is why restarting or wiping the index cannot change
//! what a query returns, and why replay after a restart produces no duplicate
//! logical events.

use super::event::{DiagnosticEvent, Severity, JOURNAL_KIND};
use super::query::DiagnosticQuery;
use crate::storage::{Database, EventAppend, EventJournal, EventSource};
use anyhow::Result;
use rusqlite::{types::Value as SqlValue, Connection};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

/// A diagnostic plus the journal sequence that makes it addressable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiagnosticRecord {
    pub sequence: i64,
    pub event: DiagnosticEvent,
}

impl DiagnosticRecord {
    pub fn to_json(&self) -> Value {
        let mut value = self.event.to_payload();
        if let Some(object) = value.as_object_mut() {
            object.insert("journal_sequence".into(), Value::from(self.sequence));
        }
        value
    }
}

#[derive(Clone)]
pub struct DiagnosticStore {
    db: Arc<Database>,
    journal: EventJournal,
}

impl DiagnosticStore {
    pub fn new(db: Arc<Database>, journal: EventJournal) -> Self {
        Self { db, journal }
    }

    pub(crate) fn database(&self) -> &Arc<Database> {
        &self.db
    }

    /// Persist a batch of diagnostics in one transaction.
    ///
    /// Correlation identities stay in the payload and are deliberately *not*
    /// mirrored onto the journal row's structural columns. Those columns are
    /// referential: `run_id` has a foreign key to `runs`, and a `session_id`
    /// mints a session row and consumes a session sequence. A diagnostic
    /// naming a rollout that is not a local run would fail the whole batch,
    /// and one naming a stale session would fabricate a session and shift the
    /// cursors the transcript pages on. Observing a failure must never mutate
    /// the state being observed.
    pub async fn append_batch(
        &self,
        events: Vec<DiagnosticEvent>,
    ) -> Result<Vec<DiagnosticRecord>> {
        if events.is_empty() {
            return Ok(Vec::new());
        }
        let inputs: Vec<EventAppend> = events
            .iter()
            .map(|event| EventAppend {
                event_id: Some(event.event_id.clone()),
                session_id: None,
                run_id: None,
                source: EventSource::System,
                kind: JOURNAL_KIND.into(),
                payload: event.to_payload(),
                remote_sequence: None,
                // `command_id` carries no foreign key and no sequencing.
                command_id: event.correlation.command_id.clone(),
                created_at: Some(event.timestamp.clone()),
            })
            .collect();
        let appended = self.journal.append_batch(inputs).await?;
        Ok(appended
            .into_iter()
            .zip(events)
            .map(|(row, event)| DiagnosticRecord {
                sequence: row.sequence,
                event,
            })
            .collect())
    }

    /// Typed search over the journal. This is both the pre-index authority and
    /// the automatic fallback whenever VictoriaLogs is unavailable.
    pub async fn search(&self, query: DiagnosticQuery) -> Result<Vec<DiagnosticRecord>> {
        let now = chrono::Utc::now();
        let (sql, params) = compile_sql(&query, now);
        self.db
            .run(move |conn| run_records(conn, &sql, params))
            .await
    }

    /// Load specific sequences (the index returns identities, never records).
    pub async fn load_by_sequences(&self, sequences: Vec<i64>) -> Result<Vec<DiagnosticRecord>> {
        if sequences.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = (0..sequences.len())
            .map(|index| format!("?{}", index + 1))
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT sequence, payload_json FROM events
             WHERE kind = '{JOURNAL_KIND}' AND sequence IN ({placeholders})
             ORDER BY sequence DESC"
        );
        let params: Vec<SqlValue> = sequences.into_iter().map(SqlValue::from).collect();
        self.db
            .run(move |conn| run_records(conn, &sql, params))
            .await
    }

    /// Sequence-ordered feed for the indexer.
    pub async fn records_after(
        &self,
        after_sequence: i64,
        limit: i64,
    ) -> Result<Vec<DiagnosticRecord>> {
        let events = self
            .journal
            .events_of_kinds_after(after_sequence, vec![JOURNAL_KIND.into()], limit)
            .await?;
        Ok(events
            .into_iter()
            .filter_map(|event| {
                DiagnosticEvent::from_payload(&event.payload).map(|diagnostic| DiagnosticRecord {
                    sequence: event.sequence,
                    event: diagnostic,
                })
            })
            .collect())
    }

    /// Highest diagnostic sequence, for indexer lag reporting.
    pub async fn head_sequence(&self) -> Result<i64> {
        self.db
            .run(|conn| {
                let head: i64 = conn.query_row(
                    &format!(
                        "SELECT COALESCE(MAX(sequence), 0) FROM events WHERE kind = '{JOURNAL_KIND}'"
                    ),
                    [],
                    |row| row.get(0),
                )?;
                Ok(head)
            })
            .await
    }

    /// Trim diagnostics beyond the retention window and the row ceiling.
    ///
    /// The index keeps 7 days; the journal keeps longer, because it is the
    /// authority the index is rebuilt from. Only `diagnostic.event` rows are
    /// ever considered — sessions, runs, approvals, visuals, and seals are
    /// other kinds and must not be reachable from here at all.
    ///
    /// Trimming below the indexer's cursor is safe: the cursor only moves
    /// forward, so a deleted row is one the index already holds or one that
    /// aged out of the index's own window anyway.
    pub async fn trim(&self, keep: Duration, max_rows: i64) -> Result<usize> {
        let cutoff = (chrono::Utc::now()
            - chrono::Duration::from_std(keep).unwrap_or_else(|_| chrono::Duration::days(30)))
        .to_rfc3339();
        self.db
            .run(move |conn| {
                let by_age = conn.execute(
                    &format!(
                        "DELETE FROM events WHERE kind = '{JOURNAL_KIND}' AND created_at < ?1"
                    ),
                    rusqlite::params![cutoff],
                )?;
                // A single burst can blow the row ceiling long before anything
                // reaches the age cutoff, so bound by count as well.
                let by_count = conn.execute(
                    &format!(
                        "DELETE FROM events WHERE kind = '{JOURNAL_KIND}' AND sequence <= (
                             SELECT sequence FROM events
                             WHERE kind = '{JOURNAL_KIND}'
                             ORDER BY sequence DESC
                             LIMIT 1 OFFSET ?1
                         )"
                    ),
                    rusqlite::params![max_rows],
                )?;
                Ok(by_age + by_count)
            })
            .await
    }

    /// Stored diagnostic count and the oldest retained timestamp.
    pub async fn summary(&self) -> Result<(i64, Option<String>)> {
        self.db
            .run(|conn| {
                let row = conn.query_row(
                    &format!(
                        "SELECT COUNT(*), MIN(created_at) FROM events WHERE kind = '{JOURNAL_KIND}'"
                    ),
                    [],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?)),
                )?;
                Ok(row)
            })
            .await
    }
}

/// Compile a typed query into one parameterized statement.
///
/// Nothing from the caller is ever concatenated into the SQL text: filter
/// values are bound, and the only interpolated fragments are placeholder
/// indices and the fixed field names this module owns.
fn compile_sql(
    query: &DiagnosticQuery,
    now: chrono::DateTime<chrono::Utc>,
) -> (String, Vec<SqlValue>) {
    let mut clauses = vec![format!("kind = '{JOURNAL_KIND}'")];
    let mut params: Vec<SqlValue> = Vec::new();

    let bind = |params: &mut Vec<SqlValue>, value: SqlValue| -> String {
        params.push(value);
        format!("?{}", params.len())
    };

    let start = bind(&mut params, SqlValue::from(query.start_timestamp(now)));
    clauses.push(format!("created_at >= {start}"));
    if let Some(end) = query.end_timestamp(now) {
        let placeholder = bind(&mut params, SqlValue::from(end));
        clauses.push(format!("created_at < {placeholder}"));
    }
    if let Some(cursor) = query.cursor {
        let placeholder = bind(&mut params, SqlValue::from(cursor));
        clauses.push(format!("sequence < {placeholder}"));
    }

    let in_clause = |params: &mut Vec<SqlValue>,
                     clauses: &mut Vec<String>,
                     field: &str,
                     values: Vec<String>| {
        if values.is_empty() {
            return;
        }
        let placeholders = values
            .into_iter()
            .map(|value| {
                params.push(SqlValue::from(value));
                format!("?{}", params.len())
            })
            .collect::<Vec<_>>()
            .join(",");
        clauses.push(format!(
            "json_extract(payload_json, '$.{field}') IN ({placeholders})"
        ));
    };

    in_clause(
        &mut params,
        &mut clauses,
        "component",
        query.components.clone(),
    );
    in_clause(
        &mut params,
        &mut clauses,
        "severity",
        query
            .severities
            .iter()
            .map(|severity| severity.as_str().to_owned())
            .collect(),
    );
    in_clause(&mut params, &mut clauses, "code", query.codes.clone());
    in_clause(&mut params, &mut clauses, "event", query.events.clone());

    for field in super::event::CORRELATION_FIELDS {
        if let Some(value) = query.correlation.get(field) {
            params.push(SqlValue::from(value.to_owned()));
            clauses.push(format!(
                "json_extract(payload_json, '$.{field}') = ?{}",
                params.len()
            ));
        }
    }

    params.push(SqlValue::from(query.limit as i64));
    let sql = format!(
        "SELECT sequence, payload_json FROM events
         WHERE {}
         ORDER BY sequence DESC
         LIMIT ?{}",
        clauses.join(" AND "),
        params.len()
    );
    (sql, params)
}

fn run_records(
    conn: &Connection,
    sql: &str,
    params: Vec<SqlValue>,
) -> Result<Vec<DiagnosticRecord>> {
    let mut statement = conn.prepare(sql)?;
    let rows = statement.query_map(rusqlite::params_from_iter(params), |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut records = Vec::new();
    for row in rows {
        let (sequence, payload) = row?;
        let Ok(value) = serde_json::from_str::<Value>(&payload) else {
            continue;
        };
        // A payload that no longer parses as the current envelope is skipped
        // rather than failing the query: a diagnostic surface that goes dark
        // because one old row is unreadable is worse than a short answer.
        if let Some(event) = DiagnosticEvent::from_payload(&value) {
            records.push(DiagnosticRecord { sequence, event });
        }
    }
    Ok(records)
}

/// Group a result set by stable code, newest first. Shared by
/// `diagnostics_explain` and the Diagnostics pane so both agree.
pub fn group_by_code(records: &[DiagnosticRecord]) -> Vec<Value> {
    let mut order: Vec<String> = Vec::new();
    let mut groups: std::collections::HashMap<String, (usize, Severity, String, String, String)> =
        std::collections::HashMap::new();
    for record in records {
        let entry = groups.entry(record.event.code.clone()).or_insert_with(|| {
            order.push(record.event.code.clone());
            (
                0,
                record.event.severity,
                record.event.component.clone(),
                record.event.message.clone(),
                record.event.timestamp.clone(),
            )
        });
        entry.0 += 1;
        entry.1 = entry.1.max(record.event.severity);
        if record.event.timestamp < entry.4 {
            entry.4 = record.event.timestamp.clone();
        }
    }
    order
        .into_iter()
        .map(|code| {
            let (count, severity, component, message, first_seen) =
                groups.remove(&code).expect("group");
            serde_json::json!({
                "code": code,
                "count": count,
                "severity": severity.as_str(),
                "component": component,
                "message": message,
                "first_seen": first_seen,
            })
        })
        .collect()
}

