//! The authoritative per-request usage ledger.
//!
//! One row per provider request, keyed `(provider, request_id)`, carrying
//! tokens, cache traffic, timing, throughput, and cost. Start, stream,
//! completion, usage, and billing metadata can each arrive separately, so
//! every write is an idempotent upsert that only ever *fills in* facts —
//! `COALESCE` keeps an earlier reported value over a later `NULL`, and a
//! provider-reported settled charge always replaces a tariff estimate without
//! creating a second row.
//!
//! Both the Usage dashboard and the model performance summaries read from
//! this table; there is deliberately no second aggregate to drift against it.

use super::model_performance::MeasurementKind;
use super::Database;
use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

/// Who vouches for a request's dollar figure. Ordered by authority: a settled
/// provider charge beats a Synth Cloud figure beats a tariff estimate.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum CostSource {
    ProviderReported,
    SynthCloud,
    TariffEstimate,
    None,
}

impl CostSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProviderReported => "provider_reported",
            Self::SynthCloud => "synth_cloud",
            Self::TariffEstimate => "tariff_estimate",
            Self::None => "none",
        }
    }
    fn parse(value: &str) -> Self {
        match value {
            "provider_reported" => Self::ProviderReported,
            "synth_cloud" => Self::SynthCloud,
            "tariff_estimate" => Self::TariffEstimate,
            _ => Self::None,
        }
    }
}

/// SQL fragment mapping a `cost_source` column to its authority rank, so the
/// upsert can keep the stronger of the stored and incoming sources.
const COST_RANK: &str = "CASE {} WHEN 'provider_reported' THEN 3 WHEN 'synth_cloud' THEN 2 WHEN 'tariff_estimate' THEN 1 ELSE 0 END";

fn cost_rank(column: &str) -> String {
    COST_RANK.replace("{}", column)
}

#[derive(Clone, Debug)]
pub struct UsageRecord {
    pub id: String,
    pub provider: String,
    pub model_id: String,
    pub model_revision: Option<String>,
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    pub request_id: String,
    pub measurement_kind: MeasurementKind,
    pub status: String,
    pub started_at_ms: i64,
    pub first_output_at_ms: Option<i64>,
    pub last_output_at_ms: Option<i64>,
    pub completed_at_ms: i64,
    pub input_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub cache_write_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub ttft_ms: Option<f64>,
    pub observed_output_tps: Option<f64>,
    pub end_to_end_output_tps: Option<f64>,
    pub billed_cost_usd: Option<f64>,
    pub estimated_cost_usd: Option<f64>,
    pub cost_source: CostSource,
    pub source: String,
}

/// One aggregated slice — the device total or one (provider, model) pair.
/// `input/output/total` are plain sums (a request that reported nothing
/// contributes nothing); cache, reasoning, cost, and throughput fields stay
/// `None` until at least one request actually reported them, so the UI can
/// say "Unavailable" instead of a fabricated zero.
#[derive(Clone, Debug, Serialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UsageBreakdown {
    pub provider: String,
    pub model_id: String,
    #[specta(type = specta_typescript::Unknown)]
    pub requests: i64,
    #[specta(type = specta_typescript::Unknown)]
    pub input_tokens: i64,
    #[specta(type = specta_typescript::Unknown)]
    pub cached_input_tokens: Option<i64>,
    #[specta(type = specta_typescript::Unknown)]
    pub non_cached_input_tokens: Option<i64>,
    #[specta(type = specta_typescript::Unknown)]
    pub cache_write_tokens: Option<i64>,
    #[specta(type = specta_typescript::Unknown)]
    pub reasoning_tokens: Option<i64>,
    #[specta(type = specta_typescript::Unknown)]
    pub output_tokens: i64,
    #[specta(type = specta_typescript::Unknown)]
    pub total_tokens: i64,
    pub cache_hit_rate: Option<f64>,
    pub billed_cost_usd: Option<f64>,
    pub estimated_cost_usd: Option<f64>,
    pub cost_source: CostSource,
    pub decode_tps_p50: Option<f64>,
    pub decode_tps_p95: Option<f64>,
    pub end_to_end_tps_p50: Option<f64>,
    pub end_to_end_tps_p95: Option<f64>,
    pub ttft_ms_p50: Option<f64>,
    pub ttft_ms_p95: Option<f64>,
    #[specta(type = specta_typescript::Unknown)]
    pub perf_sample_count: i64,
}

/// One local calendar day for one provider. Reduced from the same ledger rows
/// as `totals`, by the same `Bucket`, so a daily chart can never disagree with
/// the headline it sits under. The provider rides on `totals.provider`.
#[derive(Clone, Debug, Serialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UsageDayPoint {
    pub day: String,
    pub totals: UsageBreakdown,
}

#[derive(Clone, Debug, Serialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub window: String,
    pub totals: UsageBreakdown,
    pub models: Vec<UsageBreakdown>,
    /// Ascending by day, then provider. Days with no requests are absent
    /// rather than zero-filled — the caller owns the calendar it draws.
    pub days: Vec<UsageDayPoint>,
    pub generated_at: String,
}

/// The local calendar day a request completed on, as `YYYY-MM-DD`. The offset
/// is applied to the instant before the date is read, so an 11pm local request
/// never lands on tomorrow's bar. `None` for a timestamp chrono cannot express;
/// such a row still counts in the totals, it just has no day to sit on.
fn local_day(completed_at_ms: i64, local_offset_seconds: i32) -> Option<String> {
    let shifted = completed_at_ms.checked_add(i64::from(local_offset_seconds) * 1_000)?;
    DateTime::from_timestamp_millis(shifted).map(|stamp| stamp.format("%Y-%m-%d").to_string())
}

/// Start of the requested window in unix ms, or `None` for all time.
/// `today` starts at local midnight — the caller supplies its UTC offset in
/// seconds so the boundary is testable without touching the process timezone.
pub fn window_start_ms(window: &str, now: DateTime<Utc>, local_offset_seconds: i32) -> Option<i64> {
    const DAY_MS: i64 = 24 * 60 * 60 * 1_000;
    match window {
        "today" => {
            let local_ms = now.timestamp_millis() + i64::from(local_offset_seconds) * 1_000;
            let local_midnight = local_ms - local_ms.rem_euclid(DAY_MS);
            Some(local_midnight - i64::from(local_offset_seconds) * 1_000)
        }
        "7d" => Some(now.timestamp_millis() - 7 * DAY_MS),
        "30d" => Some(now.timestamp_millis() - 30 * DAY_MS),
        _ => None,
    }
}

#[derive(Clone)]
pub struct UsageRecordsRepository {
    db: Arc<Database>,
}

impl UsageRecordsRepository {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    pub async fn record(&self, record: UsageRecord) -> Result<()> {
        self.db.run(move |conn| upsert(conn, record)).await
    }

    /// Attach a settled charge to an existing request. Returns `false` when
    /// the request has no row yet — the caller retries after the record
    /// exists rather than this module inventing a stub with fake timing.
    ///
    /// The amount and its label move together, and only when the incoming
    /// authority is at least the stored one (or the row holds no amount yet):
    /// a weaker source must never overwrite a stronger settled figure, and a
    /// row must never carry one source's dollars under another source's name.
    pub async fn record_billed_cost(
        &self,
        provider: String,
        request_id: String,
        billed_cost_usd: f64,
        source: CostSource,
    ) -> Result<bool> {
        self.db
            .run(move |conn| {
                let authoritative = format!(
                    "({incoming} >= {stored} OR billed_cost_usd IS NULL)",
                    incoming = cost_rank("?4"),
                    stored = cost_rank("cost_source"),
                );
                let updated = conn.execute(
                    &format!(
                        "UPDATE usage_records SET
                            billed_cost_usd = CASE WHEN {authoritative} THEN ?3 ELSE billed_cost_usd END,
                            cost_source = CASE WHEN {authoritative} THEN ?4 ELSE cost_source END
                         WHERE provider = ?1 AND request_id = ?2",
                    ),
                    params![provider, request_id, billed_cost_usd, source.as_str()],
                )?;
                Ok(updated > 0)
            })
            .await
    }

    /// Aggregate every request completed at or after `since_ms` (all time when
    /// `None`) into a device total, per-(provider, model) slices, and a
    /// per-(local day, provider) series. `local_offset_seconds` decides which
    /// calendar day a request falls on; it is passed in rather than read from
    /// the process timezone so the boundary stays testable.
    pub async fn summary(
        &self,
        window: String,
        since_ms: Option<i64>,
        local_offset_seconds: i32,
    ) -> Result<UsageSummary> {
        self.db
            .run(move |conn| summarize(conn, &window, since_ms, local_offset_seconds))
            .await
    }
}

fn finite_positive(value: Option<f64>) -> Option<f64> {
    value.filter(|value| value.is_finite() && *value > 0.0)
}

fn existing_foreign_key(
    conn: &Connection,
    table: &str,
    value: Option<String>,
) -> Result<Option<String>> {
    let Some(value) = value else { return Ok(None) };
    let exists: bool = conn.query_row(
        &format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id = ?1)"),
        params![&value],
        |row| row.get(0),
    )?;
    Ok(exists.then_some(value))
}

fn upsert(conn: &Connection, mut record: UsageRecord) -> Result<()> {
    record.ttft_ms = finite_positive(record.ttft_ms);
    record.observed_output_tps = finite_positive(record.observed_output_tps);
    record.end_to_end_output_tps = finite_positive(record.end_to_end_output_tps);
    record.session_id = existing_foreign_key(conn, "sessions", record.session_id)?;
    record.run_id = existing_foreign_key(conn, "runs", record.run_id)?;
    // Normalize impossible counter combinations while preserving "unreported"
    // as NULL: a cached read count can never exceed the input it reads from.
    if let (Some(input), Some(cached)) = (record.input_tokens, record.cached_input_tokens) {
        record.cached_input_tokens = Some(cached.clamp(0, input.max(0)));
    }
    let sql = format!(
        "INSERT INTO usage_records (id,provider,model_id,model_revision,session_id,run_id,request_id,measurement_kind,status,started_at_ms,first_output_at_ms,last_output_at_ms,completed_at_ms,input_tokens,cached_input_tokens,cache_write_tokens,reasoning_tokens,output_tokens,ttft_ms,observed_output_tps,end_to_end_output_tps,billed_cost_usd,estimated_cost_usd,cost_source,source,created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26)
         ON CONFLICT(provider,request_id) DO UPDATE SET
            status=excluded.status,
            first_output_at_ms=COALESCE(excluded.first_output_at_ms,usage_records.first_output_at_ms),
            last_output_at_ms=COALESCE(excluded.last_output_at_ms,usage_records.last_output_at_ms),
            completed_at_ms=excluded.completed_at_ms,
            input_tokens=COALESCE(excluded.input_tokens,usage_records.input_tokens),
            cached_input_tokens=COALESCE(excluded.cached_input_tokens,usage_records.cached_input_tokens),
            cache_write_tokens=COALESCE(excluded.cache_write_tokens,usage_records.cache_write_tokens),
            reasoning_tokens=COALESCE(excluded.reasoning_tokens,usage_records.reasoning_tokens),
            output_tokens=COALESCE(excluded.output_tokens,usage_records.output_tokens),
            ttft_ms=COALESCE(excluded.ttft_ms,usage_records.ttft_ms),
            observed_output_tps=COALESCE(excluded.observed_output_tps,usage_records.observed_output_tps),
            end_to_end_output_tps=COALESCE(excluded.end_to_end_output_tps,usage_records.end_to_end_output_tps),
            billed_cost_usd=COALESCE(excluded.billed_cost_usd,usage_records.billed_cost_usd),
            estimated_cost_usd=COALESCE(excluded.estimated_cost_usd,usage_records.estimated_cost_usd),
            cost_source=CASE WHEN {incoming} >= {stored} THEN excluded.cost_source ELSE usage_records.cost_source END",
        incoming = cost_rank("excluded.cost_source"),
        stored = cost_rank("usage_records.cost_source"),
    );
    conn.execute(
        &sql,
        params![
            record.id,
            record.provider,
            record.model_id,
            record.model_revision,
            record.session_id,
            record.run_id,
            record.request_id,
            record.measurement_kind.as_str(),
            record.status,
            record.started_at_ms,
            record.first_output_at_ms,
            record.last_output_at_ms,
            record.completed_at_ms,
            record.input_tokens,
            record.cached_input_tokens,
            record.cache_write_tokens,
            record.reasoning_tokens,
            record.output_tokens,
            record.ttft_ms,
            record.observed_output_tps,
            record.end_to_end_output_tps,
            record.billed_cost_usd,
            record.estimated_cost_usd,
            record.cost_source.as_str(),
            record.source,
            Utc::now().to_rfc3339()
        ],
    )?;
    Ok(())
}

/// Accumulator for one (provider, model) group. Option sums distinguish
/// "no request ever reported this" from a legitimate zero.
#[derive(Default)]
struct Bucket {
    requests: i64,
    input: i64,
    output: i64,
    total: i64,
    cached: Option<i64>,
    cache_writes: Option<i64>,
    reasoning: Option<i64>,
    billed: Option<f64>,
    estimated: Option<f64>,
    any_provider_billed: bool,
    any_cloud_billed: bool,
    decode_tps: Vec<f64>,
    e2e_tps: Vec<f64>,
    ttft: Vec<f64>,
}

impl Bucket {
    fn fold(&mut self, row: &SummaryRow) {
        fn add(slot: &mut Option<i64>, value: Option<i64>) {
            if let Some(value) = value {
                *slot = Some(slot.unwrap_or(0) + value.max(0));
            }
        }
        self.requests += 1;
        self.input += row.input.unwrap_or(0).max(0);
        self.output += row.output.unwrap_or(0).max(0);
        self.total += row.total.unwrap_or(0).max(0);
        add(&mut self.cached, row.cached);
        add(&mut self.cache_writes, row.cache_writes);
        add(&mut self.reasoning, row.reasoning);
        match row.billed {
            Some(billed) => {
                self.billed = Some(self.billed.unwrap_or(0.0) + billed.max(0.0));
                match row.cost_source {
                    CostSource::SynthCloud => self.any_cloud_billed = true,
                    _ => self.any_provider_billed = true,
                }
            }
            // An estimate only counts while its request has no settled
            // charge; once billed, the estimate must not inflate totals.
            None => {
                if let Some(estimated) = row.estimated {
                    self.estimated = Some(self.estimated.unwrap_or(0.0) + estimated.max(0.0));
                }
            }
        }
        if let Some(v) = finite_positive(row.decode_tps) {
            self.decode_tps.push(v);
        }
        if let Some(v) = finite_positive(row.e2e_tps) {
            self.e2e_tps.push(v);
        }
        if let Some(v) = finite_positive(row.ttft_ms) {
            self.ttft.push(v);
        }
    }

    fn into_breakdown(mut self, provider: String, model_id: String) -> UsageBreakdown {
        let cost_source = if self.any_provider_billed {
            CostSource::ProviderReported
        } else if self.any_cloud_billed {
            CostSource::SynthCloud
        } else if self.estimated.is_some() {
            CostSource::TariffEstimate
        } else {
            CostSource::None
        };
        let cache_hit_rate = match (self.cached, self.input) {
            (Some(cached), input) if input > 0 => Some(cached as f64 / input as f64),
            _ => None,
        };
        let non_cached = self.cached.map(|cached| (self.input - cached).max(0));
        let perf_sample_count = self
            .decode_tps
            .len()
            .max(self.e2e_tps.len())
            .max(self.ttft.len()) as i64;
        let mut decode_p95 = self.decode_tps.clone();
        let mut e2e_p95 = self.e2e_tps.clone();
        let mut ttft_p95 = self.ttft.clone();
        UsageBreakdown {
            provider,
            model_id,
            requests: self.requests,
            input_tokens: self.input,
            cached_input_tokens: self.cached,
            non_cached_input_tokens: non_cached,
            cache_write_tokens: self.cache_writes,
            reasoning_tokens: self.reasoning,
            output_tokens: self.output,
            total_tokens: self.total,
            cache_hit_rate,
            billed_cost_usd: self.billed,
            estimated_cost_usd: self.estimated,
            cost_source,
            decode_tps_p50: percentile(&mut self.decode_tps, 0.5),
            decode_tps_p95: percentile(&mut decode_p95, 0.95),
            end_to_end_tps_p50: percentile(&mut self.e2e_tps, 0.5),
            end_to_end_tps_p95: percentile(&mut e2e_p95, 0.95),
            ttft_ms_p50: percentile(&mut self.ttft, 0.5),
            ttft_ms_p95: percentile(&mut ttft_p95, 0.95),
            perf_sample_count,
        }
    }
}

pub(crate) fn percentile(values: &mut [f64], q: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    values
        .get(((values.len() - 1) as f64 * q).round() as usize)
        .copied()
}

struct SummaryRow {
    provider: String,
    model_id: String,
    completed_at_ms: i64,
    input: Option<i64>,
    cached: Option<i64>,
    cache_writes: Option<i64>,
    reasoning: Option<i64>,
    output: Option<i64>,
    total: Option<i64>,
    ttft_ms: Option<f64>,
    decode_tps: Option<f64>,
    e2e_tps: Option<f64>,
    billed: Option<f64>,
    estimated: Option<f64>,
    cost_source: CostSource,
}

fn summarize(
    conn: &Connection,
    window: &str,
    since_ms: Option<i64>,
    local_offset_seconds: i32,
) -> Result<UsageSummary> {
    // Failed and interrupted requests stay in: their tokens were consumed and
    // any charge for them is real. The window is judged on completion time.
    let mut statement = conn.prepare(
        "SELECT provider,model_id,input_tokens,cached_input_tokens,cache_write_tokens,reasoning_tokens,output_tokens,total_tokens,ttft_ms,observed_output_tps,end_to_end_output_tps,billed_cost_usd,estimated_cost_usd,cost_source,completed_at_ms
         FROM usage_records
         WHERE completed_at_ms >= ?1",
    )?;
    let rows = statement.query_map(params![since_ms.unwrap_or(i64::MIN)], |row| {
        Ok(SummaryRow {
            provider: row.get(0)?,
            model_id: row.get(1)?,
            input: row.get(2)?,
            cached: row.get(3)?,
            cache_writes: row.get(4)?,
            reasoning: row.get(5)?,
            output: row.get(6)?,
            total: row.get(7)?,
            ttft_ms: row.get(8)?,
            decode_tps: row.get(9)?,
            e2e_tps: row.get(10)?,
            billed: row.get(11)?,
            estimated: row.get(12)?,
            cost_source: CostSource::parse(&row.get::<_, String>(13)?),
            completed_at_ms: row.get(14)?,
        })
    })?;

    let mut totals = Bucket::default();
    let mut groups: BTreeMap<(String, String), Bucket> = BTreeMap::new();
    // Keyed (day, provider) so the BTreeMap already yields the ascending
    // day-then-provider order the chart draws in.
    let mut daily: BTreeMap<(String, String), Bucket> = BTreeMap::new();
    for row in rows {
        let row = row?;
        totals.fold(&row);
        groups
            .entry((row.provider.clone(), row.model_id.clone()))
            .or_default()
            .fold(&row);
        if let Some(day) = local_day(row.completed_at_ms, local_offset_seconds) {
            daily
                .entry((day, row.provider.clone()))
                .or_default()
                .fold(&row);
        }
    }
    let mut models: Vec<UsageBreakdown> = groups
        .into_iter()
        .map(|((provider, model_id), bucket)| bucket.into_breakdown(provider, model_id))
        .collect();
    models.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));
    let days: Vec<UsageDayPoint> = daily
        .into_iter()
        .map(|((day, provider), bucket)| UsageDayPoint {
            day,
            totals: bucket.into_breakdown(provider, "all".into()),
        })
        .collect();
    Ok(UsageSummary {
        window: window.to_owned(),
        totals: totals.into_breakdown("all".into(), "all".into()),
        models,
        days,
        generated_at: Utc::now().to_rfc3339(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;
    use chrono::TimeZone;

    fn open() -> (tempfile::TempDir, UsageRecordsRepository) {
        let temp = tempfile::tempdir().unwrap();
        let storage = Storage::open(temp.path()).unwrap();
        let repository = UsageRecordsRepository::new(storage.database().clone());
        (temp, repository)
    }

    fn record(provider: &str, model: &str, request: &str) -> UsageRecord {
        UsageRecord {
            id: format!("perf:{provider}:{request}"),
            provider: provider.into(),
            model_id: model.into(),
            model_revision: None,
            session_id: None,
            run_id: None,
            request_id: request.into(),
            measurement_kind: MeasurementKind::ObservedStream,
            status: "completed".into(),
            started_at_ms: 1_000,
            first_output_at_ms: Some(1_200),
            last_output_at_ms: Some(2_000),
            completed_at_ms: 2_000,
            input_tokens: Some(1_000),
            cached_input_tokens: Some(400),
            cache_write_tokens: Some(100),
            reasoning_tokens: Some(50),
            output_tokens: Some(200),
            ttft_ms: Some(200.0),
            observed_output_tps: Some(25.0),
            end_to_end_output_tps: Some(20.0),
            billed_cost_usd: None,
            estimated_cost_usd: None,
            cost_source: CostSource::None,
            source: "test".into(),
        }
    }

    async fn all_time(repository: &UsageRecordsRepository) -> UsageSummary {
        repository.summary("all".into(), None, 0).await.unwrap()
    }

    #[tokio::test]
    async fn a_full_openrouter_request_with_a_settled_charge_reports_billed_money() {
        let (_t, repository) = open();
        let mut r = record("openrouter", "openai/gpt-5.6-luna", "req-1");
        r.estimated_cost_usd = Some(0.01);
        r.cost_source = CostSource::TariffEstimate;
        repository.record(r).await.unwrap();
        assert!(repository
            .record_billed_cost(
                "openrouter".into(),
                "req-1".into(),
                0.0123,
                CostSource::ProviderReported
            )
            .await
            .unwrap());

        let summary = all_time(&repository).await;
        let model = &summary.models[0];
        assert_eq!(model.requests, 1);
        assert_eq!(model.input_tokens, 1_000);
        assert_eq!(model.cached_input_tokens, Some(400));
        assert_eq!(model.non_cached_input_tokens, Some(600));
        assert_eq!(model.cache_write_tokens, Some(100));
        assert_eq!(model.reasoning_tokens, Some(50));
        assert_eq!(model.output_tokens, 200);
        assert_eq!(model.total_tokens, 1_200);
        assert_eq!(model.cache_hit_rate, Some(0.4));
        assert_eq!(model.billed_cost_usd, Some(0.0123));
        // The settled charge replaced the estimate: it must not linger in the
        // unbilled column, and there is exactly one request.
        assert_eq!(model.estimated_cost_usd, None);
        assert_eq!(model.cost_source, CostSource::ProviderReported);
    }

    #[tokio::test]
    async fn an_unsettled_openrouter_request_stays_a_labeled_estimate() {
        let (_t, repository) = open();
        let mut r = record("openrouter", "openai/gpt-5.6-luna", "req-1");
        r.estimated_cost_usd = Some(0.02);
        r.cost_source = CostSource::TariffEstimate;
        repository.record(r).await.unwrap();
        let summary = all_time(&repository).await;
        assert_eq!(summary.totals.billed_cost_usd, None);
        assert_eq!(summary.totals.estimated_cost_usd, Some(0.02));
        assert_eq!(summary.totals.cost_source, CostSource::TariffEstimate);
    }

    #[tokio::test]
    async fn duplicate_completion_and_usage_events_stay_idempotent() {
        let (_t, repository) = open();
        let mut r = record("openrouter", "openai/gpt-5.6-luna", "req-1");
        r.estimated_cost_usd = Some(0.02);
        r.cost_source = CostSource::TariffEstimate;
        repository.record(r.clone()).await.unwrap();
        // A replayed terminal event with sparser fields must not duplicate the
        // request, wipe known facts, or double-count the estimate.
        r.cached_input_tokens = None;
        r.reasoning_tokens = None;
        repository.record(r).await.unwrap();
        let summary = all_time(&repository).await;
        assert_eq!(summary.totals.requests, 1);
        assert_eq!(summary.totals.cached_input_tokens, Some(400));
        assert_eq!(summary.totals.reasoning_tokens, Some(50));
        assert_eq!(summary.totals.estimated_cost_usd, Some(0.02));
    }

    #[tokio::test]
    async fn local_laguna_reports_tokens_and_throughput_without_any_charge() {
        let (_t, repository) = open();
        let mut r = record("local-laguna", "poolside/Laguna-XS-2.1-NVFP4-mlx", "req-1");
        r.cached_input_tokens = None;
        r.cache_write_tokens = None;
        r.reasoning_tokens = None;
        repository.record(r).await.unwrap();
        let summary = all_time(&repository).await;
        let model = &summary.models[0];
        assert_eq!(model.total_tokens, 1_200);
        assert_eq!(model.decode_tps_p50, Some(25.0));
        assert_eq!(model.billed_cost_usd, None);
        assert_eq!(model.estimated_cost_usd, None);
        assert_eq!(model.cost_source, CostSource::None);
        // Unreported counters surface as None — "Unavailable" — never zero.
        assert_eq!(model.cached_input_tokens, None);
        assert_eq!(model.cache_hit_rate, None);
        assert_eq!(model.reasoning_tokens, None);
    }

    #[tokio::test]
    async fn cached_tokens_are_clamped_to_input_and_reasoning_is_not_double_counted() {
        let (_t, repository) = open();
        let mut r = record("openrouter", "openai/gpt-5.6-luna", "req-1");
        r.cached_input_tokens = Some(5_000_000);
        r.reasoning_tokens = Some(150);
        repository.record(r).await.unwrap();
        let summary = all_time(&repository).await;
        let model = &summary.models[0];
        assert_eq!(model.cached_input_tokens, Some(1_000));
        assert_eq!(model.cache_hit_rate, Some(1.0));
        // total = input + output; reasoning is already inside output and must
        // never be added again.
        assert_eq!(model.total_tokens, 1_200);
    }

    #[tokio::test]
    async fn failed_requests_retain_their_tokens_and_charges() {
        let (_t, repository) = open();
        let mut r = record("openrouter", "openai/gpt-5.6-luna", "req-1");
        r.status = "failed".into();
        r.billed_cost_usd = Some(0.004);
        r.cost_source = CostSource::ProviderReported;
        repository.record(r).await.unwrap();
        let summary = all_time(&repository).await;
        assert_eq!(summary.totals.requests, 1);
        assert_eq!(summary.totals.total_tokens, 1_200);
        assert_eq!(summary.totals.billed_cost_usd, Some(0.004));
    }

    #[tokio::test]
    async fn mixed_providers_never_blend_billing_authority() {
        let (_t, repository) = open();
        let mut billed = record("openrouter", "openai/gpt-5.6-luna", "req-1");
        billed.billed_cost_usd = Some(0.03);
        billed.cost_source = CostSource::ProviderReported;
        repository.record(billed).await.unwrap();
        let mut estimated = record("openrouter", "poolside/laguna-s-2.1", "req-2");
        estimated.estimated_cost_usd = Some(0.01);
        estimated.cost_source = CostSource::TariffEstimate;
        repository.record(estimated).await.unwrap();
        let cloud = record("synth-cloud", "openrouter/poolside/laguna-s-2.1", "req-3");
        repository.record(cloud).await.unwrap();

        let summary = all_time(&repository).await;
        let by_model = |model: &str| {
            summary
                .models
                .iter()
                .find(|row| row.model_id == model)
                .cloned()
                .unwrap()
        };
        assert_eq!(
            by_model("openai/gpt-5.6-luna").cost_source,
            CostSource::ProviderReported
        );
        assert_eq!(
            by_model("poolside/laguna-s-2.1").cost_source,
            CostSource::TariffEstimate
        );
        assert_eq!(
            by_model("openrouter/poolside/laguna-s-2.1").cost_source,
            CostSource::None
        );
        // The device totals keep settled and estimated money in separate
        // columns rather than blending them into one figure.
        assert_eq!(summary.totals.billed_cost_usd, Some(0.03));
        assert_eq!(summary.totals.estimated_cost_usd, Some(0.01));
    }

    #[tokio::test]
    async fn aggregation_is_not_capped_at_two_thousand_rows() {
        let (_t, repository) = open();
        for index in 0..2_500 {
            let mut r = record("openrouter", "openai/gpt-5.6-luna", &format!("req-{index}"));
            r.id = format!("perf:openrouter:req-{index}");
            repository.record(r).await.unwrap();
        }
        let summary = all_time(&repository).await;
        assert_eq!(summary.totals.requests, 2_500);
        assert_eq!(summary.totals.input_tokens, 2_500 * 1_000);
    }

    #[tokio::test]
    async fn windows_filter_on_their_boundary_timestamps() {
        let (_t, repository) = open();
        let mut inside = record("openrouter", "openai/gpt-5.6-luna", "inside");
        inside.completed_at_ms = 10_000;
        repository.record(inside).await.unwrap();
        let mut boundary = record("openrouter", "openai/gpt-5.6-luna", "boundary");
        boundary.id = "perf:openrouter:boundary".into();
        boundary.completed_at_ms = 5_000;
        repository.record(boundary).await.unwrap();
        let mut before = record("openrouter", "openai/gpt-5.6-luna", "before");
        before.id = "perf:openrouter:before".into();
        before.completed_at_ms = 4_999;
        repository.record(before).await.unwrap();

        let windowed = repository
            .summary("custom".into(), Some(5_000), 0)
            .await
            .unwrap();
        assert_eq!(windowed.totals.requests, 2);
        assert_eq!(all_time(&repository).await.totals.requests, 3);
    }

    #[test]
    fn window_boundaries_are_computed_in_local_time_for_today() {
        let now = Utc.with_ymd_and_hms(2026, 8, 10, 2, 30, 0).unwrap();
        // UTC-4 (EDT): 2026-08-09 22:30 local, so local midnight is
        // 2026-08-09T04:00:00Z.
        let today = window_start_ms("today", now, -4 * 3600).unwrap();
        assert_eq!(
            today,
            Utc.with_ymd_and_hms(2026, 8, 9, 4, 0, 0)
                .unwrap()
                .timestamp_millis()
        );
        // UTC+2: 2026-08-10 04:30 local, midnight at 2026-08-09T22:00:00Z.
        let today = window_start_ms("today", now, 2 * 3600).unwrap();
        assert_eq!(
            today,
            Utc.with_ymd_and_hms(2026, 8, 9, 22, 0, 0)
                .unwrap()
                .timestamp_millis()
        );
        let day_ms = 24 * 60 * 60 * 1_000;
        assert_eq!(
            window_start_ms("7d", now, 0),
            Some(now.timestamp_millis() - 7 * day_ms)
        );
        assert_eq!(
            window_start_ms("30d", now, 0),
            Some(now.timestamp_millis() - 30 * day_ms)
        );
        assert_eq!(window_start_ms("all", now, 0), None);
    }

    #[tokio::test]
    async fn the_daily_series_buckets_on_the_local_calendar_day() {
        let (_t, repository) = open();
        // 2026-08-10T02:30Z is still the 9th at UTC-4 and already the 10th at
        // UTC+2. The same row must move between bars with the offset.
        let mut late = record("openrouter", "openai/gpt-5.6-luna", "late");
        late.completed_at_ms = Utc
            .with_ymd_and_hms(2026, 8, 10, 2, 30, 0)
            .unwrap()
            .timestamp_millis();
        repository.record(late).await.unwrap();

        let eastern = repository
            .summary("all".into(), None, -4 * 3600)
            .await
            .unwrap();
        assert_eq!(
            eastern
                .days
                .iter()
                .map(|d| d.day.as_str())
                .collect::<Vec<_>>(),
            vec!["2026-08-09"]
        );

        let berlin = repository
            .summary("all".into(), None, 2 * 3600)
            .await
            .unwrap();
        assert_eq!(
            berlin
                .days
                .iter()
                .map(|d| d.day.as_str())
                .collect::<Vec<_>>(),
            vec!["2026-08-10"]
        );
    }

    #[tokio::test]
    async fn the_daily_series_splits_providers_and_reconciles_with_the_totals() {
        let (_t, repository) = open();
        let day_ms = 24 * 60 * 60 * 1_000;
        let base = Utc
            .with_ymd_and_hms(2026, 8, 10, 12, 0, 0)
            .unwrap()
            .timestamp_millis();

        for (provider, model, request, offset_days) in [
            ("openrouter", "openai/gpt-5.6-luna", "a", 0),
            ("openrouter", "openai/gpt-5.6-luna", "b", 0),
            ("synth-cloud", "openrouter/poolside/laguna-s-2.1", "c", 0),
            ("openrouter", "openai/gpt-5.6-luna", "d", 1),
        ] {
            let mut r = record(provider, model, request);
            r.id = format!("perf:{provider}:{request}");
            r.completed_at_ms = base + offset_days * day_ms;
            r.billed_cost_usd = Some(0.01);
            r.cost_source = CostSource::ProviderReported;
            repository.record(r).await.unwrap();
        }

        let summary = repository.summary("all".into(), None, 0).await.unwrap();
        // Ascending by day, then provider — the order the chart draws in.
        assert_eq!(
            summary
                .days
                .iter()
                .map(|d| (
                    d.day.as_str(),
                    d.totals.provider.as_str(),
                    d.totals.requests
                ))
                .collect::<Vec<_>>(),
            vec![
                ("2026-08-10", "openrouter", 2),
                ("2026-08-10", "synth-cloud", 1),
                ("2026-08-11", "openrouter", 1),
            ]
        );

        // The series is a partition of the same rows the headline reduces, so
        // it must add back up to it exactly.
        let charted: i64 = summary.days.iter().map(|d| d.totals.requests).sum();
        assert_eq!(charted, summary.totals.requests);
        let charted_tokens: i64 = summary.days.iter().map(|d| d.totals.total_tokens).sum();
        assert_eq!(charted_tokens, summary.totals.total_tokens);
        let charted_billed: f64 = summary
            .days
            .iter()
            .filter_map(|d| d.totals.billed_cost_usd)
            .sum();
        assert!((charted_billed - summary.totals.billed_cost_usd.unwrap()).abs() < 1e-9);
    }

    #[tokio::test]
    async fn a_settled_synth_cloud_amount_survives_a_weaker_estimate_replay() {
        let (_t, repository) = open();
        let mut r = record("synth-cloud", "openrouter/poolside/laguna-s-2.1", "req-1");
        r.billed_cost_usd = Some(0.03);
        r.cost_source = CostSource::SynthCloud;
        repository.record(r).await.unwrap();

        // A weaker authority may not overwrite the amount, and must not leave
        // its own dollars under the stronger label either.
        assert!(repository
            .record_billed_cost(
                "synth-cloud".into(),
                "req-1".into(),
                0.99,
                CostSource::TariffEstimate
            )
            .await
            .unwrap());
        let summary = all_time(&repository).await;
        assert_eq!(summary.totals.billed_cost_usd, Some(0.03));
        assert_eq!(summary.totals.cost_source, CostSource::SynthCloud);

        // Equal rank re-settles (a replayed receipt), a stronger rank wins.
        assert!(repository
            .record_billed_cost(
                "synth-cloud".into(),
                "req-1".into(),
                0.04,
                CostSource::SynthCloud
            )
            .await
            .unwrap());
        assert_eq!(
            all_time(&repository).await.totals.billed_cost_usd,
            Some(0.04)
        );
        assert!(repository
            .record_billed_cost(
                "synth-cloud".into(),
                "req-1".into(),
                0.05,
                CostSource::ProviderReported
            )
            .await
            .unwrap());
        let summary = all_time(&repository).await;
        assert_eq!(summary.totals.billed_cost_usd, Some(0.05));
        assert_eq!(summary.totals.cost_source, CostSource::ProviderReported);
    }

    #[tokio::test]
    async fn a_settled_charge_for_an_unknown_request_is_reported_not_invented() {
        let (_t, repository) = open();
        assert!(!repository
            .record_billed_cost(
                "openrouter".into(),
                "never-recorded".into(),
                1.0,
                CostSource::ProviderReported
            )
            .await
            .unwrap());
        assert_eq!(all_time(&repository).await.totals.requests, 0);
    }
}
