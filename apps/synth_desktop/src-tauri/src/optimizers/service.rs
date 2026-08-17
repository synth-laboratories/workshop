use super::models::{
    OptimizerCapabilities, OptimizerCreateRequest, OptimizerEventEnvelope, OptimizerQuery,
    OptimizerRelationship, OptimizerResourceRef, OptimizerRunRecord, OptimizerStateSlice,
    OptimizerUsageSummary, OPTIMIZER_EVENT_SCHEMA_VERSION, OPTIMIZER_RUN_SCHEMA_VERSION,
    OPTIMIZER_STATE_SLICE_SCHEMA_VERSION,
};
use crate::storage::{append_event, AppEvent, Database, EventAppend, EventJournal, EventSource};
use crate::visuals::{VisualCreateRequest, VisualRegistry, VISUAL_BINDINGS_SCHEMA_VERSION};
use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, watch, Mutex};
use uuid::Uuid;

const GEPA_FIXTURE_ID: &str = "opt_gepa_fixture";
const SFT_FIXTURE_ID: &str = "opt_sft_fixture";
const GOEX_FIXTURE_ID: &str = "opt_goex_fixture";

fn validate_control(run: &OptimizerRunRecord, command: &str) -> Result<()> {
    match command {
        "cancel" if !run.capabilities.cancel => bail!("cancel is not available for this run"),
        "pause" if !run.capabilities.pause => bail!("pause is not available for this run"),
        "resume" if !run.capabilities.resume => bail!("resume is not available for this run"),
        _ => {}
    }
    if matches!(run.status.as_str(), "completed" | "failed" | "cancelled") {
        bail!("{command} is not available for a {} run", run.status);
    }
    match command {
        "pause" if run.status != "running" => {
            bail!(
                "pause requires a running optimizer; current status is {}",
                run.status
            )
        }
        "resume" if run.status != "paused" => {
            bail!(
                "resume requires a paused optimizer; current status is {}",
                run.status
            )
        }
        _ => Ok(()),
    }
}

#[derive(Clone)]
pub struct OptimizerService {
    db: Arc<Database>,
    #[allow(dead_code)]
    journal: EventJournal,
    visuals: VisualRegistry,
    local_recipes: Arc<Mutex<HashMap<String, watch::Sender<bool>>>>,
    events_tx: broadcast::Sender<AppEvent>,
    manager: Arc<super::OptimizerManager>,
    /// Attached once by the composition root. Optimizer lifecycle failures are
    /// already recorded as bounded run evidence; this lets the same failure
    /// also be correlated with the container, stream, and visual around it —
    /// without inventing a second source of truth for the run itself.
    diagnostics: Arc<std::sync::OnceLock<Arc<crate::diagnostics::DiagnosticsService>>>,
}

impl OptimizerService {
    pub fn new(
        db: Arc<Database>,
        journal: EventJournal,
        visuals: VisualRegistry,
        events_tx: broadcast::Sender<AppEvent>,
    ) -> Self {
        Self::new_with_manager(
            db,
            journal,
            visuals,
            events_tx,
            Arc::new(super::OptimizerManager::new()),
        )
    }

    pub fn new_with_manager(
        db: Arc<Database>,
        journal: EventJournal,
        visuals: VisualRegistry,
        events_tx: broadcast::Sender<AppEvent>,
        manager: Arc<super::OptimizerManager>,
    ) -> Self {
        Self {
            db,
            journal,
            visuals,
            local_recipes: Arc::new(Mutex::new(HashMap::new())),
            events_tx,
            manager,
            diagnostics: Arc::new(std::sync::OnceLock::new()),
        }
    }

    pub fn manager(&self) -> &Arc<super::OptimizerManager> {
        &self.manager
    }

    /// Wire diagnostics in after both services exist. Idempotent; a service
    /// that is never attached simply emits nothing.
    pub fn attach_diagnostics(&self, service: Arc<crate::diagnostics::DiagnosticsService>) {
        let _ = self.diagnostics.set(service);
    }

    pub(crate) fn diagnostics(&self) -> Option<&Arc<crate::diagnostics::DiagnosticsService>> {
        self.diagnostics.get()
    }

    pub fn list_algorithms(&self) -> Vec<Value> {
        vec![
            json!({"id":"gepa","title":"GEPA","availability":"available","description":"Genetic-Pareto prompt optimization"}),
            json!({"id":"go-ex","title":"GELO / Go-Ex","availability":"available","description":"Hosted exploration with optional local slot binding"}),
            json!({"id":"sft","title":"SFT","availability":"available","description":"Hosted fine-tuning through the public Optimizers SFT service, streamed live into optimizer.sft visuals"}),
            // Local only, and only when its own runtime and a pinned target
            // recipe preflight. `eval` is never hosted.
            super::eval_recipes::algorithm_entry(),
        ]
    }

    pub fn list_recipes(&self) -> Vec<Value> {
        let mut recipes = super::recipes::recipe_catalog();
        recipes.push(super::hosted_gelo::recipe_catalog());
        recipes.push(super::sft_recipes::recipe_catalog());
        recipes.extend(super::hosted_sft::recipe_catalog());
        recipes.extend(super::eval_recipes::recipe_catalog());
        recipes
    }

    pub async fn start_recipe(
        &self,
        request: super::models::OptimizerRecipeRunRequest,
    ) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
        match request.recipe_id.as_str() {
            super::recipes::BANKING77_GEPA_SMOKE_RECIPE
            | super::recipes::BANKING77_GEPA_LUNA_RECIPE
            | super::recipes::BANKING77_GEPA_SOL_RECIPE
            | super::recipes::CRAFTAX_GEPA_SMOKE_RECIPE => {
                super::recipes::start(self, request).await
            }
            super::sft_recipes::CRAFTAX_SFT_SMOKE_RECIPE => {
                super::sft_recipes::start(self, request).await
            }
            super::hosted_gelo::HOSTED_GELO_CRAFTAX_RECIPE => {
                super::hosted_gelo::start(self, request).await
            }
            super::hosted_sft::HOSTED_SFT_FIXTURE_RECIPE
            | super::hosted_sft::HOSTED_SFT_CRAFTAX_NEMOTRON_RECIPE
            | super::hosted_sft::HOSTED_SFT_BANKING77_RECIPE => {
                super::hosted_sft::start(self, request).await
            }
            id if super::eval_recipes::is_eval_recipe(id) => {
                super::eval_recipes::start(self, request).await
            }
            _ => bail!("unknown optimizer recipe: {}", request.recipe_id),
        }
    }

    /// Freeze workspace policy source into an immutable content-addressed set
    /// before any local eval recipe can start.
    pub async fn stage_eval_candidates(
        &self,
        request: super::eval_candidates::EvalStageCandidatesRequest,
    ) -> Result<Value> {
        super::eval_candidates::stage(&self.db, request).await
    }

    pub async fn prepare_recipe(
        &self,
        request: super::models::OptimizerRecipeRunRequest,
    ) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
        match request.recipe_id.as_str() {
            super::recipes::BANKING77_GEPA_SMOKE_RECIPE
            | super::recipes::BANKING77_GEPA_LUNA_RECIPE
            | super::recipes::BANKING77_GEPA_SOL_RECIPE
            | super::recipes::CRAFTAX_GEPA_SMOKE_RECIPE => {
                super::recipes::prepare(self, request).await
            }
            _ => bail!(
                "prepare is only implemented for bounded product-owned GEPA recipes; got {}",
                request.recipe_id
            ),
        }
    }

    pub async fn start_prepared(
        &self,
        optimizer_run_id: String,
        preparation_digest: Option<String>,
        approval_receipt_id: Option<String>,
    ) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
        let run = self.get(optimizer_run_id.clone()).await?;
        let expected = run.summary.get("preparationDigest").and_then(Value::as_str);
        if let Some(expected) = expected {
            if preparation_digest.as_deref() != Some(expected) {
                bail!("preparation digest mismatch; refusing to start paid compute");
            }
        }
        let ready = run.summary.get("visualReadyReceipt").cloned();
        if ready.is_none() {
            bail!("visual readiness receipt is required before starting paid compute");
        }
        if approval_receipt_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
        {
            bail!("compute approval receipt is required before starting paid compute");
        }
        // Both digests must be present and equal. Treating either absence as
        // "nothing to compare" fails open: a run prepared without a proven
        // handshake would start unguarded, which is the case the pin exists for.
        let current_caps = self.manager.advertised_capabilities();
        let prepared_digest = run
            .summary
            .get("capabilitiesDigest")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                anyhow!(
                    "run was prepared without a proven optimizer capability digest; \
                     re-prepare it against a started sidecar before starting paid compute"
                )
            })?;
        let current_digest = current_caps
            .get("digest")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                anyhow!(
                    "optimizer capabilities are not proven; the sidecar must complete a \
                     capability handshake before starting paid compute"
                )
            })?;
        if current_digest != prepared_digest {
            bail!("optimizer capability digest changed since prepare; refusing to start");
        }
        // A matching digest proves the capabilities are unchanged, not that they
        // cover this run. Shape-validation alone would accept a handshake
        // advertising a wholly unrelated algorithm, so check the one claim that
        // matters before paying for rollouts.
        require_advertised_algorithm(&current_caps, &run.algorithm_id)?;
        super::recipes::start_prepared(self, &optimizer_run_id).await
    }

    pub async fn record_visual_ready(
        &self,
        optimizer_run_id: String,
        receipt: Value,
    ) -> Result<Value> {
        let mut run = self.get(optimizer_run_id.clone()).await?;
        let mut summary = run.summary.as_object().cloned().unwrap_or_default();
        summary.insert("visualReadyReceipt".into(), receipt.clone());
        summary.insert("waitingForViewer".into(), json!(false));
        run.summary = Value::Object(summary);
        self.persist_run(run).await?;
        Ok(receipt)
    }

    pub async fn await_visual_ready(
        &self,
        optimizer_run_id: String,
        timeout_ms: u64,
    ) -> Result<Value> {
        let deadline =
            tokio::time::Instant::now() + std::time::Duration::from_millis(timeout_ms.max(50));
        loop {
            let run = self.get(optimizer_run_id.clone()).await?;
            if let Some(receipt) = run.summary.get("visualReadyReceipt").cloned() {
                if !receipt.is_null() {
                    return Ok(receipt);
                }
            }
            if tokio::time::Instant::now() >= deadline {
                bail!("visual readiness receipt was not posted for `{optimizer_run_id}`");
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    pub async fn get_result(&self, optimizer_run_id: String) -> Result<Value> {
        let run = self.get(optimizer_run_id.clone()).await?;
        if let Some(existing) = run.summary.get("optimizerResult").cloned() {
            if cached_optimizer_result_is_authoritative(&run, &existing) {
                return Ok(existing);
            }
        }
        let result = materialize_optimizer_result(self, &run).await?;
        let mut stored = run.clone();
        let mut summary = stored.summary.as_object().cloned().unwrap_or_default();
        summary.insert("optimizerResult".into(), result.clone());
        stored.summary = Value::Object(summary);
        self.persist_run(stored).await?;
        Ok(result)
    }

    pub(super) async fn register_local_recipe(&self, run_id: String, cancel: watch::Sender<bool>) {
        self.local_recipes.lock().await.insert(run_id, cancel);
    }

    pub(super) async fn unregister_local_recipe(&self, run_id: &str) {
        self.local_recipes.lock().await.remove(run_id);
    }

    pub(super) async fn persist_run(&self, mut run: OptimizerRunRecord) -> Result<OptimizerRunRecord> {
        freeze_terminal_cursor(&mut run);
        let db = self.db.clone();
        let stored = run.clone();
        db.run_transaction(move |conn| {
            upsert_run(conn, &stored)?;
            Ok(stored)
        })
        .await
    }

    pub(crate) async fn attach_paid_compute_approval(
        &self,
        mut run: OptimizerRunRecord,
        approval_id: &str,
        max_cost_usd_micros: Option<u64>,
        max_rollouts: Option<u64>,
    ) -> Result<OptimizerRunRecord> {
        run.usage.extra.insert(
            "paidComputeApproval".into(),
            json!({
                "approvalId": approval_id,
                "cap": {
                    "maxCostUsdMicros": max_cost_usd_micros,
                    "maxRollouts": max_rollouts,
                },
                "receiptViolation": false,
            }),
        );
        self.persist_run(run).await
    }

    pub async fn list(&self, query: OptimizerQuery) -> Result<Vec<OptimizerRunRecord>> {
        let db = self.db.clone();
        db.run(move |conn| list_runs(conn, &query)).await
    }

    pub async fn get(&self, optimizer_run_id: String) -> Result<OptimizerRunRecord> {
        let db = self.db.clone();
        db.run(move |conn| load_run(conn, &optimizer_run_id)).await
    }

    pub async fn create(
        &self,
        request: OptimizerCreateRequest,
    ) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
        if let Some(fixture) = request.seed_fixture.clone() {
            return self
                .seed_fixture(&fixture, request.session_ref.clone())
                .await;
        }
        if let Some(path) = request.local_path.clone() {
            return self
                .import_local(super::models::OptimizerImportLocalRequest {
                    path,
                    session_ref: request.session_ref.clone(),
                    open_visual: request.open_visual,
                })
                .await;
        }
        let source = request
            .source
            .clone()
            .unwrap_or_else(|| "local".into())
            .to_ascii_lowercase();
        if source == "cloud" {
            return self.create_cloud(request).await;
        }
        let algorithm_id = super::normalize::normalize_algorithm_id(&request.algorithm_id);
        if algorithm_id.is_empty() || algorithm_id == "unknown" {
            bail!("algorithm_id is required");
        }
        let now = Utc::now().to_rfc3339();
        let id = request
            .id
            .clone()
            .unwrap_or_else(|| format!("opt_{}", Uuid::new_v4()));
        let capabilities = request
            .capabilities
            .clone()
            .unwrap_or_else(|| OptimizerCapabilities::for_algorithm(&algorithm_id));
        let run = OptimizerRunRecord {
            schema_version: OPTIMIZER_RUN_SCHEMA_VERSION.into(),
            id: id.clone(),
            algorithm_id: algorithm_id.clone(),
            algorithm_version: request.algorithm_version.clone(),
            status: "queued".into(),
            source: request.source.clone().unwrap_or_else(|| "local".into()),
            objective: request.objective.clone(),
            project_ref: request.project_ref.clone(),
            session_ref: request.session_ref.clone(),
            created_at: now.clone(),
            started_at: None,
            finished_at: None,
            cursor_seq: 0,
            capabilities,
            execution_bindings: request.execution_bindings.clone().unwrap_or_default(),
            input_refs: request.input_refs.clone().unwrap_or_default(),
            output_refs: vec![],
            visual_refs: vec![],
            summary: request.summary.clone().unwrap_or_else(|| json!({})),
            usage: OptimizerUsageSummary::default(),
            error: None,
        };
        let db = self.db.clone();
        let inserted = run.clone();
        let (mut run, event) = db
            .run_transaction(move |conn| {
                upsert_run(conn, &inserted)?;
                if let Some(session_ref) = inserted.session_ref.as_deref() {
                    insert_relationship(
                        conn,
                        &OptimizerRelationship {
                            from_kind: "optimizer".into(),
                            from_id: inserted.id.clone(),
                            edge: "started_from".into(),
                            to_kind: "session".into(),
                            to_id: session_ref.into(),
                            metadata: json!({}),
                        },
                    )?;
                }
                let event = append_event(
                    conn,
                    EventAppend {
                        event_id: None,
                        session_id: inserted.session_ref.clone(),
                        run_id: None,
                        source: EventSource::System,
                        kind: "optimizer.run.created".into(),
                        payload: json!({
                            "optimizerRunId": inserted.id,
                            "algorithmId": inserted.algorithm_id,
                            "status": inserted.status
                        }),
                        remote_sequence: None,
                        command_id: None,
                        created_at: None,
                    },
                )?;
                if let Some(session_ref) = inserted.session_ref.as_deref() {
                    crate::experiments::attach(
                        conn,
                        session_ref,
                        crate::experiments::MEMBER_OPTIMIZER,
                        &inserted.id,
                        &inserted.created_at,
                        &format!("{} {}", inserted.algorithm_id, inserted.id),
                    )?;
                }
                Ok((inserted, event))
            })
            .await?;
        if request.open_visual.unwrap_or(true) {
            run = self.open_visual(run.id.clone()).await?.0;
        }
        Ok((run, Some(event)))
    }

    pub async fn refresh(&self, optimizer_run_id: String) -> Result<OptimizerRunRecord> {
        if let Ok(run) = self.get(optimizer_run_id.clone()).await {
            if run.source == "cloud" {
                let (run, _) = self
                    .reconcile_cloud(super::models::OptimizerReconcileRequest {
                        optimizer_run_id: optimizer_run_id.clone(),
                        after_seq: Some(run.cursor_seq),
                        open_visual: Some(false),
                    })
                    .await?;
                return Ok(run);
            }
        }
        let mut run = self.get(optimizer_run_id.clone()).await?;
        if run.summary.get("recipeId").and_then(Value::as_str)
            == Some(super::hosted_gelo::HOSTED_GELO_CRAFTAX_RECIPE)
        {
            run = super::hosted_gelo::reconcile_persisted(self, &optimizer_run_id).await?;
        }
        if run.source == "local"
            && matches!(run.status.as_str(), "completed" | "failed" | "cancelled")
        {
            freeze_terminal_cursor(&mut run);
            let _frozen = self.persist_run(run).await?;
            run = super::recipes::reconcile_persisted(self, &optimizer_run_id).await?;
        }
        let slices = self.project_slices(&run.id, run.cursor_seq, None).await?;
        let db = self.db.clone();
        db.run(move |conn| {
            for slice in &slices {
                cache_slice(conn, slice)?;
            }
            Ok(())
        })
        .await?;
        self.get(optimizer_run_id).await
    }

    pub async fn events_after(
        &self,
        optimizer_run_id: String,
        after_seq: u64,
        limit: Option<i64>,
    ) -> Result<Vec<OptimizerEventEnvelope>> {
        let db = self.db.clone();
        let limit = limit.unwrap_or(500).clamp(1, 2000);
        db.run(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT payload_json FROM optimizer_events
                 WHERE optimizer_run_id = ?1 AND sequence_number > ?2
                 ORDER BY sequence_number ASC LIMIT ?3",
            )?;
            let rows = stmt
                .query_map(params![optimizer_run_id, after_seq as i64, limit], |row| {
                    row.get::<_, String>(0)
                })?;
            let mut out = Vec::new();
            for row in rows {
                out.push(serde_json::from_str(&row?)?);
            }
            Ok(out)
        })
        .await
    }

    pub async fn has_event_id(&self, optimizer_run_id: String, event_id: String) -> Result<bool> {
        let db = self.db.clone();
        db.run(move |conn| {
            conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM optimizer_events WHERE optimizer_run_id = ?1 AND event_id = ?2)",
                params![optimizer_run_id, event_id],
                |row| row.get::<_, bool>(0),
            )
            .map_err(Into::into)
        })
        .await
    }

    pub async fn append_events(
        &self,
        optimizer_run_id: String,
        events: Vec<OptimizerEventEnvelope>,
    ) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
        let db = self.db.clone();
        let result = db
            .run_transaction(move |conn| {
                let mut run = load_run(conn, &optimizer_run_id)?;
                for event in events {
                    if event.optimizer_run_id != run.id {
                        bail!("event optimizer_run_id mismatch");
                    }
                    if event.sequence_number <= run.cursor_seq {
                        continue;
                    }
                    insert_event(conn, &event)?;
                    apply_event_to_run(&mut run, &event);
                    run.cursor_seq = event.sequence_number;
                    freeze_terminal_cursor(&mut run);
                    upsert_cursor(conn, &run.id, run.cursor_seq, &event.occurred_at)?;
                }
                upsert_run(conn, &run)?;
                let projected = project_from_events(
                    &run,
                    &load_events_upto(conn, &run.id, run.cursor_seq)?,
                    None,
                )?;
                for slice in projected {
                    cache_slice(conn, &slice)?;
                }
                let event = append_event(
                    conn,
                    EventAppend {
                        event_id: None,
                        session_id: run.session_ref.clone(),
                        run_id: None,
                        source: EventSource::System,
                        kind: "optimizer.run.updated".into(),
                        payload: json!({
                            "optimizerRunId": run.id,
                            "status": run.status,
                            "cursorSeq": run.cursor_seq
                        }),
                        remote_sequence: None,
                        command_id: None,
                        created_at: None,
                    },
                )?;
                Ok((run, Some(event)))
            })
            .await?;
        if let Some(event) = &result.1 {
            let _ = self.events_tx.send(event.clone());
        }
        Ok(result)
    }

    pub async fn get_state(
        &self,
        optimizer_run_id: String,
        slice_id: String,
        at_seq: Option<u64>,
    ) -> Result<OptimizerStateSlice> {
        let run = self.get(optimizer_run_id.clone()).await?;
        let cursor = at_seq.unwrap_or(run.cursor_seq);
        if at_seq.is_none() {
            let db = self.db.clone();
            let sid = slice_id.clone();
            let rid = optimizer_run_id.clone();
            if let Some(cached) = db
                .run(move |conn| load_cached_slice(conn, &rid, &sid))
                .await?
            {
                if cached.cursor_seq == cursor {
                    return Ok(cached);
                }
            }
        }
        let slices = self
            .project_slices(&optimizer_run_id, cursor, Some(slice_id.as_str()))
            .await?;
        slices
            .into_iter()
            .find(|slice| slice.slice_id == slice_id)
            .ok_or_else(|| anyhow!("state slice not found: {slice_id}"))
    }

    pub async fn get_state_batch(
        &self,
        optimizer_run_id: String,
        slice_ids: Option<Vec<String>>,
        at_seq: Option<u64>,
    ) -> Result<Vec<OptimizerStateSlice>> {
        let run = self.get(optimizer_run_id.clone()).await?;
        let cursor = at_seq.unwrap_or(run.cursor_seq);
        let mut slices = self.project_slices(&optimizer_run_id, cursor, None).await?;
        if let Some(ids) = slice_ids {
            slices.retain(|slice| ids.iter().any(|id| id == &slice.slice_id));
        }
        Ok(slices)
    }

    pub async fn relationships(
        &self,
        optimizer_run_id: String,
    ) -> Result<Vec<OptimizerRelationship>> {
        let db = self.db.clone();
        db.run(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT from_kind, from_id, edge, to_kind, to_id, metadata_json
                 FROM optimizer_relationships
                 WHERE from_id = ?1 OR to_id = ?1
                 ORDER BY created_at ASC",
            )?;
            let rows = stmt.query_map(params![optimizer_run_id], |row| {
                Ok(OptimizerRelationship {
                    from_kind: row.get(0)?,
                    from_id: row.get(1)?,
                    edge: row.get(2)?,
                    to_kind: row.get(3)?,
                    to_id: row.get(4)?,
                    metadata: serde_json::from_str(&row.get::<_, String>(5)?).unwrap_or(json!({})),
                })
            })?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
        .await
    }

    pub async fn cancel(&self, id: String) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
        if let Some(cancel) = self.local_recipes.lock().await.get(&id).cloned() {
            cancel
                .send(true)
                .map_err(|_| anyhow!("local optimizer recipe is no longer running"))?;
            return self.command(id, "cancel", "cancelled").await;
        }
        if let Ok(run) = self.get(id.clone()).await {
            if run.source == "cloud" {
                if let Ok(client) = super::cloud::CloudOptimizerClient::from_config() {
                    let _ = client.cancel_run(&id).await;
                }
                return self
                    .reconcile_cloud(super::models::OptimizerReconcileRequest {
                        optimizer_run_id: id,
                        after_seq: None,
                        open_visual: Some(false),
                    })
                    .await;
            }
            if run.source == "hosted" && run.algorithm_id == "sft" {
                // A restarted desktop has no in-memory recipe worker, but cancellation
                // must still reach the canonical public SFT run. Do not fall back to
                // Optimizers-beta: it is an executor, not a Workshop control plane.
                super::sft_client::SftOptimizerClient::from_env()?
                    .cancel(&id)
                    .await?;
                return self.command(id, "cancel", "cancelled").await;
            }
        }
        self.command(id, "cancel", "cancelled").await
    }

    pub async fn pause(&self, id: String) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
        let run = self.get(id.clone()).await?;
        validate_control(&run, "pause")?;
        let is_eval = run.algorithm_id == super::eval_recipes::EVAL_ALGORITHM_ID;
        if is_eval {
            super::eval_recipes::set_paused(&id, true)?;
        }
        match self.command(id.clone(), "pause", "paused").await {
            Ok(result) => Ok(result),
            Err(error) => {
                if is_eval {
                    let _ = super::eval_recipes::set_paused(&id, false);
                }
                Err(error)
            }
        }
    }

    pub async fn resume(&self, id: String) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
        let run = self.get(id.clone()).await?;
        validate_control(&run, "resume")?;
        let is_eval = run.algorithm_id == super::eval_recipes::EVAL_ALGORITHM_ID;
        if is_eval {
            super::eval_recipes::set_paused(&id, false)?;
        }
        match self.command(id.clone(), "resume", "running").await {
            Ok(result) => Ok(result),
            Err(error) => {
                if is_eval {
                    let _ = super::eval_recipes::set_paused(&id, true);
                }
                Err(error)
            }
        }
    }

    pub async fn open_visual(
        &self,
        optimizer_run_id: String,
    ) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
        self.open_visual_in_session(optimizer_run_id, None).await
    }

    /// Create or reuse the run's primary visual and show it in the requested
    /// conversation. Showing a historical run must not rewrite the run's
    /// ownership; the session override belongs to this presentation event.
    pub async fn open_visual_in_session(
        &self,
        optimizer_run_id: String,
        session_ref: Option<String>,
    ) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
        let mut run = self.get(optimizer_run_id.clone()).await?;
        let presentation_session_ref = session_ref.or_else(|| run.session_ref.clone());
        let short_id = run
            .id
            .chars()
            .rev()
            .take(8)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        let title = format!(
            "{} · {}",
            run.objective
                .clone()
                .unwrap_or_else(|| algorithm_label(&run.algorithm_id).to_string()),
            short_id
        );
        let bindings = json!({
            "schemaVersion": VISUAL_BINDINGS_SCHEMA_VERSION,
            "slots": [{
                "slot": "optimizer_run",
                "kind": "optimizer_run",
                "source": run.id,
                "schema": OPTIMIZER_RUN_SCHEMA_VERSION
            }]
        });
        let existing = run
            .visual_refs
            .iter()
            .find(|r| r.kind == "visual")
            .map(|r| r.id.clone());
        // Public SFT and local eval are controlled outside the optional
        // Optimizers plugin, and their visuals are bundled with Workshop.
        // Neither may depend on plugin installation or advertised templates.
        //
        // That independence used to need an explicit bypass around a
        // negotiation step that consulted the plugin. It is now structural: no
        // run's visual consults the plugin, because template ids were never the
        // plugin's to grant. Keep it that way — reintroducing a capability
        // lookup here re-couples eval and SFT to plugin installation.
        let template_id = negotiate_visual_template(&run.algorithm_id);
        let template_digest = self.manager.status().await.digest;
        let visual = if let Some(visual_id) = existing {
            self.visuals.get(visual_id).await?
        } else {
            let (created, _) = self
                .visuals
                .create(VisualCreateRequest {
                    template_id: template_id.clone(),
                    title: Some(title),
                    bindings: Some(bindings),
                    id: None,
                    status: None,
                    renderer_kind: None,
                    session_id: presentation_session_ref.clone(),
                    message_id: None,
                    run_id: None,
                    trace_id: None,
                    parent_visual_id: None,
                    source_agent_id: None,
                    source_model: None,
                    content: None,
                    metadata: Some(json!({
                        "optimizerRunId": run.id,
                        "algorithmId": run.algorithm_id,
                        "templateDigest": template_digest
                    })),
                })
                .await?;
            run.visual_refs.push(OptimizerResourceRef {
                kind: "visual".into(),
                id: created.id.clone(),
                digest: template_digest.clone(),
                role: Some("primary".into()),
                title: Some(created.title.clone()),
                metadata: json!({
                    "templateId": template_id,
                    "templateDigest": template_digest
                }),
            });
            let visual_id = created.id.clone();
            let db = self.db.clone();
            let persisted = run.clone();
            db.run_transaction(move |conn| {
                upsert_run(conn, &persisted)?;
                insert_relationship(
                    conn,
                    &OptimizerRelationship {
                        from_kind: "optimizer".into(),
                        from_id: persisted.id.clone(),
                        edge: "visualized_by".into(),
                        to_kind: "visual".into(),
                        to_id: visual_id,
                        metadata: json!({}),
                    },
                )?;
                Ok(())
            })
            .await?;
            created
        };
        let (shown, show_event) = self
            .visuals
            .show(visual.id.clone(), presentation_session_ref.clone())
            .await?;
        let event = serde_json::from_value::<AppEvent>(show_event)
            .ok()
            .or(Some(AppEvent {
                schema_version: crate::storage::APP_EVENT_SCHEMA_VERSION.into(),
                sequence: 0,
                event_id: Uuid::new_v4().to_string(),
                session_id: presentation_session_ref,
                session_sequence: None,
                run_id: None,
                source: EventSource::System,
                kind: "optimizer.visual.opened".into(),
                payload: json!({
                    "optimizerRunId": run.id,
                    "visualId": shown.id,
                    "templateId": shown.template_id
                }),
                remote_sequence: None,
                command_id: None,
                created_at: Utc::now().to_rfc3339(),
            }));
        Ok((self.get(run.id).await?, event))
    }

    pub async fn seed_fixture(
        &self,
        fixture: &str,
        session_ref: Option<String>,
    ) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
        let (run, events) = match fixture {
            "gepa" | "gepa_events" => gepa_fixture(session_ref),
            "go-ex" | "goex" | "goex_events" => goex_fixture(session_ref),
            "sft" | "sft_events" => sft_fixture(session_ref),
            other => bail!("unknown optimizer fixture: {other}"),
        };
        let db = self.db.clone();
        let seed = run.clone();
        db.run_transaction(move |conn| {
            upsert_run(conn, &seed)?;
            upsert_cursor(conn, &seed.id, 0, &seed.created_at)?;
            Ok(())
        })
        .await?;
        let (run, event) = self.append_events(run.id.clone(), events).await?;
        let (run, visual_event) = self.open_visual(run.id).await?;
        Ok((run, event.or(visual_event)))
    }

    /// Import a local OSS GEPA event feed or optimizers-beta/GELO workspace.
    pub async fn import_local(
        &self,
        request: super::models::OptimizerImportLocalRequest,
    ) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
        let imported = super::local::import_local_path(&request.path)?;
        let algorithm_id = super::normalize::normalize_algorithm_id(&imported.algorithm_id);
        let events =
            super::normalize::normalize_events(&imported.events, &imported.run_id, &algorithm_id);
        if events.is_empty() {
            bail!("no normalizable events found at {}", request.path);
        }
        let now = Utc::now().to_rfc3339();
        let run = OptimizerRunRecord {
            schema_version: OPTIMIZER_RUN_SCHEMA_VERSION.into(),
            id: imported.run_id.clone(),
            algorithm_id: algorithm_id.clone(),
            algorithm_version: None,
            status: "running".into(),
            source: "local".into(),
            objective: imported.objective.clone(),
            project_ref: None,
            session_ref: request.session_ref.clone(),
            created_at: now.clone(),
            started_at: Some(now.clone()),
            finished_at: None,
            cursor_seq: 0,
            capabilities: OptimizerCapabilities::for_algorithm(&algorithm_id),
            execution_bindings: vec![],
            input_refs: vec![OptimizerResourceRef {
                kind: "local_path".into(),
                id: imported.source_path.display().to_string(),
                digest: None,
                role: Some("event_feed".into()),
                title: Some("Local optimizer workspace".into()),
                metadata: json!({}),
            }],
            output_refs: vec![],
            visual_refs: vec![],
            summary: json!({ "importedFrom": imported.source_path.display().to_string() }),
            usage: OptimizerUsageSummary::default(),
            error: None,
        };
        let db = self.db.clone();
        let seed = run.clone();
        db.run_transaction(move |conn| {
            upsert_run(conn, &seed)?;
            upsert_cursor(conn, &seed.id, 0, &seed.created_at)?;
            if let Some(session_ref) = seed.session_ref.as_deref() {
                insert_relationship(
                    conn,
                    &OptimizerRelationship {
                        from_kind: "optimizer".into(),
                        from_id: seed.id.clone(),
                        edge: "started_from".into(),
                        to_kind: "session".into(),
                        to_id: session_ref.into(),
                        metadata: json!({}),
                    },
                )?;
                crate::experiments::attach(
                    conn,
                    session_ref,
                    crate::experiments::MEMBER_OPTIMIZER,
                    &seed.id,
                    &seed.created_at,
                    &format!("{} {}", seed.algorithm_id, seed.id),
                )?;
            }
            Ok(())
        })
        .await?;
        let (mut run, event) = self.append_events(run.id.clone(), events).await?;
        if algorithm_id == "go-ex" {
            if let Ok(client) = super::hosted_client::HostedOptimizerClient::from_env() {
                if let Ok(batch) = client
                    .state_batch(
                        &run.id,
                        &[
                            "board",
                            "themes",
                            "candidates",
                            "frontier",
                            "data-engine",
                            "agents",
                        ],
                    )
                    .await
                {
                    super::hosted_gelo::append_state_batch(self, &run.id, "succeeded", batch)
                        .await?;
                    run = self.get(run.id.clone()).await?;
                }
            }
        }
        if request.open_visual.unwrap_or(true) {
            let (run, visual_event) = self.open_visual(run.id).await?;
            return Ok((run, event.or(visual_event)));
        }
        Ok((run, event))
    }

    /// Mirror + backfill a hosted Synth Cloud run (GEPA or optimizers-beta GELO).
    pub async fn reconcile_cloud(
        &self,
        request: super::models::OptimizerReconcileRequest,
    ) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
        let client = super::cloud::CloudOptimizerClient::from_config()?;
        let remote = client.get_run(&request.optimizer_run_id).await?;
        let (id, algorithm_id, status, remote_cursor, objective) =
            super::normalize::cloud_run_to_mirror(&remote)
                .ok_or_else(|| anyhow!("invalid cloud optimizer run payload"))?;
        let existing = self.get(id.clone()).await.ok();
        let now = Utc::now().to_rfc3339();
        let mut run = existing.unwrap_or_else(|| OptimizerRunRecord {
            schema_version: OPTIMIZER_RUN_SCHEMA_VERSION.into(),
            id: id.clone(),
            algorithm_id: algorithm_id.clone(),
            algorithm_version: None,
            status: status.clone(),
            source: "cloud".into(),
            objective: objective
                .clone()
                .or_else(|| Some(format!("cloud {algorithm_id}"))),
            project_ref: remote
                .get("project_id")
                .and_then(Value::as_str)
                .map(str::to_string),
            session_ref: None,
            created_at: remote
                .get("created_at")
                .or_else(|| remote.get("submitted_at"))
                .and_then(Value::as_str)
                .unwrap_or(&now)
                .to_string(),
            started_at: remote
                .get("created_at")
                .and_then(Value::as_str)
                .map(str::to_string),
            finished_at: remote
                .get("terminal_at")
                .and_then(Value::as_str)
                .map(str::to_string),
            cursor_seq: 0,
            capabilities: OptimizerCapabilities::for_algorithm(&algorithm_id),
            execution_bindings: vec![],
            input_refs: vec![],
            output_refs: vec![],
            visual_refs: vec![],
            summary: json!({ "cloud": true }),
            usage: OptimizerUsageSummary::default(),
            error: remote.get("error").cloned(),
        });
        run.status = status;
        run.algorithm_id = algorithm_id.clone();
        run.source = "cloud".into();
        if let Some(objective) = objective {
            run.objective = Some(objective);
        }
        let cloud_seq = run
            .summary
            .get("cloudEventSeq")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let after_seq = request.after_seq.unwrap_or(cloud_seq);
        let db = self.db.clone();
        let seed = run.clone();
        db.run_transaction(move |conn| {
            upsert_run(conn, &seed)?;
            upsert_cursor(conn, &seed.id, seed.cursor_seq, &Utc::now().to_rfc3339())?;
            Ok(())
        })
        .await?;

        let raw_events = client
            .events_after(&id, after_seq, Some(2000))
            .await
            .unwrap_or_default();
        let next_cloud_seq = raw_events
            .iter()
            .filter_map(|event| {
                event
                    .get("seq")
                    .or_else(|| event.get("_seq"))
                    .and_then(Value::as_u64)
            })
            .max()
            .unwrap_or(after_seq);
        if algorithm_id == "sft" {
            for raw in &raw_events {
                if super::normalize::normalize_event(raw, &id, &algorithm_id).is_none() {
                    bail!("hosted sft event omitted sequence_number or was not optimizer_event.v1");
                }
            }
        }
        let events = super::normalize::normalize_events(&raw_events, &id, &algorithm_id);
        let (mut run, event) = if events.is_empty() {
            if algorithm_id != "sft" {
                run.cursor_seq = run.cursor_seq.max(remote_cursor);
            }
            persist_cloud_event_seq(&mut run, next_cloud_seq);
            let db = self.db.clone();
            let persisted = run.clone();
            db.run(move |conn| upsert_run(conn, &persisted)).await?;
            (run, None)
        } else {
            let (mut run, event) = self.append_events(id.clone(), events).await?;
            persist_cloud_event_seq(&mut run, next_cloud_seq);
            let db = self.db.clone();
            let persisted = run.clone();
            db.run(move |conn| upsert_run(conn, &persisted)).await?;
            (run, event)
        };
        if request.open_visual.unwrap_or(false) || run.visual_refs.is_empty() {
            let opened = self.open_visual(run.id.clone()).await?;
            run = opened.0;
            return Ok((run, event.or(opened.1)));
        }
        Ok((run, event))
    }

    pub async fn list_cloud(
        &self,
        algorithm: Option<String>,
        status: Option<String>,
        limit: Option<i64>,
    ) -> Result<Vec<Value>> {
        let client = super::cloud::CloudOptimizerClient::from_config()?;
        client
            .list_runs(algorithm.as_deref(), status.as_deref(), limit)
            .await
    }

    async fn create_cloud(
        &self,
        request: OptimizerCreateRequest,
    ) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
        let algorithm_id = super::normalize::normalize_algorithm_id(&request.algorithm_id);
        let config = request
            .cloud_config
            .clone()
            .ok_or_else(|| anyhow!("cloud create requires cloudConfig"))?;
        let client = super::cloud::CloudOptimizerClient::from_config()?;
        let submitted = client
            .create_run(
                &algorithm_id,
                config,
                request.project_ref.as_deref(),
                request.id.as_deref(),
            )
            .await?;
        let run_id = submitted
            .get("run_id")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("cloud create response missing run_id"))?
            .to_string();
        self.reconcile_cloud(super::models::OptimizerReconcileRequest {
            optimizer_run_id: run_id,
            after_seq: Some(0),
            open_visual: request.open_visual.or(Some(true)),
        })
        .await
    }

    async fn command(
        &self,
        optimizer_run_id: String,
        command: &str,
        next_status: &str,
    ) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
        let command = command.to_string();
        let next_status = next_status.to_string();
        let db = self.db.clone();
        db.run_transaction(move |conn| {
            let mut run = load_run(conn, &optimizer_run_id)?;
            validate_control(&run, &command)?;
            run.status = next_status;
            if command == "resume" && run.started_at.is_none() {
                run.started_at = Some(Utc::now().to_rfc3339());
            }
            if command == "cancel" {
                run.finished_at = Some(Utc::now().to_rfc3339());
            }
            upsert_run(conn, &run)?;
            let event = append_event(
                conn,
                EventAppend {
                    event_id: None,
                    session_id: run.session_ref.clone(),
                    run_id: None,
                    source: EventSource::System,
                    kind: format!("optimizer.run.{command}"),
                    payload: json!({ "optimizerRunId": run.id, "status": run.status }),
                    remote_sequence: None,
                    command_id: None,
                    created_at: None,
                },
            )?;
            Ok((run, Some(event)))
        })
        .await
    }

    async fn project_slices(
        &self,
        optimizer_run_id: &str,
        at_seq: u64,
        only: Option<&str>,
    ) -> Result<Vec<OptimizerStateSlice>> {
        let run_id = optimizer_run_id.to_string();
        let only = only.map(str::to_string);
        let db = self.db.clone();
        db.run(move |conn| {
            let run = load_run(conn, &run_id)?;
            let events = load_events_upto(conn, &run_id, at_seq)?;
            let mut slices = project_from_events(&run, &events, None)?;
            if let Some(only) = only {
                slices.retain(|slice| slice.slice_id == only);
            }
            Ok(slices)
        })
        .await
    }
}

fn persist_cloud_event_seq(run: &mut OptimizerRunRecord, cloud_event_seq: u64) {
    let mut summary = run.summary.as_object().cloned().unwrap_or_default();
    summary.insert("cloudEventSeq".into(), json!(cloud_event_seq));
    run.summary = Value::Object(summary);
}

fn algorithm_label(algorithm_id: &str) -> &'static str {
    match algorithm_id {
        "gepa" => "GEPA",
        "go-ex" => "GELO",
        "sft" => "SFT",
        "eval" => "Eval",
        id if id == "dag" || id.starts_with("dag.") => "DAG",
        _ => "Optimizer",
    }
}

pub(in crate::optimizers) fn primary_visual_template(algorithm_id: &str) -> &'static str {
    match algorithm_id {
        "sft" => "optimizer.sft.live.v1",
        "gepa" => "optimizer.gepa.live.v1",
        "eval" => "optimizer.eval.live.v1",
        id if id == "dag" || id.starts_with("dag.") => "optimizer.dag.live.v1",
        _ => "optimizer.run.v1",
    }
}

/// Cross-check that the runtime actually claims the algorithm about to run.
///
/// This replaces an intersection against `compatibleTemplateIds`, which could
/// not fail informatively: template ids are Desktop vocabulary that the plugin
/// only knew because Desktop's own install payload told it, so the check
/// compared a host constant against a round-trip of that same constant. The
/// algorithm list is a fact the runtime owns, so comparing against it is real.
fn require_advertised_algorithm(advertised: &Value, algorithm_id: &str) -> Result<()> {
    let algorithms = advertised
        .get("algorithms")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
        .ok_or_else(|| {
            anyhow!(
                "optimizer capabilities advertise no algorithms; the sidecar must complete a \
                 capability handshake before a run is opened"
            )
        })?;
    // Namespaced ids (`dag.foo`) are served by their root algorithm.
    let root = algorithm_id.split('.').next().unwrap_or(algorithm_id);
    if algorithms
        .iter()
        .filter_map(Value::as_str)
        .any(|advertised| advertised == algorithm_id || advertised == root)
    {
        return Ok(());
    }
    bail!("optimizer runtime does not advertise algorithm `{algorithm_id}`")
}

/// Which visual Desktop renders for a run. Deliberately does not consult the
/// runtime: template ids are host vocabulary, and picking a visual is not a
/// capability decision. Gating rendering on the handshake also breaks runs the
/// managed sidecar does not serve at all — hosted SFT among them, since the
/// real plugin advertises only `gepa`. Whether a runtime can *execute* an
/// algorithm is enforced at the paid gate by `require_advertised_algorithm`.
fn negotiate_visual_template(algorithm_id: &str) -> String {
    primary_visual_template(algorithm_id).to_owned()
}

fn freeze_terminal_cursor(run: &mut OptimizerRunRecord) {
    if !matches!(run.status.as_str(), "completed" | "failed" | "cancelled") {
        return;
    }
    let usage = serde_json::to_value(&run.usage).unwrap_or(Value::Null);
    match run.summary.as_object_mut() {
        Some(summary) => {
            if !summary.contains_key("terminalCursor") {
                summary.insert("terminalCursor".into(), json!(run.cursor_seq));
            }
            if !summary.contains_key("terminalUsage") {
                summary.insert("terminalUsage".into(), usage);
            }
        }
        None => {
            run.summary = json!({
                "terminalCursor": run.cursor_seq,
                "terminalUsage": usage,
            });
        }
    }
    project_visual_evidence(run);
}

/// Write-once four-state visual verdict at terminal. Partial/failed never
/// changes run status — the turn completes and the agent reports the state.
fn project_visual_evidence(run: &mut OptimizerRunRecord) {
    if !matches!(run.status.as_str(), "completed" | "failed" | "cancelled") {
        return;
    }
    if run
        .summary
        .get("visualEvidence")
        .is_some_and(|value| value.is_object())
    {
        return;
    }
    let ready = run
        .summary
        .get("visualReadyReceipt")
        .is_some_and(|value| !value.is_null());
    let reviewed = run
        .summary
        .pointer("/visualReadyReceipt/qualityGate")
        .is_some_and(|value| !value.is_null())
        || run
            .summary
            .get("authoringReviews")
            .and_then(Value::as_array)
            .is_some_and(|reviews| !reviews.is_empty());
    let has_visual = run.visual_refs.iter().any(|item| item.kind == "visual");
    let render_failed = run.status == "failed"
        && run.error.as_ref().is_some_and(|error| {
            error
                .to_string()
                .to_lowercase()
                .contains("visual")
        });
    let (state, detail) = if ready {
        ("ready", "visual readiness receipt posted")
    } else if reviewed {
        (
            "reviewed",
            "reviews recorded without a readiness receipt",
        )
    } else if render_failed || !has_visual {
        (
            "failed",
            "no usable product visual; this does not block task completion",
        )
    } else {
        (
            "partial",
            "product visual exists but is not certified; this does not block task completion",
        )
    };
    let evidence = json!({
        "state": state,
        "decidedAt": chrono::Utc::now().to_rfc3339(),
        "detail": detail
    });
    match run.summary.as_object_mut() {
        Some(summary) => {
            summary.insert("visualEvidence".into(), evidence);
        }
        None => {
            run.summary = json!({ "visualEvidence": evidence });
        }
    }
}

fn project_waiting_for_viewer(run: &mut OptimizerRunRecord) {
    let ready = run
        .summary
        .get("visualReadyReceipt")
        .is_some_and(|value| !value.is_null());
    let waiting = !ready && run.status == "waiting_for_viewer";
    match run.summary.as_object_mut() {
        Some(summary) => {
            summary.insert("waitingForViewer".into(), json!(waiting));
        }
        None => {
            run.summary = json!({ "waitingForViewer": waiting });
        }
    }
}

fn candidate_id_of(value: &Value) -> Option<&str> {
    value
        .get("id")
        .or_else(|| value.get("candidateId"))
        .or_else(|| value.get("candidate_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
}

fn usage_authority_block(run: &OptimizerRunRecord, manifest: Option<&Value>) -> Value {
    let frozen = run.summary.get("terminalUsage").cloned();
    let live = serde_json::to_value(&run.usage).unwrap_or(Value::Null);
    let ledger = frozen.clone().unwrap_or(live.clone());
    let manifest_usage = manifest.and_then(|value| {
        value
            .get("usage")
            .cloned()
            .or_else(|| value.get("cost_authority").cloned())
            .or_else(|| {
                let cost = value.get("cost_usd").cloned()?;
                Some(json!({ "costUsd": cost }))
            })
    });
    let ledger_cost = ledger
        .get("costUsd")
        .or_else(|| ledger.get("cost_usd"))
        .and_then(Value::as_f64)
        .or(run.usage.cost_usd);
    let manifest_cost = manifest_usage
        .as_ref()
        .and_then(|value| value.get("costUsd").or_else(|| value.get("cost_usd")))
        .and_then(Value::as_f64);
    let (reconciliation_status, divergence) = match (&manifest_usage, ledger_cost, manifest_cost) {
        (None, _, _) => ("manifest_absent", Value::Null),
        (_, Some(ledger), Some(manifest)) if (ledger - manifest).abs() > 1e-9 => (
            "divergent",
            json!({ "ledgerCostUsd": ledger, "manifestCostUsd": manifest }),
        ),
        _ => ("aligned", Value::Null),
    };
    json!({
        "ledger": ledger,
        "manifest": manifest_usage,
        "authority": "manifest",
        "reconciliationStatus": reconciliation_status,
        "divergence": divergence,
        "enrichmentLedger": if frozen.is_some() { live } else { Value::Null },
    })
}

fn cached_optimizer_result_is_authoritative(run: &OptimizerRunRecord, existing: &Value) -> bool {
    match (
        run.algorithm_id.as_str(),
        existing.get("schemaVersion").and_then(Value::as_str),
    ) {
        ("sft", Some("sft_result.v1")) => true,
        ("gepa", Some("optimizer_result.v1")) => existing
            .pointer("/metrics/heldoutMeasurement")
            .is_some_and(|value| !value.is_null()),
        ("sft", _) => false,
        (_, Some("optimizer_result.v1")) => true,
        _ => false,
    }
}

async fn materialize_optimizer_result(
    service: &OptimizerService,
    run: &OptimizerRunRecord,
) -> Result<Value> {
    match run.algorithm_id.as_str() {
        "sft" => materialize_sft_result(run),
        _ => materialize_gepa_result(service, run).await,
    }
}

/// Lane D owns typed `SftRunResult` materialization. Workshop must not project
/// SFT through GEPA-shaped `best_candidate.json`.
fn materialize_sft_result(run: &OptimizerRunRecord) -> Result<Value> {
    let terminal_cursor = run
        .summary
        .get("terminalCursor")
        .and_then(Value::as_u64)
        .unwrap_or(run.cursor_seq);
    Ok(json!({
        "schemaVersion": "sft_result.v1",
        "optimizerRunId": run.id,
        "algorithmId": "sft",
        "status": run.status,
        "finalCursor": terminal_cursor,
        "enrichmentCursor": run.cursor_seq,
        "usage": usage_authority_block(run, None),
        "pending": true,
        "error": {
            "code": "sft_result_materialization_pending",
            "message": "Typed SftRunResult materialization is owned by Lane D. Workshop will not invent a GEPA-shaped result for this SFT run."
        }
    }))
}

async fn materialize_gepa_result(
    service: &OptimizerService,
    run: &OptimizerRunRecord,
) -> Result<Value> {
    let run_dir = run
        .summary
        .get("runDirectory")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from);
    let candidate_path = run_dir.as_ref().map(|dir| dir.join("best_candidate.json"));
    let manifest_path = run_dir.as_ref().map(|dir| dir.join("result_manifest.json"));
    let candidate_raw = candidate_path
        .as_ref()
        .and_then(|path| std::fs::read(path).ok());
    let manifest_raw = manifest_path
        .as_ref()
        .and_then(|path| std::fs::read(path).ok());
    let manifest = manifest_raw
        .as_ref()
        .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok());
    let mut artifact_refs = Vec::new();
    if let Some(bytes) = candidate_raw.as_ref() {
        if let Ok(digest) = service.visuals.content().put_bytes("blobs", bytes) {
            artifact_refs.push(json!({
                "kind": "content",
                "id": digest,
                "role": "best_candidate"
            }));
        }
    }
    if let Some(bytes) = manifest_raw.as_ref() {
        if let Ok(digest) = service.visuals.content().put_bytes("blobs", bytes) {
            artifact_refs.push(json!({
                "kind": "content",
                "id": digest,
                "role": "result_manifest"
            }));
        }
    }
    let prompt = candidate_raw.as_ref().and_then(|bytes| {
        let value: Value = serde_json::from_slice(bytes).ok()?;
        value
            .get("prompt")
            .or_else(|| value.pointer("/values/prompt"))
            .or_else(|| value.pointer("/payload/prompt"))
            .or_else(|| value.pointer("/lever_bundle/values/prompt"))
            .or_else(|| value.get("stage2_system"))
            .or_else(|| value.pointer("/values/stage2_system"))
            .or_else(|| value.pointer("/payload/stage2_system"))
            .or_else(|| value.pointer("/lever_bundle/values/stage2_system"))
            .or_else(|| value.get("react_system_prompt"))
            .or_else(|| value.pointer("/values/react_system_prompt"))
            .or_else(|| value.pointer("/payload/react_system_prompt"))
            .or_else(|| value.pointer("/lever_bundle/values/react_system_prompt"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    });
    let candidate_id = candidate_raw.as_ref().and_then(|bytes| {
        let value: Value = serde_json::from_slice(bytes).ok()?;
        value
            .get("id")
            .or_else(|| value.get("candidate_id"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    });
    let parent_id = candidate_raw.as_ref().and_then(|bytes| {
        let value: Value = serde_json::from_slice(bytes).ok()?;
        value
            .get("parent_id")
            .or_else(|| value.get("parentId"))
            .and_then(Value::as_str)
            .map(str::to_owned)
    });
    let materialized_digest = prompt.as_ref().map(|text| {
        use sha2::{Digest, Sha256};
        format!("sha256:{:x}", Sha256::digest(text.as_bytes()))
    });
    if run.status == "completed"
        && prompt
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none()
    {
        bail!("completed GEPA result omitted a materialized prompt");
    }
    let mut selected_candidate = json!({});
    if let Some(id) = candidate_id {
        selected_candidate["id"] = json!(id);
    }
    if let Some(parent) = parent_id {
        selected_candidate["parentId"] = json!(parent);
    }
    if let Some(prompt) = prompt {
        selected_candidate["materializedValues"] = json!({ "prompt": prompt });
    }
    if let Some(digest) = materialized_digest {
        selected_candidate["materializedDigest"] = json!(digest);
    }
    selected_candidate["frontierMember"] = json!(true);
    let selection = run
        .summary
        .get("selection")
        .cloned()
        .filter(|value| !value.is_null())
        .or_else(|| {
            manifest.as_ref().and_then(|value| {
                value
                    .get("selection")
                    .or_else(|| value.pointer("/best_candidate/acceptance_metadata/selection"))
                    .cloned()
                    .or_else(|| {
                        let best = value.get("best_candidate")?;
                        let candidate_id = best.get("candidate_id")?.clone();
                        let parent_id = best.get("parent_id").cloned().unwrap_or(Value::Null);
                        Some(json!({
                            "candidateId": candidate_id,
                            "parentId": parent_id,
                            "accepted": !parent_id.is_null(),
                            "acceptanceScore": best.get("acceptance_score").cloned().unwrap_or(Value::Null),
                            "minibatchReward": best.get("minibatch_reward").cloned().unwrap_or(Value::Null)
                        }))
                    })
            })
        })
        .unwrap_or(Value::Null);
    let heldout = run
        .summary
        .get("heldout")
        .cloned()
        .filter(|value| !value.is_null())
        .or_else(|| {
            manifest.as_ref().and_then(|value| {
                value
                    .pointer("/best_candidate/heldout_reward")
                    .filter(|value| !value.is_null())
                    .map(|score| json!({"score": score, "split": "heldout"}))
            })
        })
        .unwrap_or(Value::Null);
    let mut heldout_best_candidate = selected_candidate.clone();
    if let Some(o7) = manifest
        .as_ref()
        .and_then(|value| value.get("heldout_best_candidate"))
        .filter(|value| value.is_object())
    {
        heldout_best_candidate = o7.clone();
    }
    let mut optimization_selected_candidate = manifest
        .as_ref()
        .and_then(|value| value.get("optimization_selected_candidate"))
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or_else(|| {
            let mut selected = json!({});
            if let Some(id) = selection.get("candidateId").cloned() {
                selected["id"] = id;
            }
            if let Some(parent) = selection.get("parentId").cloned() {
                selected["parentId"] = parent;
            }
            if let Some(score) = selection
                .get("acceptanceScore")
                .or_else(|| selection.get("score"))
                .cloned()
            {
                selected["score"] = score;
            }
            selected
        });
    if candidate_id_of(&optimization_selected_candidate).is_none()
        && candidate_id_of(&heldout_best_candidate).is_some()
        && selection.get("candidateId").is_none()
    {
        // Detection-only: when O-6/O-7 are absent and acceptance metadata has
        // no distinct id, the materialized file is the only identity we have.
        optimization_selected_candidate = heldout_best_candidate.clone();
    }
    if optimization_selected_candidate
        .get("materializedValues")
        .is_none()
        && candidate_id_of(&optimization_selected_candidate)
            == candidate_id_of(&heldout_best_candidate)
    {
        if let Some(values) = heldout_best_candidate.get("materializedValues").cloned() {
            optimization_selected_candidate["materializedValues"] = values;
        }
        if let Some(digest) = heldout_best_candidate.get("materializedDigest").cloned() {
            optimization_selected_candidate["materializedDigest"] = digest;
        }
    }
    let opt_id = candidate_id_of(&optimization_selected_candidate).map(str::to_owned);
    let heldout_id = candidate_id_of(&heldout_best_candidate).map(str::to_owned);
    let identity_consistent = match (opt_id.as_deref(), heldout_id.as_deref()) {
        (Some(left), Some(right)) => left == right,
        (None, None) => true,
        _ => false,
    };
    let mut selected_for_compat = optimization_selected_candidate.clone();
    selected_for_compat["selectionCriterion"] = json!("optimization_selected");
    let terminal_cursor = run
        .summary
        .get("terminalCursor")
        .and_then(Value::as_u64)
        .unwrap_or(run.cursor_seq);
    Ok(json!({
        "schemaVersion": "optimizer_result.v1",
        "optimizerRunId": run.id,
        "algorithmId": run.algorithm_id,
        "status": run.status,
        "finalCursor": terminal_cursor,
        "enrichmentCursor": run.cursor_seq,
        "selectedCandidate": selected_for_compat,
        "optimizationSelectedCandidate": optimization_selected_candidate,
        "heldoutBestCandidate": heldout_best_candidate,
        "identityConsistent": identity_consistent,
        "metrics": {
            "selection": selection,
            "heldoutMeasurement": heldout
        },
        "usage": usage_authority_block(run, manifest.as_ref()),
        "artifactRefs": artifact_refs,
        "completionReceiptId": format!("optimizer_completion_{}", run.id)
    }))
}

fn list_runs(conn: &Connection, query: &OptimizerQuery) -> Result<Vec<OptimizerRunRecord>> {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let offset = query.offset.unwrap_or(0).max(0);
    let mut sql = String::from("SELECT payload_json FROM optimizer_runs WHERE 1=1");
    let mut binds: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(status) = query.status.as_deref() {
        sql.push_str(" AND status = ?");
        binds.push(Box::new(status.to_string()));
    }
    if let Some(algorithm_id) = query.algorithm_id.as_deref() {
        sql.push_str(" AND algorithm_id = ?");
        binds.push(Box::new(algorithm_id.to_string()));
    }
    if let Some(source) = query.source.as_deref() {
        sql.push_str(" AND source = ?");
        binds.push(Box::new(source.to_string()));
    }
    if let Some(session_ref) = query.session_ref.as_deref() {
        sql.push_str(" AND session_ref = ?");
        binds.push(Box::new(session_ref.to_string()));
    }
    if let Some(search) = query.search.as_deref() {
        sql.push_str(" AND (id LIKE ? OR objective LIKE ? OR algorithm_id LIKE ?)");
        let pattern = format!("%{search}%");
        binds.push(Box::new(pattern.clone()));
        binds.push(Box::new(pattern.clone()));
        binds.push(Box::new(pattern));
    }
    sql.push_str(" ORDER BY updated_at DESC LIMIT ? OFFSET ?");
    binds.push(Box::new(limit));
    binds.push(Box::new(offset));
    let mut stmt = conn.prepare(&sql)?;
    let params_refs: Vec<&dyn rusqlite::types::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
    let rows = stmt.query_map(params_refs.as_slice(), |row| row.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        let mut run: OptimizerRunRecord =
            serde_json::from_str(&row?).context("decode optimizer run")?;
        project_waiting_for_viewer(&mut run);
        out.push(run);
    }
    Ok(out)
}

fn load_run(conn: &Connection, optimizer_run_id: &str) -> Result<OptimizerRunRecord> {
    let payload: String = conn
        .query_row(
            "SELECT payload_json FROM optimizer_runs WHERE id = ?1",
            params![optimizer_run_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| anyhow!("optimizer run not found"))?;
    let mut run: OptimizerRunRecord = serde_json::from_str(&payload)?;
    project_waiting_for_viewer(&mut run);
    Ok(run)
}

fn upsert_run(conn: &Connection, run: &OptimizerRunRecord) -> Result<()> {
    let payload = serde_json::to_string(run)?;
    conn.execute(
        "INSERT INTO optimizer_runs(
            id, algorithm_id, algorithm_version, status, source, objective,
            project_ref, session_ref, created_at, started_at, finished_at,
            cursor_seq, capabilities_json, bindings_json, input_refs_json,
            output_refs_json, visual_refs_json, summary_json, usage_json,
            error_json, payload_json, updated_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22)
         ON CONFLICT(id) DO UPDATE SET
            algorithm_id=excluded.algorithm_id,
            algorithm_version=excluded.algorithm_version,
            status=excluded.status,
            source=excluded.source,
            objective=excluded.objective,
            project_ref=excluded.project_ref,
            session_ref=excluded.session_ref,
            started_at=excluded.started_at,
            finished_at=excluded.finished_at,
            cursor_seq=excluded.cursor_seq,
            capabilities_json=excluded.capabilities_json,
            bindings_json=excluded.bindings_json,
            input_refs_json=excluded.input_refs_json,
            output_refs_json=excluded.output_refs_json,
            visual_refs_json=excluded.visual_refs_json,
            summary_json=excluded.summary_json,
            usage_json=excluded.usage_json,
            error_json=excluded.error_json,
            payload_json=excluded.payload_json,
            updated_at=excluded.updated_at",
        params![
            run.id,
            run.algorithm_id,
            run.algorithm_version,
            run.status,
            run.source,
            run.objective,
            run.project_ref,
            run.session_ref,
            run.created_at,
            run.started_at,
            run.finished_at,
            run.cursor_seq as i64,
            serde_json::to_string(&run.capabilities)?,
            serde_json::to_string(&run.execution_bindings)?,
            serde_json::to_string(&run.input_refs)?,
            serde_json::to_string(&run.output_refs)?,
            serde_json::to_string(&run.visual_refs)?,
            serde_json::to_string(&run.summary)?,
            serde_json::to_string(&run.usage)?,
            run.error.as_ref().map(serde_json::to_string).transpose()?,
            payload,
            Utc::now().to_rfc3339(),
        ],
    )?;
    Ok(())
}

fn upsert_cursor(conn: &Connection, run_id: &str, cursor_seq: u64, updated_at: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO optimizer_event_cursors(optimizer_run_id, cursor_seq, updated_at)
         VALUES (?1, ?2, ?3)
         ON CONFLICT(optimizer_run_id) DO UPDATE SET
            cursor_seq=excluded.cursor_seq,
            updated_at=excluded.updated_at",
        params![run_id, cursor_seq as i64, updated_at],
    )?;
    Ok(())
}

fn insert_event(conn: &Connection, event: &OptimizerEventEnvelope) -> Result<()> {
    let payload = serde_json::to_string(event)?;
    conn.execute(
        "INSERT OR IGNORE INTO optimizer_events(
            event_id, optimizer_run_id, sequence_number, event_type,
            algorithm_id, occurred_at, payload_json
         ) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            event
                .event_id
                .clone()
                .unwrap_or_else(|| format!("{}:{}", event.optimizer_run_id, event.sequence_number)),
            event.optimizer_run_id,
            event.sequence_number as i64,
            event.event_type,
            event.algorithm_id,
            event.occurred_at,
            payload
        ],
    )?;
    Ok(())
}

fn insert_relationship(conn: &Connection, rel: &OptimizerRelationship) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO optimizer_relationships(
            from_kind, from_id, edge, to_kind, to_id, metadata_json, created_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            rel.from_kind,
            rel.from_id,
            rel.edge,
            rel.to_kind,
            rel.to_id,
            serde_json::to_string(&rel.metadata)?,
            Utc::now().to_rfc3339()
        ],
    )?;
    Ok(())
}

fn cache_slice(conn: &Connection, slice: &OptimizerStateSlice) -> Result<()> {
    conn.execute(
        "INSERT INTO optimizer_cached_slices(
            optimizer_run_id, slice_id, cursor_seq, updated_at, payload_json
         ) VALUES (?1,?2,?3,?4,?5)
         ON CONFLICT(optimizer_run_id, slice_id) DO UPDATE SET
            cursor_seq=excluded.cursor_seq,
            updated_at=excluded.updated_at,
            payload_json=excluded.payload_json",
        params![
            slice.run_id,
            slice.slice_id,
            slice.cursor_seq as i64,
            slice.updated_at,
            serde_json::to_string(slice)?
        ],
    )?;
    Ok(())
}

fn load_cached_slice(
    conn: &Connection,
    run_id: &str,
    slice_id: &str,
) -> Result<Option<OptimizerStateSlice>> {
    let payload: Option<String> = conn
        .query_row(
            "SELECT payload_json FROM optimizer_cached_slices
             WHERE optimizer_run_id = ?1 AND slice_id = ?2",
            params![run_id, slice_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(payload.map(|raw| serde_json::from_str(&raw)).transpose()?)
}

fn load_events_upto(
    conn: &Connection,
    run_id: &str,
    at_seq: u64,
) -> Result<Vec<OptimizerEventEnvelope>> {
    let mut stmt = conn.prepare(
        "SELECT payload_json FROM optimizer_events
         WHERE optimizer_run_id = ?1 AND sequence_number <= ?2
         ORDER BY sequence_number ASC",
    )?;
    let rows = stmt.query_map(params![run_id, at_seq as i64], |row| {
        row.get::<_, String>(0)
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(serde_json::from_str(&row?)?);
    }
    Ok(out)
}

fn apply_event_to_run(run: &mut OptimizerRunRecord, event: &OptimizerEventEnvelope) {
    // Training-job status is not the optimizer run status. `sft.training.completed`
    // / `sft.training.status` emit `succeeded` while promotion and
    // `optimizer.run.completed` are still outstanding.
    let copy_delta_status = !event.event_type.starts_with("sft.training.")
        || matches!(
            event.event_type.as_str(),
            "sft.training.queued" | "sft.training.started"
        );
    if copy_delta_status {
        if let Some(status) = event
            .snapshot
            .as_ref()
            .and_then(|s| s.get("status"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                event
                    .delta
                    .get("status")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
        {
            run.status = status;
        }
    }
    if event.event_type.ends_with(".started") || event.event_type.ends_with(".created") {
        if run.started_at.is_none() {
            run.started_at = Some(event.occurred_at.clone());
        }
        if run.status == "queued" {
            run.status = "running".into();
        }
    }
    if let Some(terminal_status) = optimizer_terminal_status(&event.event_type) {
        run.finished_at = Some(event.occurred_at.clone());
        if terminal_status == "failed" {
            run.status = "failed".into();
        } else if terminal_status == "cancelled" {
            run.status = "cancelled".into();
        } else if run.status != "cancelled" {
            run.status = "completed".into();
        }
    }
    if let Some(usage) = &event.usage_delta {
        apply_reported_cost(&mut run.usage, usage);
        if let Some(v) = usage.get("prompt_tokens").and_then(Value::as_u64) {
            run.usage.prompt_tokens += v;
        }
        if let Some(v) = usage.get("completion_tokens").and_then(Value::as_u64) {
            run.usage.completion_tokens += v;
        }
        if let Some(v) = usage.get("rollouts").and_then(Value::as_u64) {
            run.usage.rollouts += v;
        }
        if let Some(v) = usage.get("wall_time_ms").and_then(Value::as_u64) {
            run.usage.wall_time_ms += v;
        }
    }
    update_paid_compute_violation(run);
    if let Some(snapshot) = &event.snapshot {
        if let Some(summary) = snapshot.get("summary") {
            run.summary = summary.clone();
        }
        if let Some(best) = snapshot
            .get("best_score")
            .or_else(|| snapshot.get("bestScore"))
        {
            if let Some(obj) = run.summary.as_object_mut() {
                obj.insert("bestScore".into(), best.clone());
            } else {
                run.summary = json!({ "bestScore": best });
            }
        }
    }
    if let Some(error) = &event.error {
        run.error = Some(error.clone());
    }
}

fn update_paid_compute_violation(run: &mut OptimizerRunRecord) {
    let Some(approval) = run.usage.extra.get("paidComputeApproval").cloned() else {
        return;
    };
    let max_rollouts = approval.pointer("/cap/maxRollouts").and_then(Value::as_u64);
    let max_cost_micros = approval
        .pointer("/cap/maxCostUsdMicros")
        .and_then(Value::as_u64);
    let rollouts_exceeded = max_rollouts.is_some_and(|cap| run.usage.rollouts > cap);
    let cost_exceeded = match (max_cost_micros, run.usage.cost_usd) {
        (Some(cap), Some(cost)) => cost * 1_000_000.0 > cap as f64,
        _ => false,
    };
    if rollouts_exceeded || cost_exceeded {
        if let Some(object) = run
            .usage
            .extra
            .get_mut("paidComputeApproval")
            .and_then(Value::as_object_mut)
        {
            object.insert("receiptViolation".into(), Value::Bool(true));
            object.insert(
                "violationReason".into(),
                Value::String(
                    if rollouts_exceeded {
                        "rollout_cap_exceeded"
                    } else {
                        "cost_cap_exceeded"
                    }
                    .into(),
                ),
            );
        }
    }
}

/// Apply the any-unknown-to-null cost rule to an incrementally persisted run.
/// `extra.costTelemetryComplete` retains the poisoned state across reloads so
/// a later known receipt cannot turn an earlier unknown charge into a partial
/// confident sum.
fn apply_reported_cost(usage: &mut OptimizerUsageSummary, delta: &Map<String, Value>) {
    let raw = delta.get("cost_usd").or_else(|| delta.get("costUsd"));
    let reports_tokens = [
        "prompt_tokens",
        "promptTokens",
        "completion_tokens",
        "completionTokens",
    ]
    .iter()
    .any(|key| delta.contains_key(*key));
    let Some(raw) = raw else {
        if reports_tokens {
            usage
                .extra
                .insert("costTelemetryComplete".into(), Value::Bool(false));
            usage.cost_usd = None;
        }
        return;
    };
    let was_complete = usage
        .extra
        .get("costTelemetryComplete")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let reported = raw
        .as_f64()
        .filter(|value| value.is_finite() && *value >= 0.0);
    let complete = was_complete && reported.is_some();
    usage
        .extra
        .insert("costTelemetryComplete".into(), Value::Bool(complete));
    usage.cost_usd = if complete {
        Some(usage.cost_usd.unwrap_or(0.0) + reported.unwrap_or(0.0))
    } else {
        None
    };
}

fn optimizer_terminal_status(event_type: &str) -> Option<&'static str> {
    match event_type {
        "optimizer.run.completed" | "gepa.run.finished" | "goex.run_finished" | "run.completed" => {
            Some("completed")
        }
        "optimizer.run.failed" | "run.failed" => Some("failed"),
        "optimizer.run.cancelled" | "run.cancelled" => Some("cancelled"),
        _ => None,
    }
}

/// optimizers-beta state/batch returns named slice envelopes. Keep the
/// envelope at the transport boundary, but project only its durable `data`
/// payload into Workshop's algorithm state slices.
fn hosted_state_slice_data<'a>(slices: &'a Value, name: &str) -> Option<&'a Value> {
    let slice = slices.get(name)?;
    slice.get("data").or(Some(slice))
}

/// A GEPA candidate is identified by `delta.candidate_id`, falling back to the
/// optional `item.id`. Registration events carry only the delta.
fn candidate_identity(event: &OptimizerEventEnvelope) -> Option<String> {
    event
        .delta
        .get("candidate_id")
        .and_then(Value::as_str)
        .or_else(|| {
            event
                .item
                .as_ref()
                .and_then(|item| item.get("id"))
                .and_then(Value::as_str)
        })
        .map(str::to_string)
}

/// `frontier.updated` reports the winning candidate and its coverage rather
/// than a cell grid. Project that into a single Pareto cell. Anything the
/// sidecar did not report stays absent — never a zero.
fn frontier_cell_from_delta(event: &OptimizerEventEnvelope) -> Option<Value> {
    let source: &Map<String, Value> = event
        .snapshot
        .as_ref()
        .filter(|snapshot| snapshot.contains_key("best_candidate_id"))
        .unwrap_or(&event.delta);
    let candidate_id = source.get("best_candidate_id").and_then(Value::as_str)?;
    let mut cell = Map::new();
    cell.insert("candidateId".into(), json!(candidate_id));
    cell.insert("sequence".into(), json!(event.sequence_number));
    for (field, key) in [
        ("best_train_reward", "trainReward"),
        ("candidate_count", "candidateCount"),
        ("changed_candidate_id", "changedCandidateId"),
        ("coverage", "coverage"),
        ("coverage_semantics", "coverageSemantics"),
        ("generation", "generation"),
    ] {
        if let Some(value) = source.get(field) {
            cell.insert(key.into(), value.clone());
        }
    }
    Some(Value::Object(cell))
}

fn project_from_events(
    run: &OptimizerRunRecord,
    events: &[OptimizerEventEnvelope],
    _only: Option<&str>,
) -> Result<Vec<OptimizerStateSlice>> {
    let cursor = events.last().map(|e| e.sequence_number).unwrap_or(0);
    let updated_at = events
        .last()
        .map(|e| e.occurred_at.clone())
        .unwrap_or_else(|| run.created_at.clone());

    let mut timeline = Vec::new();
    let mut candidates: BTreeMap<String, Value> = BTreeMap::new();
    let mut frontier = Vec::new();
    let mut reflections = Vec::new();
    let mut artifacts = Vec::new();
    let mut logs = Vec::new();
    let mut themes = Vec::new();
    let mut board = json!({ "phase": "idle", "tick": 0 });
    let mut goex_candidates = json!({ "candidates": [] });
    let mut goex_frontier = json!({ "candidate_frontier": [] });
    let mut goex_data_engine = json!({ "rollout_evidence": {} });
    let mut goex_agents = json!({});
    let mut checkpoints: Vec<Value> = Vec::new();
    let mut curves = json!({ "steps": [], "epochs": [], "trainLoss": [], "validationLoss": [], "learningRate": [] });
    let mut dataset = json!({ "splits": {} });
    let mut compute = json!({ "provider": null });
    let mut examples = Vec::new();
    let mut checkpoint_evals = Vec::new();
    let mut eval_trials: BTreeMap<String, Value> = BTreeMap::new();
    let mut eval_scorecards: BTreeMap<String, Value> = BTreeMap::new();
    let mut eval_runtime = json!({
        "parallelism": Value::Null,
        "globalCapacity": Value::Null,
        "cancelling": false,
        "plannedTrials": 0
    });
    let mut eval_evidence = json!({
        "manifestDigest": Value::Null,
        "candidateSetId": Value::Null,
        "seedLedger": Value::Null,
        "selection": Value::Null,
        "evidenceDir": Value::Null,
        "artifacts": []
    });
    let mut usage = OptimizerUsageSummary::default();

    for event in events {
        timeline.push(json!({
            "sequence": event.sequence_number,
            "type": event.event_type,
            "occurredAt": event.occurred_at,
            "level": event.level,
            "itemId": event.item.as_ref().and_then(|i| i.get("id")).cloned()
        }));
        logs.push(json!({
            "sequence": event.sequence_number,
            "type": event.event_type,
            "occurredAt": event.occurred_at,
            "message": event.delta.get("message").cloned().unwrap_or(Value::Null),
            "raw": event.raw
        }));
        for artifact in &event.artifact_refs {
            artifacts.push(artifact.clone());
        }
        if let Some(usage_delta) = &event.usage_delta {
            apply_reported_cost(&mut usage, usage_delta);
            if let Some(v) = usage_delta.get("prompt_tokens").and_then(Value::as_u64) {
                usage.prompt_tokens += v;
            }
            if let Some(v) = usage_delta.get("completion_tokens").and_then(Value::as_u64) {
                usage.completion_tokens += v;
            }
            if let Some(v) = usage_delta.get("rollouts").and_then(Value::as_u64) {
                usage.rollouts += v;
            }
            if let Some(v) = usage_delta.get("wall_time_ms").and_then(Value::as_u64) {
                usage.wall_time_ms += v;
            }
        }
        match event.event_type.as_str() {
            "candidate.registered"
            | "candidate.accepted"
            | "candidate.rejected"
            | "candidate.evaluated"
            | "candidate.minibatch_evaluated"
            | "candidate.full_train_evaluated"
            | "gepa.candidate.updated" => {
                // The sidecar identifies a candidate on the delta
                // (`candidate_id`); `item` is optional and absent on the
                // registration events that create the candidate in the first
                // place. Keying off `item` alone left the slice empty.
                if let Some(id) = candidate_identity(event) {
                    let entry = candidates
                        .entry(id)
                        .or_insert_with(|| json!({ "id": Value::Null }));
                    if let Some(obj) = entry.as_object_mut() {
                        obj.insert("sequence".into(), json!(event.sequence_number));
                        if let Some(item) = &event.item {
                            if let Some(fields) = item.as_object() {
                                for (k, v) in fields {
                                    obj.insert(k.clone(), v.clone());
                                }
                            }
                        }
                        for (k, v) in &event.delta {
                            obj.insert(k.clone(), v.clone());
                        }
                        if let Some(snapshot) = &event.snapshot {
                            for (k, v) in snapshot {
                                obj.insert(k.clone(), v.clone());
                            }
                        }
                    }
                }
            }
            "frontier.updated" | "frontier.snapshot" | "gepa.frontier.updated" => {
                // A snapshot with explicit `cells` wins. Otherwise project the
                // sidecar's frontier delta into one cell keyed by the best
                // candidate, so the Pareto view is not empty on a real run.
                if let Some(cells) = event
                    .snapshot
                    .as_ref()
                    .and_then(|s| s.get("cells"))
                    .or_else(|| event.delta.get("cells"))
                    .and_then(Value::as_array)
                {
                    frontier = cells.clone();
                } else if let Some(cell) = frontier_cell_from_delta(event) {
                    frontier = vec![cell];
                }
            }
            "gepa.reflection" | "proposer.completed" => {
                reflections.push(json!({
                    "sequence": event.sequence_number,
                    "occurredAt": event.occurred_at,
                    "delta": event.delta,
                    "snapshot": event.snapshot
                }));
            }
            "go-ex.board.updated" | "goex.board.updated" | "goex.tick_transition" => {
                board = event
                    .snapshot
                    .as_ref()
                    .map(|s| Value::Object(s.clone()))
                    .unwrap_or_else(|| Value::Object(event.delta.clone()));
            }
            "go-ex.theme.updated" | "goex.theme.updated" => {
                themes.push(json!({
                    "sequence": event.sequence_number,
                    "delta": event.delta,
                    "snapshot": event.snapshot
                }));
            }
            "goex.state.batch.updated" => {
                if let Some(slices) = event.snapshot.as_ref().and_then(|s| s.get("slices")) {
                    if let Some(value) = hosted_state_slice_data(slices, "board") {
                        board = value.clone();
                    }
                    if let Some(value) = hosted_state_slice_data(slices, "themes") {
                        themes = value
                            .get("themes")
                            .and_then(Value::as_array)
                            .cloned()
                            .or_else(|| value.as_array().cloned())
                            .unwrap_or_default();
                    }
                    if let Some(value) = hosted_state_slice_data(slices, "candidates") {
                        goex_candidates = value.clone();
                    }
                    if let Some(value) = hosted_state_slice_data(slices, "frontier") {
                        goex_frontier = value.clone();
                    }
                    if let Some(value) = hosted_state_slice_data(slices, "data-engine") {
                        goex_data_engine = value.clone();
                    }
                    if let Some(value) = hosted_state_slice_data(slices, "agents") {
                        goex_agents = value.clone();
                    }
                }
            }
            "sft.checkpoint.created" | "sft.checkpoint.ready" => {
                if let Some(item) = &event.item {
                    let id = item.get("id").and_then(Value::as_str).unwrap_or("");
                    let mut found = false;
                    for checkpoint in checkpoints.iter_mut() {
                        if checkpoint.get("id").and_then(Value::as_str) == Some(id) {
                            if let Some(obj) = checkpoint.as_object_mut() {
                                obj.insert(
                                    "status".into(),
                                    item.get("status").cloned().unwrap_or(json!("ready")),
                                );
                                if event.event_type == "sft.checkpoint.ready" {
                                    obj.insert("ready".into(), json!(true));
                                }
                            }
                            found = true;
                        }
                    }
                    if !found {
                        checkpoints.push(item.clone());
                    }
                }
            }
            "sft.checkpoint.promoted" => {
                if let Some(item) = &event.item {
                    let id = item.get("id").and_then(Value::as_str).unwrap_or("");
                    let mut found = false;
                    for checkpoint in checkpoints.iter_mut() {
                        if checkpoint.get("id").and_then(Value::as_str) == Some(id) {
                            if let Some(obj) = checkpoint.as_object_mut() {
                                obj.insert("status".into(), json!("promoted"));
                                if let Some(raw) = obj.get_mut("raw").and_then(Value::as_object_mut)
                                {
                                    raw.insert("promoted".into(), json!(true));
                                }
                            }
                            found = true;
                        }
                    }
                    if !found {
                        checkpoints.push(item.clone());
                    }
                }
            }
            "sft.step.metrics" | "sft.training.metrics" => {
                if let Some(obj) = curves.as_object_mut() {
                    push_curve(obj, "steps", event.delta.get("step"));
                    push_curve(obj, "epochs", event.delta.get("epoch"));
                    push_curve(
                        obj,
                        "trainLoss",
                        event
                            .delta
                            .get("train_loss")
                            .or_else(|| event.delta.get("trainLoss")),
                    );
                    push_curve(
                        obj,
                        "validationLoss",
                        event
                            .delta
                            .get("validation_loss")
                            .or_else(|| event.delta.get("validationLoss")),
                    );
                    push_curve(
                        obj,
                        "learningRate",
                        event
                            .delta
                            .get("learning_rate")
                            .or_else(|| event.delta.get("learningRate")),
                    );
                }
            }
            "sft.dataset.validated" => {
                dataset = event
                    .snapshot
                    .as_ref()
                    .map(|s| Value::Object(s.clone()))
                    .unwrap_or_else(|| Value::Object(event.delta.clone()));
            }
            "sft.compute.updated" => {
                compute = event
                    .snapshot
                    .as_ref()
                    .map(|s| Value::Object(s.clone()))
                    .unwrap_or_else(|| Value::Object(event.delta.clone()));
            }
            "sft.checkpoint_eval.completed"
            | "sft.heldout_eval.completed"
            | "sft.checkpoint_evaluation.completed"
            | "sft.checkpoint_evaluation.allocated" => {
                checkpoint_evals.push(json!({
                    "sequence": event.sequence_number,
                    "delta": event.delta,
                    "snapshot": event.snapshot,
                    "item": event.item,
                    "role": event.delta.get("role").cloned().unwrap_or(json!("selection")),
                    "measurementOnly": event.delta.get("measurementOnly").cloned().unwrap_or(json!(false))
                }));
            }
            "sft.examples.updated" => {
                if let Some(rows) = event
                    .snapshot
                    .as_ref()
                    .and_then(|s| s.get("examples"))
                    .and_then(Value::as_array)
                {
                    examples = rows.clone();
                }
            }
            "sft.model.materialized" => {
                if let Some(item) = &event.item {
                    artifacts.push(item.clone());
                }
            }
            "eval.run.planned" => {
                if let Some(snapshot) = &event.snapshot {
                    for (from, to) in [
                        ("parallelism", "parallelism"),
                        ("global_capacity", "globalCapacity"),
                        ("planned_trials", "plannedTrials"),
                    ] {
                        if let (Some(value), Some(target)) =
                            (snapshot.get(from), eval_runtime.as_object_mut())
                        {
                            target.insert(to.into(), value.clone());
                        }
                    }
                    if let Some(target) = eval_evidence.as_object_mut() {
                        if let Some(value) = snapshot.get("manifest_digest") {
                            target.insert("manifestDigest".into(), value.clone());
                        }
                        if let Some(value) = snapshot.get("candidate_set_id") {
                            target.insert("candidateSetId".into(), value.clone());
                        }
                        if let Some(value) = snapshot.get("candidates") {
                            target.insert("candidates".into(), value.clone());
                        }
                    }
                }
            }
            "eval.seed_ledger.sealed" => {
                if let (Some(snapshot), Some(target)) =
                    (&event.snapshot, eval_evidence.as_object_mut())
                {
                    if let Some(value) = snapshot.get("seedLedger") {
                        target.insert("seedLedger".into(), value.clone());
                    }
                }
            }
            "eval.trial.queued" | "eval.trial.started" => {
                if let Some(id) = event.delta.get("trial_id").and_then(Value::as_str) {
                    let row = eval_trials
                        .entry(id.to_string())
                        .or_insert_with(|| json!({"id": id}));
                    if let Some(object) = row.as_object_mut() {
                        for key in ["candidate_id", "seed", "scenario", "stage"] {
                            if let Some(value) = event.delta.get(key) {
                                object.insert(key.into(), value.clone());
                            }
                        }
                        object.insert(
                            "status".into(),
                            json!(if event.event_type.ends_with("started") {
                                "running"
                            } else {
                                "queued"
                            }),
                        );
                    }
                }
            }
            "eval.trial.terminal" => {
                if let Some(item) = &event.item {
                    if let Some(id) = item.get("id").and_then(Value::as_str) {
                        eval_trials.insert(id.to_string(), item.clone());
                    }
                }
            }
            "eval.candidate.scored" => {
                if let Some(item) = &event.item {
                    let candidate = item.get("id").and_then(Value::as_str).unwrap_or("");
                    let stage = item.get("stage").and_then(Value::as_str).unwrap_or("");
                    eval_scorecards.insert(format!("{stage}:{candidate}"), item.clone());
                }
            }
            "eval.selection.completed" => {
                if let (Some(snapshot), Some(target)) =
                    (&event.snapshot, eval_evidence.as_object_mut())
                {
                    if let Some(value) = snapshot.get("selection") {
                        target.insert("selection".into(), value.clone());
                    }
                }
            }
            _ => {}
        }
    }

    if let Some(target) = eval_evidence.as_object_mut() {
        if let Some(dir) = events
            .iter()
            .rev()
            .find_map(|event| event.delta.get("evidenceDir").cloned())
        {
            target.insert("evidenceDir".into(), dir);
        }
        target.insert("artifacts".into(), json!(artifacts));
    }
    if let Some(target) = eval_runtime.as_object_mut() {
        let counts = |status: &str| {
            eval_trials
                .values()
                .filter(|row| row.get("status").and_then(Value::as_str) == Some(status))
                .count()
        };
        target.insert("queued".into(), json!(counts("queued")));
        target.insert("running".into(), json!(counts("running")));
        target.insert("evaluated".into(), json!(counts("evaluated")));
        target.insert(
            "failed".into(),
            json!(eval_trials.len() - counts("queued") - counts("running") - counts("evaluated")),
        );
        // A trial holds exactly one semaphore lease while it runs, so the
        // running count is the run's share of the machine-wide ceiling.
        target.insert("leasesHeld".into(), json!(counts("running")));
        target.insert(
            "cancelling".into(),
            json!(events
                .iter()
                .any(|event| event.event_type == "optimizer.run.cancelled")),
        );
    }

    let mut projected_run = run.clone();
    projected_run.usage = OptimizerUsageSummary::default();
    projected_run.finished_at = None;
    for event in events {
        apply_event_to_run(&mut projected_run, event);
    }
    projected_run.cursor_seq = cursor;
    projected_run.usage = usage.clone();

    let mk = |slice_id: &str, projection: &str, data: Value| OptimizerStateSlice {
        schema_version: OPTIMIZER_STATE_SLICE_SCHEMA_VERSION.into(),
        projection_schema_version: projection.into(),
        run_id: run.id.clone(),
        algorithm_id: run.algorithm_id.clone(),
        slice_id: slice_id.into(),
        cursor_seq: cursor,
        updated_at: updated_at.clone(),
        data,
    };

    let mut slices = vec![
        mk(
            "run.summary",
            "run.summary.v1",
            json!({
                "id": projected_run.id,
                "algorithmId": projected_run.algorithm_id,
                "status": projected_run.status,
                "objective": projected_run.objective,
                "source": projected_run.source,
                "capabilities": projected_run.capabilities,
                "summary": projected_run.summary,
                "startedAt": projected_run.started_at,
                "finishedAt": projected_run.finished_at,
                "cursorSeq": projected_run.cursor_seq
            }),
        ),
        mk(
            "run.timeline",
            "run.timeline.v1",
            json!({ "events": timeline }),
        ),
        mk("run.usage", "run.usage.v1", serde_json::to_value(&usage)?),
        mk("run.logs", "run.logs.v1", json!({ "entries": logs })),
        mk(
            "run.artifacts",
            "run.artifacts.v1",
            json!({ "artifacts": artifacts }),
        ),
        mk(
            "run.execution",
            "run.execution.v1",
            json!({ "bindings": projected_run.execution_bindings }),
        ),
    ];

    match run.algorithm_id.as_str() {
        "gepa" => {
            slices.push(mk(
                "gepa.candidates",
                "gepa.candidates.v1",
                json!({ "candidates": candidates.values().cloned().collect::<Vec<_>>() }),
            ));
            slices.push(mk(
                "gepa.frontier",
                "gepa.frontier.v1",
                json!({ "cells": frontier }),
            ));
            slices.push(mk(
                "gepa.reflections",
                "gepa.reflections.v1",
                json!({ "entries": reflections }),
            ));
        }
        "go-ex" => {
            slices.push(mk("go-ex.board", "go-ex.board.v1", board));
            slices.push(mk(
                "go-ex.themes",
                "go-ex.themes.v1",
                json!({ "themes": themes }),
            ));
            slices.push(mk(
                "go-ex.candidates",
                "go-ex.candidates.v1",
                goex_candidates,
            ));
            slices.push(mk("go-ex.frontier", "go-ex.frontier.v1", goex_frontier));
            slices.push(mk(
                "go-ex.data_engine",
                "go-ex.data_engine.v1",
                goex_data_engine,
            ));
            slices.push(mk("go-ex.agents", "go-ex.agents.v1", goex_agents));
        }
        "sft" => {
            slices.push(mk("sft.training_curves", "sft.training_curves.v1", curves));
            slices.push(mk(
                "sft.checkpoints",
                "sft.checkpoints.v1",
                json!({ "checkpoints": checkpoints }),
            ));
            slices.push(mk(
                "sft.checkpoint_evaluations",
                "sft.checkpoint_evaluations.v1",
                json!({ "evaluations": checkpoint_evals }),
            ));
            slices.push(mk("sft.dataset", "sft.dataset.v1", dataset));
            slices.push(mk("sft.compute", "sft.compute.v1", compute));
            slices.push(mk(
                "sft.examples",
                "sft.examples.v1",
                json!({ "examples": examples }),
            ));
        }
        "eval" => {
            slices.push(mk("eval.runtime", "eval.runtime.v1", eval_runtime));
            slices.push(mk(
                "eval.trials",
                "eval.trials.v1",
                json!({ "trials": eval_trials.values().cloned().collect::<Vec<_>>() }),
            ));
            slices.push(mk(
                "eval.scorecard",
                "eval.scorecard.v1",
                json!({ "candidates": eval_scorecards.values().cloned().collect::<Vec<_>>() }),
            ));
            slices.push(mk("eval.evidence", "eval.evidence.v1", eval_evidence));
        }
        _ => {}
    }
    Ok(slices)
}

fn push_curve(obj: &mut Map<String, Value>, key: &str, value: Option<&Value>) {
    let Some(value) = value else { return };
    if value.is_null() {
        return;
    }
    let entry = obj.entry(key.to_string()).or_insert_with(|| json!([]));
    if let Some(arr) = entry.as_array_mut() {
        arr.push(value.clone());
    }
}

fn gepa_fixture(session_ref: Option<String>) -> (OptimizerRunRecord, Vec<OptimizerEventEnvelope>) {
    let run = OptimizerRunRecord {
        schema_version: OPTIMIZER_RUN_SCHEMA_VERSION.into(),
        id: GEPA_FIXTURE_ID.into(),
        algorithm_id: "gepa".into(),
        algorithm_version: Some("1.0.0".into()),
        status: "queued".into(),
        source: "local".into(),
        objective: Some("banking77 prompt · maximize macro-F1".into()),
        project_ref: None,
        session_ref,
        created_at: "2026-08-09T15:00:00Z".into(),
        started_at: None,
        finished_at: None,
        cursor_seq: 0,
        capabilities: OptimizerCapabilities::for_algorithm("gepa"),
        execution_bindings: vec![],
        input_refs: vec![OptimizerResourceRef {
            kind: "dataset".into(),
            id: "banking77@pinned".into(),
            digest: Some("sha256:banking77demo".into()),
            role: Some("train".into()),
            title: Some("Banking77".into()),
            metadata: json!({}),
        }],
        output_refs: vec![],
        visual_refs: vec![],
        summary: json!({}),
        usage: OptimizerUsageSummary::default(),
        error: None,
    };
    let mut events = vec![
        evt(
            "gepa.run.started",
            1,
            "gepa",
            GEPA_FIXTURE_ID,
            "2026-08-09T15:00:01Z",
            json!({"status":"running"}),
            None,
            usage(0.0, 0, 0, 0, 0),
        ),
        evt(
            "proposer.started",
            2,
            "gepa",
            GEPA_FIXTURE_ID,
            "2026-08-09T15:00:05Z",
            json!({"generation":1}),
            None,
            None,
        ),
        evt(
            "candidate.evaluated",
            3,
            "gepa",
            GEPA_FIXTURE_ID,
            "2026-08-09T15:00:20Z",
            json!({"train_reward":0.71,"rank":2}),
            Some(item(
                "candidate",
                "cand_seed",
                "evaluated",
                json!({"parentId":null}),
            )),
            usage(0.12, 800, 200, 8, 15_000),
        ),
        evt(
            "candidate.accepted",
            4,
            "gepa",
            GEPA_FIXTURE_ID,
            "2026-08-09T15:00:25Z",
            json!({"train_reward":0.71,"reason":"seed"}),
            Some(item(
                "candidate",
                "cand_seed",
                "accepted",
                json!({"score":0.71,"costUsd":0.12,"rollouts":8}),
            )),
            None,
        ),
        evt(
            "proposer.completed",
            5,
            "gepa",
            GEPA_FIXTURE_ID,
            "2026-08-09T15:01:00Z",
            json!({"generation":1,"proposal_count":2,"message":"mutate instruction clarity"}),
            None,
            usage(0.4, 2400, 900, 0, 35_000),
        ),
        evt(
            "candidate.full_train_evaluated",
            6,
            "gepa",
            GEPA_FIXTURE_ID,
            "2026-08-09T15:01:40Z",
            json!({"train_reward":0.82,"best_train_reward":0.82}),
            Some(item(
                "candidate",
                "cand_m1",
                "evaluated",
                json!({"parentId":"cand_seed","score":0.82,"costUsd":0.31,"rollouts":16,"delta":0.11}),
            )),
            usage(0.31, 1600, 500, 16, 40_000),
        ),
        evt(
            "candidate.accepted",
            7,
            "gepa",
            GEPA_FIXTURE_ID,
            "2026-08-09T15:01:42Z",
            json!({"train_reward":0.82,"reason":"dominates parent"}),
            Some(item(
                "candidate",
                "cand_m1",
                "accepted",
                json!({"parentId":"cand_seed","score":0.82,"costUsd":0.31,"rollouts":16,"rank":1}),
            )),
            None,
        ),
        evt(
            "frontier.updated",
            8,
            "gepa",
            GEPA_FIXTURE_ID,
            "2026-08-09T15:01:43Z",
            json!({}),
            None,
            None,
        ),
        evt(
            "gepa.reflection",
            9,
            "gepa",
            GEPA_FIXTURE_ID,
            "2026-08-09T15:01:50Z",
            json!({"message":"Clarify fee vs balance intents; keep short system preamble."}),
            None,
            None,
        ),
        evt(
            "gepa.run.finished",
            10,
            "gepa",
            GEPA_FIXTURE_ID,
            "2026-08-09T15:02:10Z",
            json!({"status":"completed"}),
            None,
            None,
        ),
    ];
    if let Some(event) = events.get_mut(7) {
        event.snapshot = Some(map_from(json!({
            "cells": [
                {"candidateId":"cand_seed","quality":0.71,"costUsd":0.12,"rollouts":8},
                {"candidateId":"cand_m1","quality":0.82,"costUsd":0.31,"rollouts":16,"accent":true}
            ],
            "bestScore": 0.82,
            "status": "running",
            "summary": {"bestScore": 0.82, "iteration": 1}
        })));
    }
    if let Some(event) = events.get_mut(9) {
        event.snapshot = Some(map_from(json!({
            "status": "completed",
            "bestScore": 0.82,
            "summary": {"bestScore": 0.82, "iteration": 1, "accepted": 2}
        })));
    }
    (run, events)
}

fn goex_fixture(session_ref: Option<String>) -> (OptimizerRunRecord, Vec<OptimizerEventEnvelope>) {
    let run = OptimizerRunRecord {
        schema_version: OPTIMIZER_RUN_SCHEMA_VERSION.into(),
        id: GOEX_FIXTURE_ID.into(),
        algorithm_id: "go-ex".into(),
        algorithm_version: Some("0.9.0".into()),
        status: "queued".into(),
        source: "cloud".into(),
        objective: Some("craftax themes · saturate near-misses".into()),
        project_ref: None,
        session_ref,
        created_at: "2026-08-09T15:10:00Z".into(),
        started_at: None,
        finished_at: None,
        cursor_seq: 0,
        capabilities: OptimizerCapabilities::for_algorithm("go-ex"),
        execution_bindings: vec![super::models::OptimizerExecutionBinding {
            kind: "local_slot".into(),
            id: "local-mac-01".into(),
            label: Some("local-mac-01".into()),
            status: Some("leased".into()),
            metadata: json!({ "leaseId": "lease_demo" }),
        }],
        input_refs: vec![],
        output_refs: vec![],
        visual_refs: vec![],
        summary: json!({}),
        usage: OptimizerUsageSummary::default(),
        error: None,
    };
    let mut events = vec![
        evt(
            "optimizer.run.created",
            1,
            "go-ex",
            GOEX_FIXTURE_ID,
            "2026-08-09T15:10:01Z",
            json!({"status":"starting"}),
            None,
            None,
        ),
        evt(
            "go-ex.board.updated",
            2,
            "go-ex",
            GOEX_FIXTURE_ID,
            "2026-08-09T15:10:05Z",
            json!({}),
            None,
            usage(0.5, 0, 0, 4, 5_000),
        ),
        evt(
            "go-ex.theme.updated",
            3,
            "go-ex",
            GOEX_FIXTURE_ID,
            "2026-08-09T15:10:20Z",
            json!({"theme":"wood","saturation":0.4}),
            None,
            usage(1.2, 0, 0, 12, 20_000),
        ),
        evt(
            "optimizer.run.completed",
            4,
            "go-ex",
            GOEX_FIXTURE_ID,
            "2026-08-09T15:12:00Z",
            json!({"status":"completed"}),
            None,
            None,
        ),
    ];
    if let Some(event) = events.get_mut(1) {
        event.snapshot = Some(map_from(
            json!({"phase":"explore","tick":3,"status":"running"}),
        ));
    }
    (run, events)
}

fn sft_fixture(session_ref: Option<String>) -> (OptimizerRunRecord, Vec<OptimizerEventEnvelope>) {
    let run = OptimizerRunRecord {
        schema_version: OPTIMIZER_RUN_SCHEMA_VERSION.into(),
        id: SFT_FIXTURE_ID.into(),
        algorithm_id: "sft".into(),
        algorithm_version: Some("0.1.0".into()),
        status: "queued".into(),
        source: "local".into(),
        objective: Some("craftax gpt-oss-20b · fixture curves".into()),
        project_ref: None,
        session_ref,
        created_at: "2026-08-09T15:20:00Z".into(),
        started_at: None,
        finished_at: None,
        cursor_seq: 0,
        capabilities: OptimizerCapabilities::for_algorithm("sft"),
        execution_bindings: vec![],
        input_refs: vec![],
        output_refs: vec![],
        visual_refs: vec![],
        summary: json!({
            "baseModel": "openai/gpt-oss-20b",
            "adapter": "lora_r16",
            "backend": "fake"
        }),
        usage: OptimizerUsageSummary::default(),
        error: None,
    };
    let mut events = vec![
        evt(
            "optimizer.run.created",
            1,
            "sft",
            SFT_FIXTURE_ID,
            "2026-08-09T15:20:01Z",
            json!({"status":"starting"}),
            None,
            None,
        ),
        evt(
            "sft.dataset.validation_started",
            2,
            "sft",
            SFT_FIXTURE_ID,
            "2026-08-09T15:20:03Z",
            json!({"status":"validating_dataset"}),
            None,
            None,
        ),
        evt(
            "sft.dataset.validated",
            3,
            "sft",
            SFT_FIXTURE_ID,
            "2026-08-09T15:20:05Z",
            json!({}),
            None,
            None,
        ),
        evt(
            "sft.training.queued",
            4,
            "sft",
            SFT_FIXTURE_ID,
            "2026-08-09T15:20:08Z",
            json!({"status":"queued"}),
            None,
            None,
        ),
        evt(
            "sft.training.started",
            5,
            "sft",
            SFT_FIXTURE_ID,
            "2026-08-09T15:20:10Z",
            json!({"status":"running"}),
            None,
            usage(0.2, 0, 0, 0, 1_000),
        ),
        evt(
            "sft.compute.updated",
            6,
            "sft",
            SFT_FIXTURE_ID,
            "2026-08-09T15:20:12Z",
            json!({}),
            None,
            None,
        ),
        evt(
            "sft.step.metrics",
            7,
            "sft",
            SFT_FIXTURE_ID,
            "2026-08-09T15:20:30Z",
            json!({"step":50,"epoch":1,"train_loss":1.8,"validation_loss":1.6,"learning_rate":0.0002}),
            None,
            usage(1.0, 0, 0, 0, 20_000),
        ),
        evt(
            "sft.checkpoint.created",
            8,
            "sft",
            SFT_FIXTURE_ID,
            "2026-08-09T15:21:00Z",
            json!({}),
            Some(item(
                "checkpoint",
                "ckpt_50",
                "created",
                json!({"step":50,"digest":"sha256:ckpt50","promoted":false}),
            )),
            None,
        ),
        evt(
            "sft.checkpoint_eval.completed",
            9,
            "sft",
            SFT_FIXTURE_ID,
            "2026-08-09T15:21:20Z",
            json!({"metric":"macro_f1","score":0.74,"accuracy":0.81,"split":"selection","role":"selection"}),
            Some(item(
                "evaluation",
                "eval_ckpt_50",
                "completed",
                json!({"checkpointId":"ckpt_50"}),
            )),
            usage(0.5, 0, 0, 0, 10_000),
        ),
        evt(
            "sft.training.paused",
            10,
            "sft",
            SFT_FIXTURE_ID,
            "2026-08-09T15:21:30Z",
            json!({"status":"paused"}),
            None,
            None,
        ),
        evt(
            "sft.training.resumed",
            11,
            "sft",
            SFT_FIXTURE_ID,
            "2026-08-09T15:21:40Z",
            json!({"status":"running"}),
            None,
            None,
        ),
        evt(
            "sft.step.metrics",
            12,
            "sft",
            SFT_FIXTURE_ID,
            "2026-08-09T15:22:00Z",
            json!({"step":100,"epoch":2,"train_loss":1.1,"validation_loss":1.05,"learning_rate":0.00015}),
            None,
            usage(1.4, 0, 0, 0, 40_000),
        ),
        evt(
            "sft.checkpoint.created",
            13,
            "sft",
            SFT_FIXTURE_ID,
            "2026-08-09T15:22:10Z",
            json!({}),
            Some(item(
                "checkpoint",
                "ckpt_100",
                "created",
                json!({"step":100,"digest":"sha256:ckpt100","promoted":false}),
            )),
            None,
        ),
        evt(
            "sft.checkpoint_eval.completed",
            14,
            "sft",
            SFT_FIXTURE_ID,
            "2026-08-09T15:22:30Z",
            json!({"metric":"macro_f1","score":0.81,"accuracy":0.88,"split":"selection","role":"selection"}),
            Some(item(
                "evaluation",
                "eval_ckpt_100",
                "completed",
                json!({"checkpointId":"ckpt_100"}),
            )),
            usage(0.5, 0, 0, 0, 10_000),
        ),
        evt(
            "sft.checkpoint.promoted",
            15,
            "sft",
            SFT_FIXTURE_ID,
            "2026-08-09T15:22:40Z",
            json!({"checkpointId":"ckpt_100"}),
            Some(item(
                "checkpoint",
                "ckpt_100",
                "promoted",
                json!({"step":100,"digest":"sha256:ckpt100","promoted":true}),
            )),
            None,
        ),
        evt(
            "sft.heldout_eval.completed",
            16,
            "sft",
            SFT_FIXTURE_ID,
            "2026-08-09T15:23:10Z",
            json!({"metric":"macro_f1","score":0.79,"accuracy":0.86,"split":"heldout","role":"heldout","measurementOnly":true}),
            Some(item(
                "evaluation",
                "eval_heldout_ckpt_100",
                "completed",
                json!({"checkpointId":"ckpt_100"}),
            )),
            usage(0.6, 0, 0, 0, 12_000),
        ),
        evt(
            "sft.examples.updated",
            17,
            "sft",
            SFT_FIXTURE_ID,
            "2026-08-09T15:23:20Z",
            json!({}),
            None,
            None,
        ),
        evt(
            "sft.model.materialized",
            18,
            "sft",
            SFT_FIXTURE_ID,
            "2026-08-09T15:23:40Z",
            json!({"status":"completed"}),
            Some(item(
                "artifact",
                "model_ckpt_100",
                "ready",
                json!({
                    "baseModel":"openai/gpt-oss-20b",
                    "adapter":"lora_r16",
                    "checkpointId":"ckpt_100",
                    "digest":"sha256:model100"
                }),
            )),
            None,
        ),
        evt(
            "optimizer.run.completed",
            19,
            "sft",
            SFT_FIXTURE_ID,
            "2026-08-09T15:24:00Z",
            json!({"status":"completed"}),
            None,
            usage(0.8, 0, 0, 0, 77_000),
        ),
    ];
    if let Some(event) = events.get_mut(0) {
        event.snapshot = Some(map_from(json!({
            "summary": {
                "baseModel": "openai/gpt-oss-20b",
                "adapter": "lora_r16",
                "backend": "fake"
            }
        })));
    }
    if let Some(event) = events.get_mut(2) {
        event.snapshot = Some(map_from(json!({
            "splits": {
                "train": {"count": 8000, "digest": "sha256:train"},
                "selection": {"count": 1000, "digest": "sha256:selection"},
                "heldout": {"count": 1000, "digest": "sha256:heldout"}
            },
            "rejected": 42,
            "format": "chat_jsonl"
        })));
    }
    if let Some(event) = events.get_mut(5) {
        event.snapshot = Some(map_from(json!({
            "provider": "fake",
            "gpu": "A100-40G",
            "utilization": 0.72,
            "tokensPerSec": 4100,
            "spendUsd": 0.2
        })));
    }
    if let Some(event) = events.get_mut(6) {
        event.snapshot = Some(map_from(json!({"summary": {"step": 50, "epoch": 1}})));
    }
    if let Some(event) = events.get_mut(11) {
        event.snapshot = Some(map_from(json!({"summary": {"step": 100, "epoch": 2}})));
    }
    if let Some(event) = events.get_mut(14) {
        event.snapshot = Some(map_from(
            json!({"summary": {"promotedCheckpointId": "ckpt_100"}}),
        ));
    }
    if let Some(event) = events.get_mut(16) {
        event.snapshot = Some(map_from(json!({
            "examples": [
                {
                    "id": "ex_1",
                    "intent": "craft_table",
                    "baseline": "walk toward wood",
                    "selected": "gather wood, then craft table",
                    "improved": true
                },
                {
                    "id": "ex_2",
                    "intent": "survive_night",
                    "baseline": "keep exploring",
                    "selected": "return to shelter before nightfall",
                    "improved": true
                }
            ]
        })));
    }
    if let Some(event) = events.get_mut(17) {
        event.artifact_refs = vec![json!({
            "kind": "model",
            "id": "model_ckpt_100",
            "digest": "sha256:model100"
        })];
    }
    (run, events)
}

fn evt(
    event_type: &str,
    sequence: u64,
    algorithm_id: &str,
    run_id: &str,
    occurred_at: &str,
    delta: Value,
    item: Option<Value>,
    usage_delta: Option<Map<String, Value>>,
) -> OptimizerEventEnvelope {
    OptimizerEventEnvelope {
        schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
        event_id: Some(format!("{run_id}:{sequence}")),
        event_type: event_type.into(),
        sequence_number: sequence,
        occurred_at: occurred_at.into(),
        optimizer_run_id: run_id.into(),
        algorithm_id: algorithm_id.into(),
        level: Some("info".into()),
        item,
        delta: map_from(delta),
        snapshot: None,
        usage_delta,
        artifact_refs: vec![],
        error: None,
        raw: json!({}),
    }
}

fn item(kind: &str, id: &str, status: &str, raw: Value) -> Value {
    json!({"kind": kind, "type": kind, "id": id, "status": status, "raw": raw})
}

fn usage(
    cost_usd: f64,
    prompt_tokens: u64,
    completion_tokens: u64,
    rollouts: u64,
    wall_time_ms: u64,
) -> Option<Map<String, Value>> {
    Some(map_from(json!({
        "cost_usd": cost_usd,
        "prompt_tokens": prompt_tokens,
        "completion_tokens": completion_tokens,
        "rollouts": rollouts,
        "wall_time_ms": wall_time_ms
    })))
}

fn map_from(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

#[cfg(test)]
pub(in crate::optimizers) mod tests {
    use super::*;
    use crate::storage::{ContentStore, Storage};
    use tempfile::tempdir;

    #[test]
    fn runtime_and_child_completion_events_are_not_run_terminal() {
        assert_eq!(optimizer_terminal_status("runtime.job.completed"), None);
        assert_eq!(optimizer_terminal_status("proposer.completed"), None);
        assert_eq!(
            optimizer_terminal_status("optimizer.evaluation_result.received"),
            None
        );
        assert_eq!(
            optimizer_terminal_status("optimizer.run.completed"),
            Some("completed")
        );
        assert_eq!(
            optimizer_terminal_status("optimizer.run.cancelled"),
            Some("cancelled")
        );
    }

    #[tokio::test]
    async fn optimizer_controls_require_a_valid_lifecycle_transition() {
        let (svc, _dir, _) = service().await;
        let (mut run, _) = svc
            .seed_fixture("gepa", Some("session_controls".into()))
            .await
            .unwrap();
        run.capabilities.cancel = true;
        run.capabilities.pause = true;
        run.capabilities.resume = true;

        run.status = "running".into();
        assert!(validate_control(&run, "pause").is_ok());
        assert!(validate_control(&run, "resume").is_err());

        run.status = "paused".into();
        assert!(validate_control(&run, "resume").is_ok());
        assert!(validate_control(&run, "pause").is_err());

        for terminal in ["completed", "failed", "cancelled"] {
            run.status = terminal.into();
            assert!(validate_control(&run, "cancel").is_err());
            assert!(validate_control(&run, "pause").is_err());
            assert!(validate_control(&run, "resume").is_err());
        }
    }

    /// Shared with `eval_recipes::tests`, which replays a real worker stream
    /// through the same service to prove the projection.
    pub(in crate::optimizers) async fn service() -> (
        OptimizerService,
        tempfile::TempDir,
        tokio::sync::broadcast::Receiver<AppEvent>,
    ) {
        let dir = tempdir().unwrap();
        let storage = Storage::open(dir.path().join("core")).unwrap();
        let journal = EventJournal::new(storage.database().clone());
        let content = ContentStore::new(storage.content_root());
        let visuals = VisualRegistry::new(storage.database().clone(), journal.clone(), content);
        let (events_tx, events_rx) = tokio::sync::broadcast::channel(16);
        let manager = Arc::new(crate::optimizers::OptimizerManager::with_home(
            dir.path().join("optimizer-home"),
        ));
        std::fs::create_dir_all(manager.home()).unwrap();
        std::fs::write(
            manager.home().join("capabilities.json"),
            // Mirrors what a real handshake now stores: runtime-owned facts.
            // Template ids are resolved host-side and are no longer requested
            // from, or answered by, the runtime.
            //
            // `gepa` alone, because that is what the Desktop-managed sidecar
            // actually serves. Local eval is a separate runtime that bypasses
            // negotiation entirely, and sft's control plane is its own surface.
            // The list this replaced was derived from `compatibleTemplateIds`
            // and carried `optimizer.dag.live.v1` — an id with no implementation
            // behind it — so translating it wholesale would have had the fake
            // advertise an algorithm no runtime can run.
            serde_json::to_vec(&json!({
                "algorithms": ["gepa"],
                "replay": true,
                "cancellation": true
            }))
            .unwrap(),
        )
        .unwrap();
        (
            OptimizerService::new_with_manager(
                storage.database().clone(),
                journal,
                visuals,
                events_tx,
                manager,
            ),
            dir,
            events_rx,
        )
    }

    #[tokio::test]
    async fn seeds_gepa_fixture_and_projects_slices() {
        let (svc, _dir, _) = service().await;
        let (run, _) = svc
            .seed_fixture("gepa", Some("session_test".into()))
            .await
            .unwrap();
        assert_eq!(run.algorithm_id, "gepa");
        assert!(run.cursor_seq >= 10);
        assert!(!run.visual_refs.is_empty());
        let frontier = svc
            .get_state(run.id.clone(), "gepa.frontier".into(), Some(8))
            .await
            .unwrap();
        assert_eq!(frontier.slice_id, "gepa.frontier");
        let cells = frontier
            .data
            .get("cells")
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(cells.len(), 2);
    }

    #[tokio::test]
    async fn opens_primary_visual_in_current_conversation_without_reassigning_run() {
        let (svc, _dir, _) = service().await;
        svc.manager()
            .set_status(crate::optimizers::OptimizerSidecarStatus {
                phase: "ready".into(),
                base_url: None,
                version: Some("0.2.9.dev20260814".into()),
                digest: Some("sha256:template-package".into()),
                detail: None,
                updated_at: 0,
            })
            .await;
        let (run, _) = svc
            .create(
                serde_json::from_value(json!({
                    "algorithmId": "gepa",
                    "id": "gepa_historical_run",
                    "sessionRef": "session_original",
                    "openVisual": false
                }))
                .unwrap(),
            )
            .await
            .unwrap();

        let (shown, event) = svc
            .open_visual_in_session(run.id.clone(), Some("session_current".into()))
            .await
            .unwrap();
        let event = event.expect("visual show event");

        assert_eq!(shown.session_ref.as_deref(), Some("session_original"));
        assert_eq!(event.kind, "visual.show");
        assert_eq!(event.session_id.as_deref(), Some("session_current"));
        assert_eq!(shown.visual_refs.len(), 1);
        assert_eq!(
            shown.visual_refs[0].digest.as_deref(),
            Some("sha256:template-package")
        );

        let visual_id = shown.visual_refs[0].id.clone();
        assert_eq!(
            svc.visuals
                .get(visual_id.clone())
                .await
                .unwrap()
                .metadata
                .get("templateDigest"),
            Some(&json!("sha256:template-package"))
        );
        let (reopened, second_event) = svc
            .open_visual_in_session(run.id, Some("session_current".into()))
            .await
            .unwrap();
        assert_eq!(reopened.visual_refs[0].id, visual_id);
        assert_eq!(
            second_event.unwrap().session_id.as_deref(),
            Some("session_current")
        );
    }

    #[test]
    fn visual_selection_is_host_owned_and_capability_checks_guard_execution() {
        // Visual selection never consults the plugin. It used to intersect
        // against `compatibleTemplateIds` — host vocabulary the plugin only
        // knew because Desktop's install payload told it — so the check
        // compared a host constant against a round-trip of that same constant.
        // Worse, tightening it would have broken every run the managed sidecar
        // does not serve: the real plugin advertises only `gepa`, so hosted SFT
        // and local eval would have lost their visuals entirely.
        assert_eq!(negotiate_visual_template("gepa"), "optimizer.gepa.live.v1");
        assert_eq!(negotiate_visual_template("sft"), "optimizer.sft.live.v1");
        assert_eq!(negotiate_visual_template("eval"), "optimizer.eval.live.v1");

        // Execution is where a capability claim has to hold up.
        let serves_sft_only = json!({ "algorithms": ["sft"] });
        let error = require_advertised_algorithm(&serves_sft_only, "gepa").unwrap_err();
        assert!(
            error.to_string().contains("does not advertise algorithm"),
            "got: {error}"
        );

        // Absent capabilities refuse rather than waving the run through.
        let absent = require_advertised_algorithm(&json!({}), "gepa").unwrap_err();
        assert!(
            absent.to_string().contains("advertise no algorithms"),
            "got: {absent}"
        );

        let serves_gepa = json!({ "algorithms": ["gepa"] });
        require_advertised_algorithm(&serves_gepa, "gepa").unwrap();
        // Namespaced ids resolve through their root algorithm. Asserted with a
        // fabricated id on purpose: the only namespaced arm in the tree is
        // `dag.*`, which has no implementation behind it, and naming it here
        // would document dead code as a supported capability.
        require_advertised_algorithm(&serves_gepa, "gepa.variant").unwrap();
    }

    #[tokio::test]
    async fn dedupes_replayed_events() {
        let (svc, _dir, _) = service().await;
        let (run, _) = svc.seed_fixture("gepa", None).await.unwrap();
        let events = svc.events_after(run.id.clone(), 0, None).await.unwrap();
        let (again, _) = svc.append_events(run.id.clone(), events).await.unwrap();
        assert_eq!(again.cursor_seq, run.cursor_seq);
    }

    #[tokio::test]
    async fn append_events_publishes_optimizer_run_updated_on_the_bus() {
        let (svc, _dir, mut rx) = service().await;
        let (run, _) = svc.seed_fixture("gepa", None).await.unwrap();
        while rx.try_recv().is_ok() {}
        let extra = OptimizerEventEnvelope {
            schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
            event_id: Some(format!("{}:bus-test", run.id)),
            event_type: "optimizer.recipe.diagnostic".into(),
            sequence_number: run.cursor_seq + 1,
            occurred_at: chrono::Utc::now().to_rfc3339(),
            optimizer_run_id: run.id.clone(),
            algorithm_id: "gepa".into(),
            level: Some("info".into()),
            item: None,
            delta: serde_json::Map::new(),
            snapshot: None,
            usage_delta: None,
            artifact_refs: vec![],
            error: None,
            raw: json!({ "source": "bus_test" }),
        };
        svc.append_events(run.id.clone(), vec![extra])
            .await
            .unwrap();
        let event = rx.try_recv().expect("optimizer.run.updated on the bus");
        assert_eq!(event.kind, "optimizer.run.updated");
        assert_eq!(event.payload["optimizerRunId"], run.id);
    }

    /// Regression for the A3 Banking77 runs: the sidecar registers candidates
    /// and reports the frontier on the delta, with no `item` and no `cells`.
    /// Both slices used to project empty on a real run.
    #[tokio::test]
    async fn projects_sidecar_candidates_and_frontier_without_item_or_cells() {
        let (svc, _dir, _) = service().await;
        let (run, _) = svc
            .create(
                serde_json::from_value(json!({
                    "algorithmId": "gepa",
                    "id": "gepa_projection_probe",
                    "openVisual": false
                }))
                .unwrap(),
            )
            .await
            .unwrap();
        let event = |seq: u64, event_type: &str, delta: Value| OptimizerEventEnvelope {
            schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
            event_id: Some(format!("{}:{seq}", run.id)),
            event_type: event_type.into(),
            sequence_number: seq,
            occurred_at: chrono::Utc::now().to_rfc3339(),
            optimizer_run_id: run.id.clone(),
            algorithm_id: "gepa".into(),
            level: Some("info".into()),
            item: None,
            delta: delta.as_object().cloned().unwrap_or_default(),
            snapshot: None,
            usage_delta: None,
            artifact_refs: vec![],
            error: None,
            raw: json!({}),
        };
        svc.append_events(
            run.id.clone(),
            vec![
                event(
                    1,
                    "candidate.registered",
                    json!({"candidate_id": "gepa_seed", "source": "seed"}),
                ),
                event(
                    2,
                    "candidate.evaluated",
                    json!({"candidate_id": "gepa_seed", "train_reward": 0.7}),
                ),
                event(
                    3,
                    "frontier.updated",
                    json!({
                        "best_candidate_id": "gepa_seed",
                        "best_train_reward": 0.7,
                        "candidate_count": 1,
                        "coverage_semantics": "solved_reward_positive"
                    }),
                ),
            ],
        )
        .await
        .unwrap();

        let candidates = svc
            .get_state(run.id.clone(), "gepa.candidates".into(), None)
            .await
            .unwrap();
        let rows = candidates
            .data
            .get("candidates")
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["candidate_id"], json!("gepa_seed"));
        assert_eq!(rows[0]["train_reward"], json!(0.7));

        let frontier = svc
            .get_state(run.id.clone(), "gepa.frontier".into(), None)
            .await
            .unwrap();
        let cells = frontier
            .data
            .get("cells")
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0]["candidateId"], json!("gepa_seed"));
        assert_eq!(cells[0]["trainReward"], json!(0.7));
    }

    /// A run nobody reported cost for is unknown, not free.
    #[tokio::test]
    async fn unreported_cost_stays_null_never_zero() {
        let (svc, _dir, _) = service().await;
        let (run, _) = svc
            .create(
                serde_json::from_value(json!({
                    "algorithmId": "gepa",
                    "id": "gepa_cost_probe",
                    "openVisual": false
                }))
                .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(run.usage.cost_usd, None);
        let usage = svc
            .get_state(run.id.clone(), "run.usage".into(), None)
            .await
            .unwrap();
        assert_eq!(usage.data.get("costUsd"), Some(&Value::Null));
    }

    #[tokio::test]
    async fn exceeding_an_approved_cap_is_a_durable_receipt_violation() {
        let (svc, _dir, _) = service().await;
        let (run, _) = svc
            .create(
                serde_json::from_value(json!({
                    "algorithmId": "gepa",
                    "id": "gepa_cap_probe",
                    "openVisual": false
                }))
                .unwrap(),
            )
            .await
            .unwrap();
        let run = svc
            .attach_paid_compute_approval(run, "approval-paid", Some(500_000), Some(4))
            .await
            .unwrap();
        let event = OptimizerEventEnvelope {
            schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
            event_id: Some("gepa_cap_probe:1".into()),
            event_type: "optimizer.usage".into(),
            sequence_number: 1,
            occurred_at: chrono::Utc::now().to_rfc3339(),
            optimizer_run_id: run.id.clone(),
            algorithm_id: "gepa".into(),
            level: Some("info".into()),
            item: None,
            delta: Map::new(),
            snapshot: None,
            usage_delta: Some(
                json!({"rollouts":5,"cost_usd":0.25})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
            artifact_refs: vec![],
            error: None,
            raw: json!({}),
        };
        svc.append_events(run.id.clone(), vec![event])
            .await
            .unwrap();
        let stored = svc.get(run.id).await.unwrap();
        assert_eq!(
            stored.usage.extra["paidComputeApproval"]["receiptViolation"],
            true
        );
        assert_eq!(
            stored.usage.extra["paidComputeApproval"]["violationReason"],
            "rollout_cap_exceeded"
        );
    }

    #[test]
    fn mixed_cost_receipts_never_project_a_partial_sum() {
        let mut usage = OptimizerUsageSummary::default();
        apply_reported_cost(
            &mut usage,
            &serde_json::from_value(json!({"cost_usd": 0.02})).unwrap(),
        );
        assert_eq!(usage.cost_usd, Some(0.02));
        apply_reported_cost(
            &mut usage,
            &serde_json::from_value(json!({"cost_usd": null})).unwrap(),
        );
        assert_eq!(usage.cost_usd, None);
        apply_reported_cost(
            &mut usage,
            &serde_json::from_value(json!({"cost_usd": 0.03})).unwrap(),
        );
        assert_eq!(usage.cost_usd, None);
        assert_eq!(
            usage.extra.get("costTelemetryComplete"),
            Some(&Value::Bool(false))
        );
    }

    #[test]
    fn token_receipt_without_cost_poisons_cost_completeness() {
        let mut usage = OptimizerUsageSummary::default();
        apply_reported_cost(
            &mut usage,
            &serde_json::from_value(json!({"cost_usd": 0.02})).unwrap(),
        );
        apply_reported_cost(
            &mut usage,
            &serde_json::from_value(json!({"prompt_tokens": 10, "completion_tokens": 2})).unwrap(),
        );
        assert_eq!(usage.cost_usd, None);
        assert_eq!(
            usage.extra.get("costTelemetryComplete"),
            Some(&Value::Bool(false))
        );
    }

    /// A recipe in the catalog that `start_recipe` does not route is a dead
    /// button in the product. Every advertised id must dispatch somewhere.
    #[tokio::test]
    async fn every_catalogued_recipe_is_routable() {
        let (svc, _dir, _) = service().await;
        for recipe in svc.list_recipes() {
            let id = recipe
                .get("id")
                .and_then(Value::as_str)
                .expect("recipe id")
                .to_string();
            let error = svc
                .start_recipe(
                    serde_json::from_value(json!({ "recipeId": id, "openVisual": false })).unwrap(),
                )
                .await
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default();
            assert!(
                !error.contains("unknown optimizer recipe"),
                "catalogued recipe {id} is not routed by start_recipe"
            );
        }
    }

    #[tokio::test]
    async fn reports_sft_as_available_algorithm() {
        let (svc, _dir, _) = service().await;
        let sft = svc
            .list_algorithms()
            .into_iter()
            .find(|a| a.get("id") == Some(&json!("sft")))
            .unwrap();
        assert_eq!(sft.get("availability"), Some(&json!("available")));
    }

    #[tokio::test]
    async fn lists_hosted_sft_fixture_recipe() {
        let (svc, _dir, _) = service().await;
        let recipe = svc
            .list_recipes()
            .into_iter()
            .find(|item| item.get("id") == Some(&json!("sft.hosted.fixture.v1")))
            .unwrap();
        assert_eq!(recipe.get("algorithmId"), Some(&json!("sft")));
        assert_ne!(recipe.get("id"), Some(&json!("goex.sft.v1")));
    }

    #[tokio::test]
    async fn lists_craftax_nemotron_tinker_recipe_and_refuses_unknown_base_model() {
        let (svc, _dir, _) = service().await;
        let recipe = svc
            .list_recipes()
            .into_iter()
            .find(|item| item.get("id") == Some(&json!("sft.craftax.nemotron-nano.tinker.v1")))
            .unwrap();
        assert_eq!(recipe.get("algorithmId"), Some(&json!("sft")));
        if std::env::var("SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .is_none()
        {
            assert_eq!(recipe.get("availability"), Some(&json!("unavailable")));
        }
        let err = svc
            .start_recipe(super::super::models::OptimizerRecipeRunRequest {
                recipe_id: "sft.craftax.nemotron-nano.tinker.v1".into(),
                session_ref: None,
                open_visual: Some(false),
                base_model: Some("nvidia/nemotron-3-nano-30b-a3b".into()),
                dataset_shard: None,
                candidate_set_id: None,
                search: None,
            })
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("sft_tinker_base_models.toml"), "{err}");
    }

    #[tokio::test]
    async fn sft_training_completed_does_not_mark_the_run_succeeded() {
        let (svc, _dir, _) = service().await;
        let (run, _) = svc
            .create(OptimizerCreateRequest {
                algorithm_id: "sft".into(),
                algorithm_version: None,
                objective: Some("hosted sft status".into()),
                source: Some("hosted".into()),
                project_ref: None,
                session_ref: None,
                id: Some("sft_hosted_status".into()),
                execution_bindings: None,
                input_refs: None,
                capabilities: None,
                summary: None,
                open_visual: Some(false),
                seed_fixture: None,
                cloud_config: None,
                local_path: None,
            })
            .await
            .unwrap();
        let (updated, _) = svc
            .append_events(
                run.id.clone(),
                vec![evt(
                    "sft.training.completed",
                    1,
                    "sft",
                    &run.id,
                    "2026-08-12T19:40:00Z",
                    json!({"status": "succeeded"}),
                    None,
                    None,
                )],
            )
            .await
            .unwrap();
        assert_eq!(updated.status, "queued");
        assert_ne!(updated.status, "succeeded");
        assert_ne!(updated.status, "completed");
    }

    #[tokio::test]
    async fn sft_fixture_projects_slices_and_scrubs_checkpoints() {
        let (svc, _dir, _) = service().await;
        let (run, _) = svc.seed_fixture("sft", None).await.unwrap();
        assert_eq!(run.algorithm_id, "sft");
        assert_eq!(
            run.visual_refs
                .iter()
                .find(|resource| resource.kind == "visual")
                .and_then(|resource| resource.metadata.get("templateId")),
            Some(&json!("optimizer.sft.live.v1"))
        );
        assert!(run.cursor_seq >= 18);
        assert_eq!(
            svc.list_algorithms()
                .into_iter()
                .find(|a| a.get("id") == Some(&json!("sft")))
                .unwrap()
                .get("availability"),
            Some(&json!("available"))
        );

        let latest = svc
            .get_state_batch(
                run.id.clone(),
                Some(vec![
                    "sft.checkpoints".into(),
                    "sft.training_curves".into(),
                    "sft.checkpoint_evaluations".into(),
                    "sft.dataset".into(),
                    "sft.examples".into(),
                ]),
                None,
            )
            .await
            .unwrap();
        let checkpoints = latest
            .iter()
            .find(|slice| slice.slice_id == "sft.checkpoints")
            .unwrap();
        let ckpts = checkpoints
            .data
            .get("checkpoints")
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(ckpts.len(), 2);
        assert!(ckpts.iter().any(|ckpt| {
            ckpt.get("status") == Some(&json!("promoted"))
                || ckpt.get("raw").and_then(|raw| raw.get("promoted")) == Some(&json!(true))
        }));

        let mid = svc
            .get_state(run.id.clone(), "sft.checkpoints".into(), Some(9))
            .await
            .unwrap();
        let mid_ckpts = mid
            .data
            .get("checkpoints")
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(mid_ckpts.len(), 1);
        assert_eq!(mid_ckpts[0].get("id"), Some(&json!("ckpt_50")));

        let evals = svc
            .get_state(run.id.clone(), "sft.checkpoint_evaluations".into(), None)
            .await
            .unwrap();
        let evaluations = evals
            .data
            .get("evaluations")
            .and_then(Value::as_array)
            .unwrap();
        assert!(evaluations.iter().any(|evaluation| {
            evaluation.get("role") == Some(&json!("heldout"))
                || evaluation.get("delta").and_then(|delta| delta.get("role"))
                    == Some(&json!("heldout"))
        }));
    }

    #[tokio::test]
    async fn imports_local_gepa_oss_event_feed() {
        let (svc, dir, _) = service().await;
        let feed = dir.path().join("event_feed.jsonl");
        std::fs::write(
            &feed,
            r#"{"schema_version":"event_stream_record.v1","event_id":"e1","sequence_number":1,"event_type":"run.started","timestamp":"2026-08-09T15:00:00Z","fields":{"run_id":"gepa_import_1"},"event":{}}
{"schema_version":"event_stream_record.v1","event_id":"e2","sequence_number":2,"event_type":"candidate.accepted","timestamp":"2026-08-09T15:00:01Z","fields":{"run_id":"gepa_import_1","candidate_id":"c1","train_reward":0.5},"event":{}}
"#,
        )
        .unwrap();
        let (run, _) = svc
            .import_local(super::super::models::OptimizerImportLocalRequest {
                path: feed.display().to_string(),
                session_ref: None,
                open_visual: Some(false),
            })
            .await
            .unwrap();
        assert_eq!(run.id, "gepa_import_1");
        assert_eq!(run.algorithm_id, "gepa");
        assert_eq!(run.source, "local");
        assert!(run.cursor_seq >= 2);
    }

    #[tokio::test]
    async fn imports_local_gelo_events_jsonl() {
        let (svc, dir, _) = service().await;
        let run_dir = dir
            .path()
            .join("runs")
            .join("goex_import_1")
            .join("artifacts");
        std::fs::create_dir_all(&run_dir).unwrap();
        let feed = run_dir.join("events.jsonl");
        std::fs::write(
            &feed,
            r#"{"run_id":"goex_import_1","event_type":"theme.updated","_seq":1,"created_at":"2026-08-09T15:10:00Z","payload":{"theme":"oak"},"algorithm":"go-ex"}
{"run_id":"goex_import_1","event_type":"run.completed","_seq":2,"created_at":"2026-08-09T15:11:00Z","payload":{},"algorithm":"go-ex"}
"#,
        )
        .unwrap();
        let (run, _) = svc
            .import_local(super::super::models::OptimizerImportLocalRequest {
                path: dir
                    .path()
                    .join("runs")
                    .join("goex_import_1")
                    .display()
                    .to_string(),
                session_ref: None,
                open_visual: Some(false),
            })
            .await
            .unwrap();
        assert_eq!(run.algorithm_id, "go-ex");
        assert_eq!(run.source, "local");
        assert!(run.cursor_seq >= 2);
    }

    #[tokio::test]
    async fn imports_local_optimizer_event_sidecar() {
        let (svc, dir, _) = service().await;
        let feed = dir.path().join("events.optimizer.jsonl");
        std::fs::write(
            &feed,
            r#"{"schema_version":"optimizer_event.v1","type":"theme.updated","sequence_number":1,"created_at":"2026-08-09T15:10:00Z","run_id":"goex_canon_1","optimizer_run_id":"goex_canon_1","algorithm_id":"go-ex","delta":{"theme":"oak"},"raw":{}}
{"schema_version":"optimizer_event.v1","type":"run.completed","sequence_number":2,"created_at":"2026-08-09T15:11:00Z","run_id":"goex_canon_1","optimizer_run_id":"goex_canon_1","algorithm_id":"go-ex","delta":{},"raw":{}}
"#,
        )
        .unwrap();
        let (run, _) = svc
            .import_local(super::super::models::OptimizerImportLocalRequest {
                path: feed.display().to_string(),
                session_ref: None,
                open_visual: Some(false),
            })
            .await
            .unwrap();
        assert_eq!(run.id, "goex_canon_1");
        assert_eq!(run.algorithm_id, "go-ex");
        assert!(run.cursor_seq >= 2);
    }

    #[tokio::test]
    async fn prepared_compute_requires_ready_approval_and_matching_digest() {
        let (svc, _dir, _) = service().await;
        let (run, _) = svc
            .create(
                serde_json::from_value(json!({
                    "algorithmId": "gepa",
                    "id": "banking77_prepare_gate",
                    "openVisual": false
                }))
                .unwrap(),
            )
            .await
            .unwrap();
        let mut stored = run.clone();
        stored.status = "waiting_for_viewer".into();
        stored.summary = json!({
            "recipeId": "gepa.banking77.smoke.v1",
            "preparationDigest": "sha256:prepare",
            "capabilitiesDigest": "sha256:caps"
        });
        svc.persist_run(stored).await.unwrap();

        let mismatch = svc
            .start_prepared(
                run.id.clone(),
                Some("sha256:other".into()),
                Some("approval-1".into()),
            )
            .await
            .unwrap_err();
        assert!(mismatch.to_string().contains("digest mismatch"));

        let missing_ready = svc
            .start_prepared(
                run.id.clone(),
                Some("sha256:prepare".into()),
                Some("approval-1".into()),
            )
            .await
            .unwrap_err();
        assert!(missing_ready.to_string().contains("visual readiness"));

        svc.record_visual_ready(
            run.id.clone(),
            json!({
                "schemaVersion": "synth.visual-subscription-receipt.v1",
                "visualId": "visual_prepare_gate",
                "optimizerRunId": run.id,
                "templateId": "optimizer.gepa.live.v1",
                "replayedThrough": 0,
                "subscribedFrom": 1
            }),
        )
        .await
        .unwrap();

        let ready = svc.get(run.id.clone()).await.unwrap();
        assert_eq!(ready.summary["waitingForViewer"], json!(false));
        assert!(ready.summary.get("visualReadyReceipt").is_some());

        let missing_approval = svc
            .start_prepared(run.id.clone(), Some("sha256:prepare".into()), None)
            .await
            .unwrap_err();
        assert!(missing_approval.to_string().contains("approval"));
    }

    /// A4. Absent capabilities must refuse, not skip.
    ///
    /// Before this gate closed, a run prepared while no handshake had ever
    /// succeeded carried no `capabilitiesDigest`, so start's comparison was a
    /// skipped `if let` and paid compute began entirely unguarded — the anti-swap
    /// pin was inert in exactly the case it exists to catch. The three existing
    /// `start_prepared` assertions all trip earlier gates (preparation digest,
    /// visual readiness, approval) and never reach this check, so it needs its
    /// own coverage.
    #[tokio::test]
    async fn absent_capabilities_refuse_paid_start_instead_of_skipping_the_pin() {
        async fn run_past_earlier_gates(
            svc: &OptimizerService,
            id: &str,
            summary: Value,
        ) -> String {
            let (run, _) = svc
                .create(
                    serde_json::from_value(json!({
                        "algorithmId": "gepa",
                        "id": id,
                        "openVisual": false
                    }))
                    .unwrap(),
                )
                .await
                .unwrap();
            let mut stored = run.clone();
            stored.status = "waiting_for_viewer".into();
            stored.summary = summary;
            svc.persist_run(stored).await.unwrap();
            svc.record_visual_ready(
                run.id.clone(),
                json!({
                    "schemaVersion": "synth.visual-subscription-receipt.v1",
                    "visualId": format!("visual_{id}"),
                    "optimizerRunId": run.id,
                    "templateId": "optimizer.gepa.live.v1",
                    "replayedThrough": 0,
                    "subscribedFrom": 1
                }),
            )
            .await
            .unwrap();
            run.id
        }

        let (svc, _dir, _) = service().await;
        let home = svc.manager.home().to_path_buf();
        let write_caps = |digest: Option<&str>, algorithms: &[&str]| {
            let mut caps = json!({
                "algorithms": algorithms,
                "replay": true,
                "cancellation": true
            });
            if let Some(digest) = digest {
                caps.as_object_mut()
                    .unwrap()
                    .insert("digest".into(), json!(digest));
            }
            std::fs::write(
                home.join("capabilities.json"),
                serde_json::to_vec(&caps).unwrap(),
            )
            .unwrap();
        };

        // The run was prepared against a proven handshake, but the sidecar is no
        // longer proving anything. Previously: skipped. Now: refused.
        write_caps(None, &["gepa"]);
        let id = run_past_earlier_gates(
            &svc,
            "caps_absent_live",
            json!({
                "recipeId": "gepa.banking77.smoke.v1",
                "preparationDigest": "sha256:prepare",
                "capabilitiesDigest": "sha256:caps"
            }),
        )
        .await;
        let error = svc
            .start_prepared(id, Some("sha256:prepare".into()), Some("approval-1".into()))
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("capabilities are not proven"),
            "live capabilities absent must refuse, got: {error}"
        );

        // The run was prepared while nothing was proven. This is the fails-open
        // case: no pin was ever recorded, so there was nothing to compare.
        write_caps(Some("sha256:caps"), &["gepa"]);
        let id = run_past_earlier_gates(
            &svc,
            "caps_absent_prepared",
            json!({
                "recipeId": "gepa.banking77.smoke.v1",
                "preparationDigest": "sha256:prepare"
            }),
        )
        .await;
        let error = svc
            .start_prepared(id, Some("sha256:prepare".into()), Some("approval-1".into()))
            .await
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("prepared without a proven optimizer capability digest"),
            "a run with no recorded pin must refuse, got: {error}"
        );

        // A3: matching digests prove capabilities are unchanged, not that they
        // cover this run. A handshake advertising an unrelated algorithm passes
        // both shape-validation and the digest pin, and must still be refused.
        write_caps(Some("sha256:caps"), &["sft"]);
        let id = run_past_earlier_gates(
            &svc,
            "caps_wrong_algorithm",
            json!({
                "recipeId": "gepa.banking77.smoke.v1",
                "preparationDigest": "sha256:prepare",
                "capabilitiesDigest": "sha256:caps"
            }),
        )
        .await;
        let error = svc
            .start_prepared(id, Some("sha256:prepare".into()), Some("approval-1".into()))
            .await
            .unwrap_err();
        assert!(
            error.to_string().contains("does not advertise algorithm"),
            "a runtime that does not serve this algorithm must refuse, got: {error}"
        );

        // Both present and equal, and the algorithm is served: the capability
        // gate is satisfied and control reaches the recipe. Whatever fails past
        // here, it is not this gate.
        write_caps(Some("sha256:caps"), &["gepa"]);
        let id = run_past_earlier_gates(
            &svc,
            "caps_matched",
            json!({
                "recipeId": "gepa.banking77.smoke.v1",
                "preparationDigest": "sha256:prepare",
                "capabilitiesDigest": "sha256:caps"
            }),
        )
        .await;
        let outcome = svc
            .start_prepared(id, Some("sha256:prepare".into()), Some("approval-1".into()))
            .await;
        if let Err(error) = outcome {
            let text = error.to_string();
            assert!(
                !text.contains("capabilities are not proven")
                    && !text.contains("prepared without a proven")
                    && !text.contains("capability digest changed")
                    && !text.contains("does not advertise algorithm")
                    && !text.contains("advertise no algorithms"),
                "matching digests and a served algorithm must clear the capability gate, got: {text}"
            );
        }
    }

    #[tokio::test]
    async fn get_result_returns_structured_prompt_without_filesystem_paths() {
        let (svc, dir, _) = service().await;
        let run_dir = dir.path().join("banking77_result");
        std::fs::create_dir_all(&run_dir).unwrap();
        std::fs::write(
            run_dir.join("best_candidate.json"),
            r#"{
                "candidate_id": "candidate_winner",
                "parent_id": "seed",
                "lever_bundle": {
                    "values": {
                        "stage2_system": "Classify the Banking77 intent carefully."
                    }
                },
                "payload": {
                    "stage2_system": "Classify the Banking77 intent carefully."
                }
            }"#,
        )
        .unwrap();
        std::fs::write(
            run_dir.join("result_manifest.json"),
            r#"{
                "best_candidate": {
                    "candidate_id": "candidate_winner",
                    "parent_id": "seed",
                    "acceptance_score": 0.82,
                    "minibatch_reward": 0.85,
                    "heldout_reward": 0.80
                }
            }"#,
        )
        .unwrap();
        let (run, _) = svc
            .create(
                serde_json::from_value(json!({
                    "algorithmId": "gepa",
                    "id": "banking77_result_run",
                    "openVisual": false,
                    "summary": {
                        "runDirectory": run_dir.display().to_string(),
                        "selection": {"score": 0.82},
                        "heldout": {"score": 0.80}
                    }
                }))
                .unwrap(),
            )
            .await
            .unwrap();
        let mut stored = run.clone();
        stored.status = "completed".into();
        stored.summary = json!({
            "runDirectory": run_dir.display().to_string()
        });
        svc.persist_run(stored).await.unwrap();
        let result = svc.get_result(run.id.clone()).await.unwrap();
        assert_eq!(result["schemaVersion"], json!("optimizer_result.v1"));
        assert_eq!(
            result["selectedCandidate"]["materializedValues"]["prompt"],
            json!("Classify the Banking77 intent carefully.")
        );
        assert!(result["selectedCandidate"]["materializedDigest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"));
        assert_eq!(result["metrics"]["selection"]["accepted"], json!(true));
        assert_eq!(
            result["metrics"]["heldoutMeasurement"],
            json!({"score": 0.80, "split": "heldout"})
        );
        let encoded = result.to_string();
        assert!(!encoded.contains("best_candidate.json"));
        assert!(!encoded.contains("runDirectory"));
        assert!(!encoded.contains(&run_dir.display().to_string()));
        assert!(!result["artifactRefs"][0]["id"].as_str().unwrap().is_empty());
        assert_eq!(result["identityConsistent"], json!(true));
        assert_eq!(
            result["optimizationSelectedCandidate"]["id"],
            json!("candidate_winner")
        );
        assert_eq!(
            result["heldoutBestCandidate"]["id"],
            json!("candidate_winner")
        );
        assert_eq!(result["usage"]["authority"], json!("manifest"));
        assert_eq!(
            result["usage"]["reconciliationStatus"],
            json!("manifest_absent")
        );
        assert_eq!(result["selectedCandidate"]["selectionCriterion"], json!("optimization_selected"));
    }

    #[tokio::test]
    async fn get_result_keeps_optimization_and_heldout_identities_distinct() {
        let (svc, dir, _) = service().await;
        let run_dir = dir.path().join("identity_split");
        std::fs::create_dir_all(&run_dir).unwrap();
        std::fs::write(
            run_dir.join("best_candidate.json"),
            r#"{"candidate_id":"heldout_winner","parent_id":"seed","payload":{"prompt":"heldout prompt"}}"#,
        )
        .unwrap();
        std::fs::write(
            run_dir.join("result_manifest.json"),
            r#"{"optimization_selected_candidate":{"id":"train_selected","score":0.7},"heldout_best_candidate":{"id":"heldout_winner","score":0.9}}"#,
        )
        .unwrap();
        let (run, _) = svc
            .create(
                serde_json::from_value(json!({
                    "algorithmId": "gepa",
                    "id": "identity_split_run",
                    "openVisual": false
                }))
                .unwrap(),
            )
            .await
            .unwrap();
        let mut stored = run.clone();
        stored.status = "completed".into();
        stored.cursor_seq = 12;
        stored.summary = json!({
            "runDirectory": run_dir.display().to_string(),
            "terminalCursor": 10
        });
        svc.persist_run(stored).await.unwrap();
        let result = svc.get_result(run.id).await.unwrap();
        assert_eq!(result["finalCursor"], json!(10));
        assert_eq!(result["enrichmentCursor"], json!(12));
        assert_eq!(result["optimizationSelectedCandidate"]["id"], json!("train_selected"));
        assert_eq!(result["heldoutBestCandidate"]["id"], json!("heldout_winner"));
        assert_eq!(result["selectedCandidate"]["id"], json!("train_selected"));
        assert_eq!(result["identityConsistent"], json!(false));
    }

    #[tokio::test]
    async fn terminal_summary_records_visual_evidence_without_blocking_completion() {
        let (svc, _dir, _) = service().await;
        let (run, _) = svc
            .create(
                serde_json::from_value(json!({
                    "algorithmId": "gepa",
                    "id": "visual_evidence_run",
                    "openVisual": false
                }))
                .unwrap(),
            )
            .await
            .unwrap();
        let mut stored = run.clone();
        stored.status = "completed".into();
        stored.visual_refs = vec![crate::optimizers::models::OptimizerResourceRef {
            kind: "visual".into(),
            id: "visual_evidence_1".into(),
            digest: None,
            role: None,
            title: None,
            metadata: json!({}),
        }];
        stored.summary = json!({});
        let persisted = svc.persist_run(stored).await.unwrap();
        assert_eq!(persisted.status, "completed");
        assert_eq!(persisted.summary["visualEvidence"]["state"], json!("partial"));
        assert_eq!(
            persisted.summary["visualEvidence"]["detail"]
                .as_str()
                .unwrap()
                .contains("does not block"),
            true
        );

        let mut ready = persisted.clone();
        ready.summary["visualReadyReceipt"] = json!({"schemaVersion": "synth.visual-subscription-receipt.v1"});
        ready.summary.as_object_mut().unwrap().remove("visualEvidence");
        let rerecorded = svc.persist_run(ready).await.unwrap();
        assert_eq!(rerecorded.summary["visualEvidence"]["state"], json!("ready"));
    }

    #[tokio::test]
    async fn craftax_result_materializes_react_prompt_without_filesystem_paths() {
        let (svc, dir, _) = service().await;
        let run_dir = dir.path().join("craftax_result");
        std::fs::create_dir_all(&run_dir).unwrap();
        std::fs::write(
            run_dir.join("best_candidate.json"),
            r#"{
                "candidate_id": "craftax_winner",
                "parent_id": "seed",
                "lever_bundle": {
                    "values": {
                        "react_system_prompt": "Observe carefully, choose one valid Craftax action, then reassess."
                    }
                }
            }"#,
        )
        .unwrap();
        let (run, _) = svc
            .create(
                serde_json::from_value(json!({
                    "algorithmId": "gepa",
                    "id": "craftax_result_run",
                    "openVisual": false
                }))
                .unwrap(),
            )
            .await
            .unwrap();
        let mut stored = run.clone();
        stored.status = "completed".into();
        stored.summary = json!({
            "recipeId": "gepa.craftax.smoke.v1",
            "runDirectory": run_dir.display().to_string(),
            "selection": {"score": 0.4},
            "heldout": {"score": 0.3}
        });
        svc.persist_run(stored).await.unwrap();
        let result = svc.get_result(run.id).await.unwrap();
        assert_eq!(result["schemaVersion"], json!("optimizer_result.v1"));
        assert_eq!(
            result["selectedCandidate"]["materializedValues"]["prompt"],
            json!("Observe carefully, choose one valid Craftax action, then reassess.")
        );
        assert!(result["selectedCandidate"]["materializedDigest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"));
        let encoded = result.to_string();
        assert!(!encoded.contains("best_candidate.json"));
        assert!(!encoded.contains("runDirectory"));
        assert!(!encoded.contains(&run_dir.display().to_string()));
    }

    #[tokio::test]
    async fn completed_result_without_prompt_fails_closed() {
        let (svc, dir, _) = service().await;
        let run_dir = dir.path().join("banking77_empty");
        std::fs::create_dir_all(&run_dir).unwrap();
        std::fs::write(
            run_dir.join("best_candidate.json"),
            r#"{"id":"candidate_empty"}"#,
        )
        .unwrap();
        let (run, _) = svc
            .create(
                serde_json::from_value(json!({
                    "algorithmId": "gepa",
                    "id": "banking77_empty_prompt",
                    "openVisual": false
                }))
                .unwrap(),
            )
            .await
            .unwrap();
        let mut stored = run.clone();
        stored.status = "completed".into();
        stored.summary = json!({ "runDirectory": run_dir.display().to_string() });
        svc.persist_run(stored).await.unwrap();
        let error = svc.get_result(run.id).await.unwrap_err();
        assert!(error.to_string().contains("materialized prompt"));
    }

    #[tokio::test]
    async fn sft_get_result_does_not_materialize_best_candidate_json() {
        let (svc, dir, _) = service().await;
        let run_dir = dir.path().join("sft_result");
        std::fs::create_dir_all(&run_dir).unwrap();
        std::fs::write(
            run_dir.join("best_candidate.json"),
            r#"{"candidate_id":"should_not_be_read","payload":{"prompt":"gepa shaped"}}"#,
        )
        .unwrap();
        let (run, _) = svc
            .create(
                serde_json::from_value(json!({
                    "algorithmId": "sft",
                    "id": "sft_typed_result_run",
                    "openVisual": false
                }))
                .unwrap(),
            )
            .await
            .unwrap();
        let mut stored = run.clone();
        stored.status = "completed".into();
        stored.cursor_seq = 4;
        stored.summary = json!({
            "runDirectory": run_dir.display().to_string(),
            "terminalCursor": 3
        });
        svc.persist_run(stored).await.unwrap();
        let result = svc.get_result(run.id).await.unwrap();
        assert_eq!(result["schemaVersion"], json!("sft_result.v1"));
        assert_eq!(result["algorithmId"], json!("sft"));
        assert_eq!(result["pending"], json!(true));
        assert_eq!(
            result["error"]["code"],
            json!("sft_result_materialization_pending")
        );
        assert_eq!(result["finalCursor"], json!(3));
        assert_eq!(result["enrichmentCursor"], json!(4));
        assert_eq!(result["usage"]["authority"], json!("manifest"));
        let encoded = result.to_string();
        assert!(!encoded.contains("best_candidate.json"));
        assert!(!encoded.contains("should_not_be_read"));
        assert!(!encoded.contains("gepa shaped"));
    }

    #[tokio::test]
    async fn freeze_terminal_cursor_survives_late_enrichment() {
        let (svc, _dir, _) = service().await;
        let (run, _) = svc
            .create(
                serde_json::from_value(json!({
                    "algorithmId": "gepa",
                    "id": "terminal_cursor_freeze",
                    "openVisual": false
                }))
                .unwrap(),
            )
            .await
            .unwrap();
        let mut stored = run.clone();
        stored.status = "completed".into();
        stored.cursor_seq = 8;
        stored.usage.cost_usd = Some(1.25);
        stored.summary = json!({"terminalCursor": 8});
        svc.persist_run(stored).await.unwrap();
        let mut enriched = svc.get(run.id.clone()).await.unwrap();
        enriched.cursor_seq = 11;
        enriched.usage.cost_usd = Some(0.01);
        if let Some(summary) = enriched.summary.as_object_mut() {
            summary.insert("enrichmentNote".into(), json!("late"));
        }
        let persisted = svc.persist_run(enriched).await.unwrap();
        assert_eq!(persisted.summary["terminalCursor"], json!(8));
        assert_eq!(persisted.summary["terminalUsage"]["costUsd"], json!(1.25));
        let result = svc.get_result(run.id).await;
        // No GEPA artifacts: this path is about cursor/usage authority, not prompt materialization.
        if let Ok(result) = result {
            assert_eq!(result["finalCursor"], json!(8));
            assert_eq!(result["enrichmentCursor"], json!(11));
            assert_eq!(result["usage"]["ledger"]["costUsd"], json!(1.25));
            assert_eq!(result["usage"]["authority"], json!("manifest"));
        }
        let reread = svc.get("terminal_cursor_freeze".into()).await.unwrap();
        assert_eq!(reread.summary["terminalCursor"], json!(8));
        assert_eq!(reread.summary["terminalUsage"]["costUsd"], json!(1.25));
        assert_eq!(reread.cursor_seq, 11);
    }

    #[tokio::test]
    async fn optimizer_create_attaches_the_session_experiment_group() {
        let (svc, _dir, _) = service().await;
        svc.create(
            serde_json::from_value(json!({
                "algorithmId": "gepa",
                "id": "opt_exp_1",
                "sessionRef": "session_exp",
                "openVisual": false
            }))
            .unwrap(),
        )
        .await
        .unwrap();
        let group = svc
            .db
            .run(|conn| crate::experiments::load_for_session(conn, "session_exp"))
            .await
            .unwrap()
            .expect("optimizer create owns an experiment group");
        assert_eq!(group.session_id, "session_exp");
        assert_eq!(group.members.len(), 1);
        assert_eq!(group.members[0].member_id, "opt_exp_1");
        assert_eq!(
            group.members[0].member_kind,
            crate::experiments::MEMBER_OPTIMIZER
        );
    }
}
