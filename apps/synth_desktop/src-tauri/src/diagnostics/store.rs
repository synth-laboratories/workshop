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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::event::{validate, DiagnosticInput};
    use crate::storage::Storage;
    use serde_json::json;
    use tempfile::tempdir;

    fn store(root: &std::path::Path) -> DiagnosticStore {
        let storage = Storage::open(root).unwrap();
        let journal = EventJournal::new(storage.database().clone());
        DiagnosticStore::new(storage.database().clone(), journal)
    }

    fn sample(component: &str, code: &str, severity: Severity) -> DiagnosticEvent {
        validate(DiagnosticInput::new(
            severity,
            component,
            "test.event",
            code,
            "sample diagnostic",
        ))
        .unwrap()
    }

    #[tokio::test]
    async fn persists_and_reads_back_a_batch() {
        let dir = tempdir().unwrap();
        let store = store(dir.path());
        let written = store
            .append_batch(vec![
                sample("renderer", "load_failed", Severity::Error),
                sample("containers", "capability_rejected", Severity::Warn),
            ])
            .await
            .unwrap();
        assert_eq!(written.len(), 2);
        assert!(written[0].sequence < written[1].sequence);

        let all = store.search(DiagnosticQuery::default()).await.unwrap();
        assert_eq!(all.len(), 2);
        // Newest first.
        assert_eq!(all[0].event.code, "capability_rejected");
        assert_eq!(store.head_sequence().await.unwrap(), written[1].sequence);
    }

    #[tokio::test]
    async fn filters_by_label_and_correlation() {
        let dir = tempdir().unwrap();
        let store = store(dir.path());
        let mut projection = DiagnosticInput::new(
            Severity::Error,
            "visual-host",
            "visual.projection.rejected",
            "unsupported_trace_projection_schema",
            "Unsupported trace projection schema: synth.trace.v5",
        );
        projection.correlation.visual_id = Some("vis_9".into());
        projection.correlation.trace_id = Some("trace_1".into());
        store
            .append_batch(vec![
                validate(projection).unwrap(),
                sample("renderer", "load_failed", Severity::Warn),
            ])
            .await
            .unwrap();

        let by_visual = store
            .search(DiagnosticQuery {
                correlation: crate::diagnostics::event::Correlation {
                    visual_id: Some("vis_9".into()),
                    ..Default::default()
                },
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(by_visual.len(), 1);
        assert_eq!(by_visual[0].event.details.len(), 0);
        assert_eq!(
            by_visual[0].event.code,
            "unsupported_trace_projection_schema"
        );

        let by_severity = store
            .search(DiagnosticQuery {
                severities: vec![Severity::Error],
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(by_severity.len(), 1);

        let by_component = store
            .search(DiagnosticQuery {
                components: vec!["renderer".into()],
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(by_component.len(), 1);
        assert_eq!(by_component[0].event.component, "renderer");
    }

    #[tokio::test]
    async fn cursor_pages_backwards_without_repeating_a_row() {
        let dir = tempdir().unwrap();
        let store = store(dir.path());
        store
            .append_batch(
                (0..5)
                    .map(|index| {
                        let mut input = DiagnosticInput::new(
                            Severity::Info,
                            "renderer",
                            "test.event",
                            "test_code",
                            format!("event {index}"),
                        );
                        // Inside the default window: a query's job is to be
                        // bounded, and a fixture dated outside that bound tests
                        // nothing about paging.
                        input.timestamp = Some(
                            (chrono::Utc::now() - chrono::Duration::seconds(10 - index))
                                .to_rfc3339(),
                        );
                        validate(input).unwrap()
                    })
                    .collect(),
            )
            .await
            .unwrap();

        let first = store
            .search(DiagnosticQuery {
                limit: 2,
                ..Default::default()
            })
            .await
            .unwrap();
        let second = store
            .search(DiagnosticQuery {
                limit: 2,
                cursor: Some(first.last().unwrap().sequence),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(first.len(), 2);
        assert_eq!(second.len(), 2);
        assert!(second[0].sequence < first[1].sequence);
    }

    #[tokio::test]
    async fn the_window_bounds_the_result_set() {
        let dir = tempdir().unwrap();
        let store = store(dir.path());
        let mut old = DiagnosticInput::new(
            Severity::Error,
            "renderer",
            "test.event",
            "test_code",
            "ancient",
        );
        old.timestamp = Some("2020-01-01T00:00:00Z".into());
        store
            .append_batch(vec![
                validate(old).unwrap(),
                sample("renderer", "recent_code", Severity::Error),
            ])
            .await
            .unwrap();

        let recent = store
            .search(DiagnosticQuery {
                since: std::time::Duration::from_secs(600),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].event.code, "recent_code");
    }

    #[tokio::test]
    async fn sequences_load_exactly_what_the_index_named() {
        let dir = tempdir().unwrap();
        let store = store(dir.path());
        let written = store
            .append_batch(vec![
                sample("renderer", "first_code", Severity::Info),
                sample("renderer", "second_code", Severity::Info),
                sample("renderer", "third_code", Severity::Info),
            ])
            .await
            .unwrap();
        let loaded = store
            .load_by_sequences(vec![written[0].sequence, written[2].sequence])
            .await
            .unwrap();
        let codes: Vec<&str> = loaded.iter().map(|r| r.event.code.as_str()).collect();
        assert_eq!(codes, vec!["third_code", "first_code"]);
    }

    #[tokio::test]
    async fn the_trim_drops_stale_diagnostics_and_nothing_else() {
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let journal = EventJournal::new(storage.database().clone());
        let store = DiagnosticStore::new(storage.database().clone(), journal.clone());

        // A domain event of another kind, from long before the cutoff. If the
        // trim can reach this, it can reach a session, a run, or a seal.
        journal
            .append(EventAppend {
                event_id: None,
                session_id: None,
                run_id: None,
                source: crate::storage::EventSource::System,
                kind: "runtime.ready".into(),
                payload: json!({"ancient": true}),
                remote_sequence: None,
                command_id: None,
                created_at: Some("2020-01-01T00:00:00+00:00".into()),
            })
            .await
            .unwrap();

        let mut stale = DiagnosticInput::new(
            Severity::Error,
            "renderer",
            "test.event",
            "stale_code",
            "old",
        );
        stale.timestamp = Some("2020-01-01T00:00:00Z".into());
        store
            .append_batch(vec![
                validate(stale).unwrap(),
                sample("renderer", "fresh_code", Severity::Error),
            ])
            .await
            .unwrap();

        let removed = store
            .trim(std::time::Duration::from_secs(86_400), 10_000)
            .await
            .unwrap();
        assert_eq!(removed, 1);

        let remaining = store
            .search(DiagnosticQuery {
                since: std::time::Duration::from_secs(7 * 86_400),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].event.code, "fresh_code");

        // The other kind survived.
        let survivors = journal.events_after(0, 100).await.unwrap();
        assert!(
            survivors.iter().any(|event| event.kind == "runtime.ready"),
            "the trim deleted an event kind it does not own"
        );
    }

    #[tokio::test]
    async fn the_row_ceiling_bounds_a_burst_the_age_window_cannot() {
        let dir = tempdir().unwrap();
        let store = store(dir.path());
        store
            .append_batch(
                (0..20)
                    .map(|_| sample("renderer", "burst_code", Severity::Info))
                    .collect(),
            )
            .await
            .unwrap();
        // Everything is recent, so only the ceiling can act.
        let removed = store
            .trim(std::time::Duration::from_secs(86_400), 5)
            .await
            .unwrap();
        assert_eq!(removed, 15);
        assert_eq!(store.summary().await.unwrap().0, 5);
    }

    #[test]
    fn compiled_sql_binds_every_filter_value() {
        let query = DiagnosticQuery {
            components: vec!["renderer".into()],
            severities: vec![Severity::Error],
            codes: vec!["load_failed".into()],
            correlation: crate::diagnostics::event::Correlation {
                visual_id: Some("vis_1".into()),
                ..Default::default()
            },
            cursor: Some(10),
            ..Default::default()
        };
        let (sql, params) = compile_sql(&query, chrono::Utc::now());
        assert!(!sql.contains("vis_1"), "{sql}");
        assert!(!sql.contains("load_failed"), "{sql}");
        assert!(sql.contains("ORDER BY sequence DESC"));
        assert!(params.len() >= 6);
    }

    #[test]
    fn grouping_collapses_repeats_and_keeps_the_first_occurrence() {
        let records: Vec<DiagnosticRecord> = (0..3)
            .map(|index| DiagnosticRecord {
                sequence: index + 1,
                event: {
                    let mut input = DiagnosticInput::new(
                        Severity::Error,
                        "containers",
                        "container.rollout.failed",
                        "rollout_failed",
                        "boom",
                    );
                    input.timestamp = Some(format!("2026-08-16T00:00:0{index}Z"));
                    validate(input).unwrap()
                },
            })
            .collect();
        let grouped = group_by_code(&records);
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped[0]["count"], json!(3));
        assert_eq!(grouped[0]["first_seen"], json!("2026-08-16T00:00:00+00:00"));
    }
}
