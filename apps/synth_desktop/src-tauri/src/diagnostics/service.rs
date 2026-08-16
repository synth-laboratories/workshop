//! The diagnostics facade: emit, status, query, tail, explain, bundle.
//!
//! Ordering here is the whole design. Emission is a queue push. Persistence is
//! a batched background write to the authoritative journal. Indexing is a
//! background pass that only ever reads committed rows. Queries prefer the
//! index and fall back to the journal automatically. No step in that chain is
//! allowed to make the step before it wait.

use super::bus::{DiagnosticBus, Enqueued, DRAIN_BATCH};
use super::codes;
use super::event::{validate, DiagnosticEvent, DiagnosticInput, Severity};
use super::explain::{self, IdentitySet};
use super::indexer::Indexer;
use super::query::{self, DiagnosticQuery};
use super::sidecar::{SidecarConfig, SidecarState, VictoriaLogsSidecar};
use super::store::{group_by_code, DiagnosticRecord, DiagnosticStore};
use super::victorialogs::VictoriaLogsClient;
use crate::storage::{Database, EventJournal};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// Longest a queued diagnostic waits before being written even if the batch
/// never fills. Short enough that a failure is queryable while the user is
/// still looking at it.
pub const FLUSH_INTERVAL: Duration = Duration::from_millis(250);

/// Pause between indexing passes when the index is caught up.
pub const INDEX_IDLE: Duration = Duration::from_secs(2);

/// Pause before retrying after the index refused a batch.
pub const INDEX_RETRY: Duration = Duration::from_secs(15);

/// Delay before the sidecar is started, measured from the call that follows
/// the main window becoming interactive.
pub const LAZY_START_DELAY: Duration = Duration::from_secs(3);

/// How long diagnostics stay in the authoritative journal.
///
/// Deliberately longer than the index's 7 days: the journal is what the index
/// is rebuilt from, so it has to outlive it. The evidence that matters —
/// traces, run records, seals — is stored elsewhere and is never touched here.
pub const JOURNAL_RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// Row ceiling, so a burst cannot outrun the age window.
pub const JOURNAL_MAX_ROWS: i64 = 200_000;

/// How often the trim runs. Retention is a floor, not a deadline.
pub const TRIM_INTERVAL: Duration = Duration::from_secs(60 * 60);

#[derive(Default)]
struct Stats {
    persisted: AtomicU64,
    rejected: AtomicU64,
    write_failures: AtomicU64,
    indexed: AtomicU64,
    index_failures: AtomicU64,
}

pub struct DiagnosticsService {
    bus: Arc<DiagnosticBus>,
    store: DiagnosticStore,
    sidecar: Arc<VictoriaLogsSidecar>,
    indexer: Indexer,
    root: PathBuf,
    instance_id: Option<String>,
    started: AtomicBool,
    stats: Stats,
}

impl DiagnosticsService {
    pub fn new(db: Arc<Database>, journal: EventJournal, root: impl Into<PathBuf>) -> Arc<Self> {
        let root = root.into();
        let store = DiagnosticStore::new(db, journal);
        Arc::new(Self {
            bus: DiagnosticBus::new(),
            indexer: Indexer::new(store.clone(), root.clone()),
            store,
            sidecar: VictoriaLogsSidecar::new(SidecarConfig::for_root(root.clone())),
            root,
            instance_id: crate::instance::name(),
            started: AtomicBool::new(false),
            stats: Stats::default(),
        })
    }

    pub fn bus(&self) -> &Arc<DiagnosticBus> {
        &self.bus
    }

    pub fn store(&self) -> &DiagnosticStore {
        &self.store
    }

    pub fn sidecar(&self) -> &Arc<VictoriaLogsSidecar> {
        &self.sidecar
    }

    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    /// Record one diagnostic. Returns immediately.
    ///
    /// An invalid envelope is counted and dropped rather than propagated: a
    /// producer path must not acquire a new failure mode by describing one.
    pub fn emit(&self, mut input: DiagnosticInput) -> Enqueued {
        if input.correlation.instance_id.is_none() {
            input.correlation.instance_id = self.instance_id.clone();
        }
        match validate(input) {
            Ok(event) => self.bus.enqueue(event),
            Err(_) => {
                self.stats.rejected.fetch_add(1, Ordering::Relaxed);
                Enqueued::DroppedIncoming
            }
        }
    }

    /// Emit and wait for the event to be durable. Tests and the renderer's
    /// synchronous report command use this; production hot paths must not.
    pub async fn emit_now(&self, input: DiagnosticInput) -> Result<DiagnosticRecord> {
        let mut input = input;
        if input.correlation.instance_id.is_none() {
            input.correlation.instance_id = self.instance_id.clone();
        }
        let event = validate(input)?;
        let mut written = self.store.append_batch(vec![event]).await?;
        self.stats.persisted.fetch_add(1, Ordering::Relaxed);
        written.pop().context("diagnostic append returned no row")
    }

    /// Drain whatever is queued right now. Called by the writer loop and,
    /// synchronously, by queries so a just-emitted diagnostic is visible.
    pub async fn flush(&self) -> Result<usize> {
        let mut total = 0usize;
        loop {
            let batch = self.bus.drain(DRAIN_BATCH);
            if batch.is_empty() {
                break;
            }
            total += batch.len();
            self.store.append_batch(batch).await?;
        }
        self.stats
            .persisted
            .fetch_add(total as u64, Ordering::Relaxed);
        if let Some(report) = self.bus.take_saturation() {
            // Exactly one bounded saturation diagnostic per recovery.
            let mut input = DiagnosticInput::new(
                Severity::Warn,
                "diagnostics",
                "diagnostics.queue.saturated",
                codes::DIAGNOSTICS_QUEUE_SATURATED,
                format!("dropped {} diagnostic events under load", report.dropped),
            );
            input
                .details
                .insert("dropped".into(), json!(report.dropped));
            input.details.insert(
                "by_severity_component".into(),
                json!(report.by_severity_component),
            );
            let _ = self.store.append_batch(vec![validate(input)?]).await;
        }
        Ok(total)
    }

    /// Drain before a read so a just-emitted diagnostic is visible.
    ///
    /// A failed drain must not fail the query — the answer is simply missing
    /// whatever could not be written — but it must not be invisible either, or
    /// a full disk reads as "no diagnostics were recorded".
    async fn flush_before_read(&self) {
        if let Err(error) = self.flush().await {
            self.stats.write_failures.fetch_add(1, Ordering::Relaxed);
            eprintln!("synth-desktop: diagnostics could not be persisted: {error:#}");
        }
    }

    /// Start the background writer, then lazily the index.
    ///
    /// Call this *after* the main window is interactive. Nothing in here is on
    /// a startup critical path, and a failure to start anything leaves the
    /// service usable in journal-only mode.
    pub fn start(self: &Arc<Self>) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let writer = self.clone();
        tauri::async_runtime::spawn(async move {
            loop {
                tokio::select! {
                    _ = writer.bus.wait_for_work() => {}
                    _ = tokio::time::sleep(FLUSH_INTERVAL) => {}
                }
                if let Err(error) = writer.flush().await {
                    eprintln!("synth-desktop: diagnostics writer failed: {error:#}");
                    tokio::time::sleep(INDEX_RETRY).await;
                }
            }
        });

        let indexing = self.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(LAZY_START_DELAY).await;
            indexing.run_index_loop().await;
        });
    }

    async fn run_index_loop(self: Arc<Self>) {
        let mut last_trim = std::time::Instant::now();
        loop {
            if last_trim.elapsed() >= TRIM_INTERVAL {
                last_trim = std::time::Instant::now();
                match self.store.trim(JOURNAL_RETENTION, JOURNAL_MAX_ROWS).await {
                    Ok(0) => {}
                    Ok(removed) => eprintln!("synth-desktop: trimmed {removed} stale diagnostics"),
                    Err(error) => {
                        eprintln!("synth-desktop: diagnostics trim failed: {error:#}")
                    }
                }
            }
            let state = self.sidecar.state().await;
            let ready = match state {
                SidecarState::Ready if !self.sidecar.exited().await => SidecarState::Ready,
                SidecarState::Ready => self.sidecar.restart_with_backoff().await,
                SidecarState::Degraded(_) | SidecarState::Stopped => self.sidecar.start().await,
                SidecarState::Starting => {
                    tokio::time::sleep(INDEX_IDLE).await;
                    continue;
                }
            };
            if ready != SidecarState::Ready {
                // Degraded is a status, not an outage: queries keep answering
                // from the journal until the next attempt.
                tokio::time::sleep(INDEX_RETRY).await;
                continue;
            }
            let Some(url) = self.sidecar.url().await else {
                tokio::time::sleep(INDEX_RETRY).await;
                continue;
            };
            let Ok(client) = VictoriaLogsClient::new(url) else {
                tokio::time::sleep(INDEX_RETRY).await;
                continue;
            };
            match self.indexer.index_once(&client).await {
                Ok(progress) => {
                    self.stats
                        .indexed
                        .fetch_add(progress.indexed as u64, Ordering::Relaxed);
                    if progress.lag == 0 {
                        tokio::time::sleep(INDEX_IDLE).await;
                    }
                }
                Err(error) => {
                    self.stats.index_failures.fetch_add(1, Ordering::Relaxed);
                    eprintln!("synth-desktop: diagnostics indexing failed: {error:#}");
                    tokio::time::sleep(INDEX_RETRY).await;
                }
            }
        }
    }

    /// Status for the MCP, the Diagnostics pane, and support bundles.
    pub async fn status(&self) -> Value {
        let state = self.sidecar.state().await;
        let (stored, oldest) = self.store.summary().await.unwrap_or((0, None));
        json!({
            "schema": "synth.diagnostics-status.v1",
            "state": state.label(),
            "reason": state.reason(),
            "local_only": true,
            "index_url": self.sidecar.url().await,
            "instance": self.instance_id,
            "data_dir": self.root.display().to_string(),
            "retention_days": self.sidecar.config().retention_days,
            "journal_retention_days": JOURNAL_RETENTION.as_secs() / 86_400,
            "journal_max_rows": JOURNAL_MAX_ROWS,
            "quota_bytes": self.sidecar.config().quota_bytes,
            "index_bytes": self.sidecar.index_size_bytes(),
            "index_lag": self.indexer.lag().await,
            "queue": {
                "depth": self.bus.depth(),
                "capacity": self.bus.capacity(),
                "enqueued": self.bus.enqueued_total(),
            },
            "stored_events": stored,
            "oldest_event": oldest,
            "persisted": self.stats.persisted.load(Ordering::Relaxed),
            "rejected": self.stats.rejected.load(Ordering::Relaxed),
            "write_failures": self.stats.write_failures.load(Ordering::Relaxed),
            "indexed": self.stats.indexed.load(Ordering::Relaxed),
            "index_failures": self.stats.index_failures.load(Ordering::Relaxed),
        })
    }

    /// Typed query. Uses the index when it is ready, the journal otherwise —
    /// and the journal always supplies the records themselves.
    pub async fn query(&self, request: DiagnosticQuery) -> Result<Value> {
        self.flush_before_read().await;
        let (records, source) = self.search(&request).await?;
        let (page, truncated) = bound_response(records, request.limit);
        let cursor = page.last().map(|record| record.sequence);
        Ok(json!({
            "schema": "synth.diagnostics-result.v1",
            "source": source,
            "count": page.len(),
            "truncated": truncated,
            "cursor": cursor,
            "groups": group_by_code(&page),
            "events": page.iter().map(DiagnosticRecord::to_json).collect::<Vec<_>>(),
        }))
    }

    /// Newest events, no paging. A cursor on a tail is a contradiction.
    pub async fn tail(&self, mut request: DiagnosticQuery) -> Result<Value> {
        request.cursor = None;
        request.limit = request.limit.min(50);
        self.query(request).await
    }

    async fn search(&self, request: &DiagnosticQuery) -> Result<(Vec<DiagnosticRecord>, &'static str)> {
        if matches!(self.sidecar.state().await, SidecarState::Ready) {
            if let Some(url) = self.sidecar.url().await {
                match self.search_index(&url, request).await {
                    Ok(records) => return Ok((records, "victorialogs")),
                    Err(error) => {
                        // The index is an optimization. Falling back is the
                        // designed behavior, not an error the caller handles.
                        eprintln!("synth-desktop: diagnostics index query failed: {error:#}");
                    }
                }
            }
        }
        Ok((self.store.search(request.clone()).await?, "journal"))
    }

    async fn search_index(&self, url: &str, request: &DiagnosticQuery) -> Result<Vec<DiagnosticRecord>> {
        let client = VictoriaLogsClient::new(url)?;
        let logsql = super::victorialogs::compile(request, chrono::Utc::now())?;
        let sequences = tokio::time::timeout(
            query::QUERY_TIMEOUT,
            client.search_sequences(&logsql, request.limit * 4),
        )
        .await
        .context("diagnostics index query timed out")??;
        let page: Vec<i64> = sequences
            .into_iter()
            .filter(|sequence| request.cursor.is_none_or(|cursor| *sequence < cursor))
            .take(request.limit)
            .collect();
        self.store.load_by_sequences(page).await
    }

    /// Gather and order the causal neighborhood around supplied identities.
    pub async fn explain(&self, request: DiagnosticQuery) -> Result<Value> {
        self.flush_before_read().await;
        let mut identities = IdentitySet::from_correlation(&request.correlation);
        if identities.is_empty() {
            anyhow::bail!("diagnostics explain requires at least one correlation identity");
        }
        let mut collected: std::collections::BTreeMap<i64, DiagnosticRecord> = Default::default();
        let mut frontier = identities.pairs();
        for _ in 0..=explain::EXPANSION_HOPS {
            let mut discovered = Vec::new();
            for (field, value) in std::mem::take(&mut frontier) {
                let mut scoped = request.clone();
                scoped.correlation = Default::default();
                scoped.correlation.set(&field, Some(value));
                scoped.cursor = None;
                let (records, _) = self.search(&scoped).await?;
                discovered.extend(records);
            }
            if discovered.is_empty() {
                break;
            }
            let added = identities.absorb(&discovered);
            for record in discovered {
                collected.insert(record.sequence, record);
            }
            if added == 0 {
                break;
            }
            frontier = identities.pairs();
        }
        let mut records: Vec<DiagnosticRecord> = collected.into_values().collect();
        records.sort_by(|left, right| right.sequence.cmp(&left.sequence));
        records.truncate(request.limit.max(query::DEFAULT_LIMIT));
        Ok(explain::build(&records, &identities))
    }

    /// Write a redacted, bounded local support artifact. Nothing is uploaded.
    pub async fn bundle(&self, request: DiagnosticQuery) -> Result<Value> {
        let result = self.query(request).await?;
        let bundles = self.root.join("bundles");
        std::fs::create_dir_all(&bundles).context("create diagnostics bundle directory")?;
        let name = format!(
            "diagnostics-bundle-{}.json",
            chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
        );
        let path = bundles.join(&name);
        let body = json!({
            "schema": "synth.diagnostics-bundle.v1",
            "generated_at": chrono::Utc::now().to_rfc3339(),
            "local_only": true,
            "status": self.status().await,
            "result": result,
        });
        std::fs::write(&path, serde_json::to_vec_pretty(&body)?)
            .with_context(|| format!("write diagnostics bundle {}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(json!({
            "schema": "synth.diagnostics-bundle-receipt.v1",
            "path": path.display().to_string(),
            "bytes": std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0),
            "events": body["result"]["count"],
            "uploaded": false,
        }))
    }

    /// Drop the disposable index. Journal rows, traces, and run evidence stay.
    pub async fn clear_index(&self) -> Result<Value> {
        self.sidecar.clear_index().await?;
        Ok(json!({
            "cleared": true,
            "authoritative_events_retained": self.store.summary().await.unwrap_or((0, None)).0,
        }))
    }
}

/// Enforce the response byte ceiling by dropping whole records off the tail.
fn bound_response(records: Vec<DiagnosticRecord>, limit: usize) -> (Vec<DiagnosticRecord>, bool) {
    let mut bytes = 0usize;
    let mut page = Vec::with_capacity(records.len().min(limit));
    let mut truncated = false;
    for record in records.into_iter().take(limit) {
        let size = record.to_json().to_string().len();
        if bytes + size > query::MAX_RESPONSE_BYTES && !page.is_empty() {
            truncated = true;
            break;
        }
        bytes += size;
        page.push(record);
    }
    (page, truncated)
}

/// Emit helper for call sites that hold an `Option<Arc<DiagnosticsService>>`.
pub fn emit_optional(service: Option<&Arc<DiagnosticsService>>, input: DiagnosticInput) {
    if let Some(service) = service {
        service.emit(input);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagnostics::event::Correlation;
    use crate::storage::Storage;
    use tempfile::tempdir;

    fn service(root: &std::path::Path) -> Arc<DiagnosticsService> {
        let storage = Storage::open(root.join("data")).unwrap();
        let journal = EventJournal::new(storage.database().clone());
        DiagnosticsService::new(
            storage.database().clone(),
            journal,
            root.join("diagnostics"),
        )
    }

    fn projection_failure() -> DiagnosticInput {
        let mut input = DiagnosticInput::new(
            Severity::Error,
            "visual-host",
            "visual.projection.rejected",
            codes::UNSUPPORTED_TRACE_PROJECTION_SCHEMA,
            "Unsupported trace projection schema: synth.trace.v5",
        );
        input.correlation.visual_id = Some("vis_9".into());
        input.correlation.visual_revision = Some(14);
        input.correlation.trace_id = Some("trace_1".into());
        input
            .details
            .insert("received_schema".into(), json!("synth.trace.v5"));
        input
    }

    #[tokio::test]
    async fn the_reproduced_renderer_failure_becomes_one_typed_answer() {
        let dir = tempdir().unwrap();
        let service = service(dir.path());
        service.emit(projection_failure());

        let result = service
            .query(DiagnosticQuery {
                correlation: Correlation {
                    visual_id: Some("vis_9".into()),
                    ..Default::default()
                },
                ..Default::default()
            })
            .await
            .unwrap();

        assert_eq!(result["source"], json!("journal"));
        assert_eq!(result["count"], json!(1));
        let event = &result["events"][0];
        assert_eq!(event["code"], json!(codes::UNSUPPORTED_TRACE_PROJECTION_SCHEMA));
        assert_eq!(event["details"]["received_schema"], json!("synth.trace.v5"));
        assert_eq!(event["visual_id"], json!("vis_9"));
        assert_eq!(event["trace_id"], json!("trace_1"));
    }

    #[tokio::test]
    async fn a_degraded_index_still_answers_from_the_journal() {
        let dir = tempdir().unwrap();
        let service = service(dir.path());
        service.emit(projection_failure());
        let status = service.status().await;
        assert_eq!(status["state"], json!("stopped"));
        assert_eq!(status["local_only"], json!(true));

        let result = service.query(DiagnosticQuery::default()).await.unwrap();
        assert_eq!(result["source"], json!("journal"));
        assert_eq!(result["count"], json!(1));
    }

    #[tokio::test]
    async fn explain_names_the_upstream_cause_across_correlated_identities() {
        let dir = tempdir().unwrap();
        let service = service(dir.path());

        let mut rejection = DiagnosticInput::new(
            Severity::Error,
            "containers",
            "container.capability.rejected",
            codes::CONTAINER_CAPABILITY_REJECTED,
            "container ctr_1 does not declare rollouts/start",
        );
        rejection.correlation.container_id = Some("ctr_1".into());
        rejection.correlation.rollout_id = Some("roll_1".into());
        rejection
            .details
            .insert("missing_operations".into(), json!(["rollouts/start"]));

        let mut symptom = projection_failure();
        symptom.correlation.rollout_id = Some("roll_1".into());

        service.emit(rejection);
        service.emit(symptom);

        let explained = service
            .explain(DiagnosticQuery {
                correlation: Correlation {
                    visual_id: Some("vis_9".into()),
                    ..Default::default()
                },
                ..Default::default()
            })
            .await
            .unwrap();

        assert_eq!(
            explained["cause"]["code"],
            json!(codes::CONTAINER_CAPABILITY_REJECTED)
        );
        assert_eq!(
            explained["symptoms"][0]["code"],
            json!(codes::UNSUPPORTED_TRACE_PROJECTION_SCHEMA)
        );
        // The caller only knew the visual; the container was discovered.
        assert!(explained["identities"]["container_id"]
            .as_array()
            .unwrap()
            .contains(&json!("ctr_1")));
    }

    #[tokio::test]
    async fn explain_refuses_to_run_without_an_identity() {
        let dir = tempdir().unwrap();
        let service = service(dir.path());
        assert!(service.explain(DiagnosticQuery::default()).await.is_err());
    }

    #[tokio::test]
    async fn an_invalid_envelope_is_counted_not_propagated() {
        let dir = tempdir().unwrap();
        let service = service(dir.path());
        let mut invalid = projection_failure();
        invalid.component = "not-a-component".into();
        assert_eq!(service.emit(invalid), Enqueued::DroppedIncoming);
        assert_eq!(service.status().await["rejected"], json!(1));
        assert_eq!(service.status().await["stored_events"], json!(0));
    }

    #[tokio::test]
    async fn saturation_is_reported_once_with_counts_by_severity_and_component() {
        let dir = tempdir().unwrap();
        let service = service(dir.path());
        // Fill the informational lane past its bound.
        for index in 0..(super::super::bus::NORMAL_CAPACITY + 10) {
            let mut input = DiagnosticInput::new(
                Severity::Info,
                "renderer",
                "test.event",
                "test_code",
                format!("event {index}"),
            );
            input.timestamp = Some("2026-08-16T00:00:00Z".into());
            service.emit(input);
        }
        service.flush().await.unwrap();

        let saturation = service
            .query(DiagnosticQuery {
                codes: vec![codes::DIAGNOSTICS_QUEUE_SATURATED.into()],
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(saturation["count"], json!(1));
        assert!(saturation["events"][0]["details"]["dropped"].as_u64().unwrap() >= 10);
    }

    #[tokio::test]
    async fn errors_survive_a_flood_of_informational_events() {
        let dir = tempdir().unwrap();
        let service = service(dir.path());
        for index in 0..(super::super::bus::NORMAL_CAPACITY + 500) {
            service.emit(DiagnosticInput::new(
                Severity::Info,
                "renderer",
                "test.event",
                "test_code",
                format!("noise {index}"),
            ));
        }
        service.emit(projection_failure());
        service.flush().await.unwrap();

        let errors = service
            .query(DiagnosticQuery {
                severities: vec![Severity::Error],
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(errors["count"], json!(1));
        assert_eq!(
            errors["events"][0]["code"],
            json!(codes::UNSUPPORTED_TRACE_PROJECTION_SCHEMA)
        );
    }

    #[tokio::test]
    async fn a_bundle_is_local_redacted_and_private() {
        let dir = tempdir().unwrap();
        let service = service(dir.path());
        let mut input = projection_failure();
        input
            .details
            .insert("authorization".into(), json!("Bearer sk-abcdefghijklmnop"));
        service.emit(input);

        let receipt = service.bundle(DiagnosticQuery::default()).await.unwrap();
        assert_eq!(receipt["uploaded"], json!(false));
        let path = receipt["path"].as_str().unwrap();
        let body = std::fs::read_to_string(path).unwrap();
        assert!(!body.contains("sk-abcdefghijklmnop"), "bundle leaked a token");
        assert!(body.contains("synth.diagnostics-bundle.v1"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[tokio::test]
    async fn clearing_the_index_keeps_the_authoritative_events() {
        let dir = tempdir().unwrap();
        let service = service(dir.path());
        service.emit(projection_failure());
        service.flush().await.unwrap();
        let cleared = service.clear_index().await.unwrap();
        assert_eq!(cleared["authoritative_events_retained"], json!(1));
        assert_eq!(
            service.query(DiagnosticQuery::default()).await.unwrap()["count"],
            json!(1)
        );
    }

    #[tokio::test]
    async fn responses_are_bounded_by_bytes_as_well_as_rows() {
        let dir = tempdir().unwrap();
        let service = service(dir.path());
        for index in 0..200 {
            let mut input = DiagnosticInput::new(
                Severity::Error,
                "renderer",
                "test.event",
                "test_code",
                "x".repeat(1_800),
            );
            input.correlation.session_id = Some(format!("sess_{index}"));
            service.emit(input);
        }
        service.flush().await.unwrap();
        let result = service
            .query(DiagnosticQuery {
                limit: query::MAX_LIMIT,
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(result["truncated"], json!(true));
        assert!(result.to_string().len() < query::MAX_RESPONSE_BYTES * 2);
    }

    #[tokio::test]
    async fn a_tail_never_pages() {
        let dir = tempdir().unwrap();
        let service = service(dir.path());
        service.emit(projection_failure());
        let tail = service
            .tail(DiagnosticQuery {
                limit: query::MAX_LIMIT,
                cursor: Some(1),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(tail["count"], json!(1));
    }
}
