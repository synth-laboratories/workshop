//! Durable delivery receipts for optimizer projections.
//!
//! Projection rows are the source of truth. The outbox is only the durable
//! wake-up contract that makes every renderer surface fetch that truth after a
//! missed broadcast or process restart.

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::optimizers::models::OptimizerResourceRef;

pub const RUN_EVENT_CONSUMER: &str = "run_event";
pub const VISUAL_CONSUMER_PREFIX: &str = "visual:";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionDelivery {
    pub run_id: String,
    pub projection_revision: u64,
    pub consumer: String,
    pub delivery_state: String,
    pub attempts: u32,
    pub last_error: Option<String>,
}

/// Enqueue the shared run subscriber and every visual explicitly bound to the
/// run. This is called from `upsert_projection`, inside its caller's SQLite
/// transaction; projection truth and the obligation to announce it therefore
/// cannot commit independently.
pub fn enqueue(conn: &Connection, run_id: &str, projection_revision: u64) -> Result<()> {
    let mut consumers = vec![RUN_EVENT_CONSUMER.to_string()];
    let visual_refs: Option<String> = conn
        .query_row(
            "SELECT visual_refs_json FROM optimizer_runs WHERE id=?1",
            [run_id],
            |row| row.get(0),
        )
        .ok();
    if let Some(raw) = visual_refs {
        let refs: Vec<OptimizerResourceRef> = serde_json::from_str(&raw)
            .with_context(|| format!("decode visual refs while enqueuing projection {run_id}"))?;
        consumers.extend(
            refs.into_iter()
                .filter(|reference| reference.kind == "visual")
                .map(|reference| format!("{VISUAL_CONSUMER_PREFIX}{}", reference.id)),
        );
    }
    consumers.sort();
    consumers.dedup();
    let revision = i64::try_from(projection_revision).context("projection revision overflow")?;
    for consumer in consumers {
        conn.execute(
            "INSERT INTO optimizer_projection_outbox(
                 run_id, projection_revision, consumer, delivery_state,
                 attempts, last_error, updated_at
             ) VALUES (?1,?2,?3,'pending',0,NULL,datetime('now'))
             ON CONFLICT(run_id, projection_revision, consumer) DO NOTHING",
            params![run_id, revision, consumer],
        )?;
    }
    Ok(())
}

/// One delivery cycle is one latest durable revision per run. Superseded rows
/// travel with it and are acknowledged together, so a long-disconnected UI
/// never has to replay a stale wake-up storm before reaching current truth.
pub fn pending_latest(
    conn: &Connection,
    only_run_id: Option<&str>,
) -> Result<Vec<ProjectionDelivery>> {
    let mut stmt = conn.prepare(
        "SELECT o.run_id, o.projection_revision, o.consumer,
                o.delivery_state, o.attempts, o.last_error
         FROM optimizer_projection_outbox o
         JOIN (
             SELECT run_id, MAX(projection_revision) AS revision
             FROM optimizer_projection_outbox
             WHERE delivery_state != 'delivered'
               AND (?1 IS NULL OR run_id = ?1)
             GROUP BY run_id
         ) latest
           ON latest.run_id=o.run_id AND latest.revision=o.projection_revision
         WHERE o.delivery_state != 'delivered'
         ORDER BY o.run_id, o.consumer",
    )?;
    let rows = stmt.query_map([only_run_id], |row| {
        let revision: i64 = row.get(1)?;
        Ok(ProjectionDelivery {
            run_id: row.get(0)?,
            projection_revision: revision.max(0) as u64,
            consumer: row.get(2)?,
            delivery_state: row.get(3)?,
            attempts: row.get::<_, i64>(4)?.max(0) as u32,
            last_error: row.get(5)?,
        })
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
}

pub fn mark_delivered(conn: &Connection, run_id: &str, through_revision: u64) -> Result<usize> {
    let revision = i64::try_from(through_revision).context("projection revision overflow")?;
    conn.execute(
        "UPDATE optimizer_projection_outbox
         SET delivery_state='delivered', attempts=attempts+1,
             last_error=NULL, updated_at=datetime('now')
         WHERE run_id=?1 AND projection_revision<=?2 AND delivery_state!='delivered'",
        params![run_id, revision],
    )
    .map_err(Into::into)
}

pub fn mark_failed(
    conn: &Connection,
    run_id: &str,
    projection_revision: u64,
    error: &str,
) -> Result<usize> {
    let revision = i64::try_from(projection_revision).context("projection revision overflow")?;
    conn.execute(
        "UPDATE optimizer_projection_outbox
         SET delivery_state=CASE
                 WHEN consumer LIKE 'visual:%' THEN 'visual_projection_delivery_failed'
                 ELSE 'event_delivery_failed'
             END,
             attempts=attempts+1, last_error=?3, updated_at=datetime('now')
         WHERE run_id=?1 AND projection_revision<=?2 AND delivery_state!='delivered'",
        params![run_id, revision, error],
    )
    .map_err(Into::into)
}

pub fn mark_visual_failed(conn: &Connection, run_id: &str, error: &str) -> Result<usize> {
    conn.execute(
        "UPDATE optimizer_projection_outbox
         SET delivery_state='visual_projection_delivery_failed',
             attempts=attempts+1, last_error=?2, updated_at=datetime('now')
         WHERE run_id=?1
           AND projection_revision=(
               SELECT MAX(projection_revision) FROM optimizer_projection_outbox WHERE run_id=?1
           )
           AND consumer LIKE 'visual:%'",
        params![run_id, error],
    )
    .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::migrations::apply_migrations;
    use serde_json::json;

    fn run_row(conn: &Connection) {
        apply_migrations(conn).unwrap();
        conn.execute(
            "INSERT INTO optimizer_runs(
                id, algorithm_id, status, source, created_at, cursor_seq,
                capabilities_json, bindings_json, input_refs_json,
                output_refs_json, visual_refs_json, summary_json, usage_json,
                payload_json, updated_at
             ) VALUES ('run-1','eval','running','local','now',0,
                '{}','{}','[]','[]',?1,'{}','{}','{}','now')",
            [json!([
                {"kind":"visual","id":"vis-a"},
                {"kind":"visual","id":"vis-b"}
            ])
            .to_string()],
        )
        .unwrap();
    }

    #[test]
    fn failure_then_retry_converges_every_bound_surface_in_one_cycle() {
        let conn = Connection::open_in_memory().unwrap();
        run_row(&conn);
        enqueue(&conn, "run-1", 7).unwrap();
        let pending = pending_latest(&conn, Some("run-1")).unwrap();
        assert_eq!(pending.len(), 3);
        assert_eq!(
            pending
                .iter()
                .map(|row| row.consumer.as_str())
                .collect::<Vec<_>>(),
            vec!["run_event", "visual:vis-a", "visual:vis-b"]
        );

        mark_failed(&conn, "run-1", 7, "no renderer subscribers").unwrap();
        let failed = pending_latest(&conn, Some("run-1")).unwrap();
        assert_eq!(failed[0].delivery_state, "event_delivery_failed");
        assert!(failed[1..].iter().all(|row| {
            row.delivery_state == "visual_projection_delivery_failed" && row.attempts == 1
        }));

        assert_eq!(mark_delivered(&conn, "run-1", 7).unwrap(), 3);
        assert!(pending_latest(&conn, Some("run-1")).unwrap().is_empty());
    }

    #[test]
    fn one_latest_delivery_acknowledges_superseded_revisions() {
        let conn = Connection::open_in_memory().unwrap();
        run_row(&conn);
        enqueue(&conn, "run-1", 7).unwrap();
        enqueue(&conn, "run-1", 8).unwrap();
        let pending = pending_latest(&conn, None).unwrap();
        assert_eq!(pending.len(), 3);
        assert!(pending.iter().all(|row| row.projection_revision == 8));
        assert_eq!(mark_delivered(&conn, "run-1", 8).unwrap(), 6);
    }
}
