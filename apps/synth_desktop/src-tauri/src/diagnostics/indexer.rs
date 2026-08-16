//! Journal → VictoriaLogs indexing by durable sequence.
//!
//! The indexer never sees a producer. It reads committed journal rows in
//! sequence order, ships them, and advances a cursor only after the index
//! accepts them — at-least-once delivery with no back-pressure on anything
//! that emits. A crash, a restart, or a wiped index resumes from the cursor;
//! the journal sequence travels with each row as its idempotency key, so a
//! replayed batch cannot produce a duplicate *logical* result even though the
//! index may briefly hold duplicate *rows*.

use super::store::DiagnosticStore;
use super::victorialogs::VictoriaLogsClient;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const CURSOR_SCHEMA: &str = "synth.diagnostics-index-cursor.v1";

/// Rows per indexing pass.
pub const INDEX_BATCH: i64 = 500;

pub fn cursor_path(root: &Path) -> PathBuf {
    root.join("indexer-cursor.json")
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexCursor {
    pub schema: String,
    pub sequence: i64,
    pub updated_at: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IndexProgress {
    pub indexed: usize,
    pub cursor: i64,
    /// Diagnostics committed to the journal but not yet indexed.
    pub lag: i64,
}

#[derive(Clone)]
pub struct Indexer {
    store: DiagnosticStore,
    root: PathBuf,
}

impl Indexer {
    pub fn new(store: DiagnosticStore, root: impl Into<PathBuf>) -> Self {
        Self {
            store,
            root: root.into(),
        }
    }

    /// A missing or corrupt cursor means "index everything again".
    ///
    /// Re-indexing is cheap and safe; refusing to index because a small JSON
    /// file was truncated by a power loss would leave the agent with no search
    /// path for the rest of the installation's life.
    pub fn load_cursor(&self) -> i64 {
        std::fs::read_to_string(cursor_path(&self.root))
            .ok()
            .and_then(|raw| serde_json::from_str::<IndexCursor>(&raw).ok())
            .filter(|cursor| cursor.schema == CURSOR_SCHEMA && cursor.sequence >= 0)
            .map(|cursor| cursor.sequence)
            .unwrap_or(0)
    }

    pub fn save_cursor(&self, sequence: i64) {
        let cursor = IndexCursor {
            schema: CURSOR_SCHEMA.into(),
            sequence,
            updated_at: chrono::Utc::now().to_rfc3339(),
        };
        let path = cursor_path(&self.root);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let Ok(body) = serde_json::to_vec(&cursor) else {
            return;
        };
        // Write-then-rename so a crash mid-write cannot leave a half cursor
        // that reads as a *valid* smaller sequence and silently re-indexes.
        let temporary = path.with_extension("json.writing");
        if std::fs::write(&temporary, body).is_ok() {
            let _ = std::fs::rename(temporary, path);
        }
    }

    /// Index one batch. Returns what moved so the caller can decide whether to
    /// loop immediately or wait for the next tick.
    pub async fn index_once(&self, client: &VictoriaLogsClient) -> Result<IndexProgress> {
        let cursor = self.load_cursor();
        let records = self.store.records_after(cursor, INDEX_BATCH).await?;
        if records.is_empty() {
            return Ok(IndexProgress {
                indexed: 0,
                cursor,
                lag: 0,
            });
        }
        let lines: Vec<serde_json::Value> = records
            .iter()
            .map(|record| record.event.to_index_line(record.sequence))
            .collect();
        client.ingest(&lines).await?;
        let advanced = records.last().map(|record| record.sequence).unwrap_or(cursor);
        self.save_cursor(advanced);
        let head = self.store.head_sequence().await.unwrap_or(advanced);
        Ok(IndexProgress {
            indexed: records.len(),
            cursor: advanced,
            lag: (head - advanced).max(0),
        })
    }

    /// Journal rows waiting to be indexed.
    pub async fn lag(&self) -> i64 {
        let head = self.store.head_sequence().await.unwrap_or(0);
        (head - self.load_cursor()).max(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::event::{validate, DiagnosticInput, Severity};
    use crate::storage::{EventJournal, Storage};
    use tempfile::tempdir;

    fn store(root: &Path) -> DiagnosticStore {
        let storage = Storage::open(root).unwrap();
        let journal = EventJournal::new(storage.database().clone());
        DiagnosticStore::new(storage.database().clone(), journal)
    }

    #[test]
    fn a_missing_cursor_indexes_from_the_beginning() {
        let dir = tempdir().unwrap();
        let indexer = Indexer::new(store(&dir.path().join("db")), dir.path());
        assert_eq!(indexer.load_cursor(), 0);
    }

    #[test]
    fn a_corrupt_cursor_rebuilds_instead_of_failing() {
        let dir = tempdir().unwrap();
        let indexer = Indexer::new(store(&dir.path().join("db")), dir.path());
        std::fs::write(cursor_path(dir.path()), b"{ this is not json").unwrap();
        assert_eq!(indexer.load_cursor(), 0);
        std::fs::write(
            cursor_path(dir.path()),
            br#"{"schema":"some.other.schema","sequence":900,"updatedAt":"now"}"#,
        )
        .unwrap();
        assert_eq!(indexer.load_cursor(), 0);
    }

    #[test]
    fn the_cursor_round_trips() {
        let dir = tempdir().unwrap();
        let indexer = Indexer::new(store(&dir.path().join("db")), dir.path());
        indexer.save_cursor(4_242);
        assert_eq!(indexer.load_cursor(), 4_242);
        assert!(!cursor_path(dir.path())
            .with_extension("json.writing")
            .exists());
    }

    #[tokio::test]
    async fn lag_reports_journal_rows_the_index_has_not_seen() {
        let dir = tempdir().unwrap();
        let store = store(&dir.path().join("db"));
        let indexer = Indexer::new(store.clone(), dir.path());
        store
            .append_batch(
                (0..3)
                    .map(|index| {
                        validate(DiagnosticInput::new(
                            Severity::Info,
                            "renderer",
                            "test.event",
                            "test_code",
                            format!("event {index}"),
                        ))
                        .unwrap()
                    })
                    .collect(),
            )
            .await
            .unwrap();
        assert_eq!(indexer.lag().await, 3);
        indexer.save_cursor(2);
        assert_eq!(indexer.lag().await, 1);
    }

    #[tokio::test]
    async fn index_lines_carry_the_journal_sequence_for_every_record() {
        let dir = tempdir().unwrap();
        let store = store(&dir.path().join("db"));
        let written = store
            .append_batch(vec![validate(DiagnosticInput::new(
                Severity::Error,
                "visual-host",
                "visual.projection.rejected",
                "unsupported_trace_projection_schema",
                "Unsupported trace projection schema: synth.trace.v5",
            ))
            .unwrap()])
            .await
            .unwrap();
        let line = written[0].event.to_index_line(written[0].sequence);
        assert_eq!(
            line["journal_sequence"],
            serde_json::json!(written[0].sequence.to_string())
        );
    }

    #[tokio::test]
    async fn a_failed_ingest_leaves_the_cursor_where_it_was() {
        let dir = tempdir().unwrap();
        let store = store(&dir.path().join("db"));
        let indexer = Indexer::new(store.clone(), dir.path());
        store
            .append_batch(vec![validate(DiagnosticInput::new(
                Severity::Warn,
                "renderer",
                "test.event",
                "test_code",
                "one",
            ))
            .unwrap()])
            .await
            .unwrap();
        // Nothing is listening on this port, so ingestion fails.
        let client = VictoriaLogsClient::new("http://127.0.0.1:1").unwrap();
        assert!(indexer.index_once(&client).await.is_err());
        assert_eq!(indexer.load_cursor(), 0, "cursor advanced past unindexed rows");
    }

    #[tokio::test]
    async fn an_empty_journal_is_a_no_op_pass() {
        let dir = tempdir().unwrap();
        let indexer = Indexer::new(store(&dir.path().join("db")), dir.path());
        let client = VictoriaLogsClient::new("http://127.0.0.1:1").unwrap();
        let progress = indexer.index_once(&client).await.unwrap();
        assert_eq!(progress, IndexProgress::default());
    }
}
