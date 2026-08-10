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

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementKind {
    Decode,
    ObservedStream,
    EndToEnd,
    ProviderReported,
}

impl MeasurementKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Decode => "decode",
            Self::ObservedStream => "observed_stream",
            Self::EndToEnd => "end_to_end",
            Self::ProviderReported => "provider_reported",
        }
    }
    fn parse(value: &str) -> Self {
        match value {
            "decode" => Self::Decode,
            "end_to_end" => Self::EndToEnd,
            "provider_reported" => Self::ProviderReported,
            _ => Self::ObservedStream,
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelPerformanceSummary {
    pub provider: String,
    pub model_id: String,
    pub measurement_kind: MeasurementKind,
    pub sample_count: usize,
    pub tps_p50: Option<f64>,
    pub tps_p95: Option<f64>,
    pub ttft_p50_ms: Option<f64>,
    pub last_observed_at: String,
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
    let mut statement = conn.prepare("SELECT provider,model_id,measurement_kind,completed_at_ms,ttft_ms,observed_output_tps,end_to_end_output_tps FROM usage_records WHERE status='completed' AND output_tokens IS NOT NULL AND output_tokens>0 ORDER BY completed_at_ms")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, Option<f64>>(4)?,
            row.get::<_, Option<f64>>(5)?,
            row.get::<_, Option<f64>>(6)?,
        ))
    })?;
    let mut groups: BTreeMap<(String, String, MeasurementKind), Aggregate> = BTreeMap::new();
    for row in rows {
        let (provider, model, kind, completed, ttft, observed, e2e) = row?;
        let a = groups
            .entry((provider, model, MeasurementKind::parse(&kind)))
            .or_default();
        a.last_ms = a.last_ms.max(completed);
        if let Some(v) = finite_positive(ttft) {
            a.ttft.push(v)
        }
        if let Some(v) = finite_positive(observed.or(e2e)) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::usage_records::{CostSource, UsageRecord, UsageRecordsRepository};
    use crate::storage::Storage;

    fn record(request: &str, tps: f64) -> UsageRecord {
        UsageRecord {
            id: format!("perf-{request}"),
            provider: "openrouter".into(),
            model_id: "model-a".into(),
            model_revision: None,
            session_id: None,
            run_id: None,
            request_id: request.into(),
            measurement_kind: MeasurementKind::ObservedStream,
            status: "completed".into(),
            started_at_ms: 1000,
            first_output_at_ms: Some(1100),
            last_output_at_ms: Some(2100),
            completed_at_ms: 2200,
            input_tokens: Some(10),
            cached_input_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
            output_tokens: Some(20),
            ttft_ms: Some(100.),
            observed_output_tps: Some(tps),
            end_to_end_output_tps: Some(16.7),
            billed_cost_usd: None,
            estimated_cost_usd: None,
            cost_source: CostSource::None,
            source: "codex_app_server".into(),
        }
    }

    #[tokio::test]
    async fn records_idempotently_and_returns_model_percentiles() {
        let t = tempfile::tempdir().unwrap();
        let s = Storage::open(t.path()).unwrap();
        let w = UsageRecordsRepository::new(s.database().clone());
        for (id, tps) in [("one", 10.), ("two", 20.), ("three", 30.), ("three", 30.)] {
            w.record(record(id, tps)).await.unwrap()
        }
        let v = ModelPerformanceRepository::new(s.database().clone())
            .summaries()
            .await
            .unwrap();
        assert_eq!(v[0].sample_count, 3);
        assert_eq!(v[0].tps_p50, Some(20.));
    }

    #[tokio::test]
    async fn omits_failed_and_tokenless_rows() {
        let t = tempfile::tempdir().unwrap();
        let s = Storage::open(t.path()).unwrap();
        let w = UsageRecordsRepository::new(s.database().clone());
        let mut x = record("failed", 40.);
        x.status = "failed".into();
        w.record(x).await.unwrap();
        let mut x = record("tokenless", 40.);
        x.output_tokens = None;
        x.observed_output_tps = None;
        w.record(x).await.unwrap();
        let v = ModelPerformanceRepository::new(s.database().clone())
            .summaries()
            .await
            .unwrap();
        assert!(v.is_empty());
    }
}
