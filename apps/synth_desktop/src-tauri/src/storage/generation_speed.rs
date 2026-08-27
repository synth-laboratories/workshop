//! Durable store for observed generation-speed measurements.
//!
//! One row per output-text segment, carrying both the derived rate and the raw
//! samples it was regressed from. Keeping the evidence next to the conclusion is
//! the point: a value shown in the transcript can be recomputed offline from its
//! own row, and a value that cannot be recomputed is a bug rather than a
//! difference of opinion.
//!
//! Writes are idempotent on `measurement_id`. Late provider response usage may
//! enrich the same segment after its lifecycle event, so conflicts update the
//! evidence row rather than minting a second measurement.

use super::Database;
use anyhow::Result;
use rusqlite::{params, Connection};
use serde::Serialize;
use std::sync::Arc;

/// One measurement as the store holds it. Field-for-field the wire contract the
/// renderer consumes, so there is no second shape to drift against it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GenerationSpeedRow {
    pub measurement_id: String,
    pub schema_version: String,
    pub measurement_kind: String,
    pub session_id: String,
    pub turn_id: String,
    pub response_id: Option<String>,
    pub item_id: String,
    pub output_index: i64,
    pub content_index: i64,
    pub phase: String,
    pub status: String,
    pub tps: Option<f64>,
    pub exact_tokens_after_first_sample: i64,
    pub duration_ms: f64,
    pub sample_count: i64,
    pub token_count_source: String,
    pub tokenizer_id: Option<String>,
    pub clock_source: String,
    pub unavailable_reason: Option<String>,
    /// JSON array of quality flags, as stored.
    pub quality_flags_json: String,
    /// JSON array of `{atUs, cumulativeTokens, sequenceNumber}` samples.
    pub samples_json: String,
    pub provider: Option<String>,
    pub model_id: Option<String>,
    pub created_at: String,
}

#[derive(Clone)]
pub struct GenerationSpeedRepository {
    db: Arc<Database>,
}

impl GenerationSpeedRepository {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub async fn record(&self, row: GenerationSpeedRow) -> Result<()> {
        self.db.run(move |conn| insert(conn, row)).await
    }

    /// Every measurement of one turn, oldest segment first.
    pub async fn for_turn(
        &self,
        session_id: String,
        turn_id: String,
    ) -> Result<Vec<GenerationSpeedRow>> {
        self.db
            .run(move |conn| select_turn(conn, &session_id, &turn_id))
            .await
    }
}

fn insert(conn: &Connection, row: GenerationSpeedRow) -> Result<()> {
    conn.execute(
        "INSERT INTO generation_speed_measurements (
            measurement_id,schema_version,measurement_kind,session_id,turn_id,response_id,item_id,
            output_index,content_index,phase,status,tps,exact_tokens_after_first_sample,duration_ms,
            sample_count,token_count_source,tokenizer_id,clock_source,unavailable_reason,
            quality_flags,samples_json,provider,model_id,created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24)
         ON CONFLICT(measurement_id) DO UPDATE SET
            status=excluded.status,
            tps=excluded.tps,
            exact_tokens_after_first_sample=excluded.exact_tokens_after_first_sample,
            duration_ms=excluded.duration_ms,
            sample_count=excluded.sample_count,
            token_count_source=excluded.token_count_source,
            tokenizer_id=excluded.tokenizer_id,
            clock_source=excluded.clock_source,
            unavailable_reason=excluded.unavailable_reason,
            quality_flags=excluded.quality_flags,
            samples_json=excluded.samples_json,
            provider=excluded.provider,
            model_id=excluded.model_id,
            created_at=excluded.created_at",
        params![
            row.measurement_id,
            row.schema_version,
            row.measurement_kind,
            row.session_id,
            row.turn_id,
            row.response_id,
            row.item_id,
            row.output_index,
            row.content_index,
            row.phase,
            row.status,
            row.tps,
            row.exact_tokens_after_first_sample,
            row.duration_ms,
            row.sample_count,
            row.token_count_source,
            row.tokenizer_id,
            row.clock_source,
            row.unavailable_reason,
            row.quality_flags_json,
            row.samples_json,
            row.provider,
            row.model_id,
            row.created_at,
        ],
    )?;
    Ok(())
}

fn select_turn(
    conn: &Connection,
    session_id: &str,
    turn_id: &str,
) -> Result<Vec<GenerationSpeedRow>> {
    let mut statement = conn.prepare(
        "SELECT measurement_id,schema_version,measurement_kind,session_id,turn_id,response_id,item_id,
                output_index,content_index,phase,status,tps,exact_tokens_after_first_sample,duration_ms,
                sample_count,token_count_source,tokenizer_id,clock_source,unavailable_reason,
                quality_flags,samples_json,provider,model_id,created_at
           FROM generation_speed_measurements
          WHERE session_id = ?1 AND turn_id = ?2
          ORDER BY created_at ASC, measurement_id ASC",
    )?;
    let rows = statement.query_map(params![session_id, turn_id], |row| {
        Ok(GenerationSpeedRow {
            measurement_id: row.get(0)?,
            schema_version: row.get(1)?,
            measurement_kind: row.get(2)?,
            session_id: row.get(3)?,
            turn_id: row.get(4)?,
            response_id: row.get(5)?,
            item_id: row.get(6)?,
            output_index: row.get(7)?,
            content_index: row.get(8)?,
            phase: row.get(9)?,
            status: row.get(10)?,
            tps: row.get(11)?,
            exact_tokens_after_first_sample: row.get(12)?,
            duration_ms: row.get(13)?,
            sample_count: row.get(14)?,
            token_count_source: row.get(15)?,
            tokenizer_id: row.get(16)?,
            clock_source: row.get(17)?,
            unavailable_reason: row.get(18)?,
            quality_flags_json: row.get(19)?,
            samples_json: row.get(20)?,
            provider: row.get(21)?,
            model_id: row.get(22)?,
            created_at: row.get(23)?,
        })
    })?;
    let mut measurements = Vec::new();
    for row in rows {
        measurements.push(row?);
    }
    Ok(measurements)
}
