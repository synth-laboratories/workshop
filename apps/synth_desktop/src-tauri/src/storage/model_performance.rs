//! Throughput summaries over the authoritative usage ledger.
//!
//! Since migration 8 there is no separate performance table: every request
//! lives in `usage_records` (see `storage::usage_records`), and this module
//! only aggregates the throughput view of it for the model pickers.

use super::usage_records::percentile;
use super::Database;
use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

#[derive(
    Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, specta::Type,
)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementKind {
    Decode,
    /// A rate regressed from one output-text segment's own samples.
    ObservedStreamSegment,
    /// Turn-wide tokens over a gap-filtered denominator, recorded before
    /// segment measurement existed. Kept readable, never treated as a
    /// measurement: see migration 21.
    LegacyObservedStreamEstimate,
    EndToEnd,
    ProviderReported,
}

impl MeasurementKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Decode => "decode",
            Self::ObservedStreamSegment => "observed_stream_segment",
            Self::LegacyObservedStreamEstimate => "legacy_observed_stream_estimate",
            Self::EndToEnd => "end_to_end",
            Self::ProviderReported => "provider_reported",
        }
    }
    fn parse(value: &str) -> Self {
        match value {
            "decode" => Self::Decode,
            "observed_stream_segment" => Self::ObservedStreamSegment,
            "provider_reported" => Self::ProviderReported,
            // `observed_stream` is the pre-migration-20 name for the turn-wide
            // estimate. A row still carrying it has not been reinterpreted.
            "observed_stream" | "legacy_observed_stream_estimate" => {
                Self::LegacyObservedStreamEstimate
            }
            _ => Self::EndToEnd,
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelPerformanceSummary {
    pub provider: String,
    pub model_id: String,
    pub measurement_kind: MeasurementKind,
    #[specta(type = specta_typescript::Number)]
    pub sample_count: usize,
    pub tps_p50: Option<f64>,
    pub tps_p95: Option<f64>,
    pub ttft_p50_ms: Option<f64>,
    pub last_observed_at: String,
}

/// One authoritative request measurement for reconstructing per-user-turn
/// throughput in the transcript. The renderer groups these rows between user
/// message timestamps; it must never substitute the model's lifetime p50.
#[derive(Clone, Debug, Serialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ModelPerformanceTurnSample {
    pub run_id: Option<String>,
    pub measurement_kind: MeasurementKind,
    #[specta(type = specta_typescript::Number)]
    pub started_at_ms: i64,
    #[specta(type = specta_typescript::Number)]
    pub completed_at_ms: i64,
    pub output_tps: f64,
}

#[derive(Clone)]
pub struct ModelPerformanceRepository {
    db: Arc<Database>,
}

impl ModelPerformanceRepository {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }
    pub async fn summaries(&self) -> Result<Vec<ModelPerformanceSummary>> {
        self.db.run(summaries).await
    }

    pub async fn turn_samples(
        &self,
        session_id: String,
    ) -> Result<Vec<ModelPerformanceTurnSample>> {
        self.db
            .run(move |connection| turn_samples(connection, &session_id))
            .await
    }
}

fn finite_positive(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite() && *value > 0.0)
}

#[derive(Default)]
struct Aggregate {
    tps: Vec<f64>,
    ttft: Vec<f64>,
    last_ms: i64,
}

fn summaries(conn: &Connection) -> Result<Vec<ModelPerformanceSummary>> {
    let mut statement = conn.prepare("SELECT provider,model_id,measurement_kind,completed_at_ms,ttft_ms,observed_output_tps FROM usage_records WHERE status='completed' AND output_tokens IS NOT NULL AND output_tokens>0 AND observed_output_tps IS NOT NULL AND observed_output_tps>0 ORDER BY completed_at_ms")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, Option<f64>>(4)?,
            row.get::<_, Option<f64>>(5)?,
        ))
    })?;
    let mut groups: BTreeMap<(String, String, MeasurementKind), Aggregate> = BTreeMap::new();
    for row in rows {
        let (provider, model, kind, completed, ttft, observed) = row?;
        let a = groups
            .entry((provider, model, MeasurementKind::parse(&kind)))
            .or_default();
        a.last_ms = a.last_ms.max(completed);
        if let Some(v) = finite_positive(ttft) {
            a.ttft.push(v)
        }
        if let Some(v) = finite_positive(observed) {
            a.tps.push(v)
        }
    }
    Ok(groups
        .into_iter()
        .filter_map(|((provider, model_id, measurement_kind), mut a)| {
            if a.tps.is_empty() {
                return None;
            }
            let mut p95 = a.tps.clone();
            let sample_count = a.tps.len();
            Some(ModelPerformanceSummary {
                provider,
                model_id,
                measurement_kind,
                sample_count,
                tps_p50: percentile(&mut a.tps, 0.5),
                tps_p95: percentile(&mut p95, 0.95),
                ttft_p50_ms: percentile(&mut a.ttft, 0.5),
                last_observed_at: chrono::DateTime::<Utc>::from_timestamp_millis(a.last_ms)
                    .unwrap_or_default()
                    .to_rfc3339(),
            })
        })
        .collect())
}

fn turn_samples(conn: &Connection, session_id: &str) -> Result<Vec<ModelPerformanceTurnSample>> {
    let mut statement = conn.prepare(
        "SELECT run_id,measurement_kind,started_at_ms,completed_at_ms,
                observed_output_tps
         FROM usage_records
         WHERE session_id=?1 AND output_tokens IS NOT NULL AND output_tokens>0
           AND observed_output_tps IS NOT NULL
           AND observed_output_tps>0
         ORDER BY started_at_ms,completed_at_ms",
    )?;
    let rows = statement.query_map([session_id], |row| {
        let kind = MeasurementKind::parse(&row.get::<_, String>(1)?);
        Ok((
            row.get::<_, Option<String>>(0)?,
            kind,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, Option<f64>>(4)?,
        ))
    })?;
    let mut samples = Vec::new();
    for row in rows {
        let (run_id, measurement_kind, started_at_ms, completed_at_ms, output_tps) = row?;
        let Some(output_tps) = finite_positive(output_tps) else {
            continue;
        };
        samples.push(ModelPerformanceTurnSample {
            run_id,
            measurement_kind,
            started_at_ms,
            completed_at_ms,
            output_tps,
        });
    }
    Ok(samples)
}

