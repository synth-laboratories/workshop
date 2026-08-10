use super::Database;
use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementKind { Decode, ObservedStream, EndToEnd, ProviderReported }

impl MeasurementKind {
    fn as_str(self) -> &'static str { match self { Self::Decode => "decode", Self::ObservedStream => "observed_stream", Self::EndToEnd => "end_to_end", Self::ProviderReported => "provider_reported" } }
    fn parse(value: &str) -> Self { match value { "decode" => Self::Decode, "end_to_end" => Self::EndToEnd, "provider_reported" => Self::ProviderReported, _ => Self::ObservedStream } }
}

#[derive(Clone, Debug)]
pub struct ModelPerformanceSample {
    pub id: String, pub provider: String, pub model_id: String,
    pub model_revision: Option<String>, pub session_id: Option<String>, pub run_id: Option<String>,
    pub request_id: String, pub measurement_kind: MeasurementKind, pub status: String,
    pub started_at_ms: i64, pub first_output_at_ms: Option<i64>, pub last_output_at_ms: Option<i64>,
    pub completed_at_ms: i64, pub input_tokens: Option<i64>, pub cached_input_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>, pub output_tokens: Option<i64>, pub ttft_ms: Option<f64>,
    pub observed_output_tps: Option<f64>, pub end_to_end_output_tps: Option<f64>, pub source: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelPerformanceSummary {
    pub provider: String, pub model_id: String, pub measurement_kind: MeasurementKind,
    pub sample_count: usize, pub tps_p50: Option<f64>, pub tps_p95: Option<f64>,
    pub ttft_p50_ms: Option<f64>, pub last_observed_at: String,
}

#[derive(Clone)]
pub struct ModelPerformanceRepository { db: Arc<Database> }

impl ModelPerformanceRepository {
    pub fn new(db: Arc<Database>) -> Self { Self { db } }
    pub async fn record(&self, sample: ModelPerformanceSample) -> Result<()> { self.db.run(move |conn| record(conn, sample)).await }
    pub async fn summaries(&self) -> Result<Vec<ModelPerformanceSummary>> { self.db.run(summaries).await }
}

fn finite_positive(value: Option<f64>) -> Option<f64> { value.filter(|value| value.is_finite() && *value > 0.0) }

fn existing_foreign_key(conn: &Connection, table: &str, value: Option<String>) -> Result<Option<String>> {
    let Some(value) = value else { return Ok(None) };
    let exists: bool = conn.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE id = ?1)"), params![&value], |row| row.get(0))?;
    Ok(exists.then_some(value))
}

fn record(conn: &Connection, mut sample: ModelPerformanceSample) -> Result<()> {
    sample.ttft_ms = finite_positive(sample.ttft_ms);
    sample.observed_output_tps = finite_positive(sample.observed_output_tps);
    sample.end_to_end_output_tps = finite_positive(sample.end_to_end_output_tps);
    sample.session_id = existing_foreign_key(conn, "sessions", sample.session_id)?;
    sample.run_id = existing_foreign_key(conn, "runs", sample.run_id)?;
    conn.execute(
        "INSERT INTO model_performance_samples (id,provider,model_id,model_revision,session_id,run_id,request_id,measurement_kind,status,started_at_ms,first_output_at_ms,last_output_at_ms,completed_at_ms,input_tokens,cached_input_tokens,reasoning_tokens,output_tokens,ttft_ms,observed_output_tps,end_to_end_output_tps,source,created_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22)
         ON CONFLICT(provider,request_id) DO UPDATE SET status=excluded.status,first_output_at_ms=COALESCE(excluded.first_output_at_ms,model_performance_samples.first_output_at_ms),last_output_at_ms=COALESCE(excluded.last_output_at_ms,model_performance_samples.last_output_at_ms),completed_at_ms=excluded.completed_at_ms,input_tokens=COALESCE(excluded.input_tokens,model_performance_samples.input_tokens),cached_input_tokens=COALESCE(excluded.cached_input_tokens,model_performance_samples.cached_input_tokens),reasoning_tokens=COALESCE(excluded.reasoning_tokens,model_performance_samples.reasoning_tokens),output_tokens=COALESCE(excluded.output_tokens,model_performance_samples.output_tokens),ttft_ms=COALESCE(excluded.ttft_ms,model_performance_samples.ttft_ms),observed_output_tps=COALESCE(excluded.observed_output_tps,model_performance_samples.observed_output_tps),end_to_end_output_tps=COALESCE(excluded.end_to_end_output_tps,model_performance_samples.end_to_end_output_tps)",
        params![sample.id,sample.provider,sample.model_id,sample.model_revision,sample.session_id,sample.run_id,sample.request_id,sample.measurement_kind.as_str(),sample.status,sample.started_at_ms,sample.first_output_at_ms,sample.last_output_at_ms,sample.completed_at_ms,sample.input_tokens,sample.cached_input_tokens,sample.reasoning_tokens,sample.output_tokens,sample.ttft_ms,sample.observed_output_tps,sample.end_to_end_output_tps,sample.source,Utc::now().to_rfc3339()],
    )?;
    conn.execute(
        "DELETE FROM model_performance_samples WHERE provider=?1 AND model_id=?2 AND id NOT IN (SELECT id FROM model_performance_samples WHERE provider=?1 AND model_id=?2 ORDER BY completed_at_ms DESC,id DESC LIMIT 10000)",
        params![sample.provider, sample.model_id],
    )?;
    Ok(())
}

#[derive(Default)]
struct Aggregate { tps: Vec<f64>, ttft: Vec<f64>, last_ms: i64 }
fn percentile(values: &mut [f64], q: f64) -> Option<f64> { if values.is_empty() { return None } values.sort_by(f64::total_cmp); values.get(((values.len()-1) as f64*q).round() as usize).copied() }

fn summaries(conn: &Connection) -> Result<Vec<ModelPerformanceSummary>> {
    let mut statement = conn.prepare("SELECT provider,model_id,measurement_kind,completed_at_ms,ttft_ms,observed_output_tps,end_to_end_output_tps FROM model_performance_samples WHERE status='completed' AND output_tokens IS NOT NULL AND output_tokens>0 ORDER BY completed_at_ms")?;
    let rows = statement.query_map([], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?,row.get::<_,String>(2)?,row.get::<_,i64>(3)?,row.get::<_,Option<f64>>(4)?,row.get::<_,Option<f64>>(5)?,row.get::<_,Option<f64>>(6)?)))?;
    let mut groups: BTreeMap<(String,String,MeasurementKind),Aggregate> = BTreeMap::new();
    for row in rows { let (provider,model,kind,completed,ttft,observed,e2e)=row?; let a=groups.entry((provider,model,MeasurementKind::parse(&kind))).or_default(); a.last_ms=a.last_ms.max(completed); if let Some(v)=finite_positive(ttft){a.ttft.push(v)} if let Some(v)=finite_positive(observed.or(e2e)){a.tps.push(v)} }
    Ok(groups.into_iter().filter_map(|((provider,model_id,measurement_kind),mut a)| { if a.tps.is_empty(){return None} let mut p95=a.tps.clone(); let sample_count=a.tps.len(); Some(ModelPerformanceSummary{provider,model_id,measurement_kind,sample_count,tps_p50:percentile(&mut a.tps,0.5),tps_p95:percentile(&mut p95,0.95),ttft_p50_ms:percentile(&mut a.ttft,0.5),last_observed_at:chrono::DateTime::<Utc>::from_timestamp_millis(a.last_ms).unwrap_or_default().to_rfc3339()}) }).collect())
}

#[cfg(test)]
mod tests {
    use super::*; use crate::storage::Storage;
    fn sample(request:&str,tps:f64)->ModelPerformanceSample{ModelPerformanceSample{id:format!("perf-{request}"),provider:"openrouter".into(),model_id:"model-a".into(),model_revision:None,session_id:None,run_id:None,request_id:request.into(),measurement_kind:MeasurementKind::ObservedStream,status:"completed".into(),started_at_ms:1000,first_output_at_ms:Some(1100),last_output_at_ms:Some(2100),completed_at_ms:2200,input_tokens:Some(10),cached_input_tokens:None,reasoning_tokens:None,output_tokens:Some(20),ttft_ms:Some(100.),observed_output_tps:Some(tps),end_to_end_output_tps:Some(16.7),source:"codex_app_server".into()}}
    #[tokio::test] async fn records_idempotently_and_returns_model_percentiles(){let t=tempfile::tempdir().unwrap();let s=Storage::open(t.path()).unwrap();let r=ModelPerformanceRepository::new(s.database().clone());for (id,tps) in [("one",10.),("two",20.),("three",30.),("three",30.)]{r.record(sample(id,tps)).await.unwrap()}let v=r.summaries().await.unwrap();assert_eq!(v[0].sample_count,3);assert_eq!(v[0].tps_p50,Some(20.));}
    #[tokio::test] async fn omits_failed_and_tokenless_rows(){let t=tempfile::tempdir().unwrap();let s=Storage::open(t.path()).unwrap();let r=ModelPerformanceRepository::new(s.database().clone());let mut x=sample("failed",40.);x.status="failed".into();r.record(x).await.unwrap();let mut x=sample("tokenless",40.);x.output_tokens=None;x.observed_output_tps=None;r.record(x).await.unwrap();assert!(r.summaries().await.unwrap().is_empty());}
}
