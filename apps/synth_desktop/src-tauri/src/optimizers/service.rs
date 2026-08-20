use super::events::{plan_batch, EventVerdict, OptimizerEventDraft, SequenceContract};
use super::models::{
    OptimizerCapabilities, OptimizerCreateRequest, OptimizerEventEnvelope, OptimizerQuery,
    OptimizerRelationship, OptimizerResourceRef, OptimizerRunRecord, OptimizerStateSlice,
    OptimizerUsageSummary, OPTIMIZER_EVENT_SCHEMA_VERSION, OPTIMIZER_RUN_SCHEMA_VERSION,
    OPTIMIZER_STATE_SLICE_SCHEMA_VERSION,
};
use super::results;
use super::terminal;
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

/// Turn the assembled catalog into one authoritative admission answer. Source
/// catalogs still own their recipes; this projection makes their independent
/// asset/runtime/contract failures comparable so callers never need to infer
/// readiness by joining several MCP responses themselves.
fn project_recipe_readiness(mut recipe: Value) -> Value {
    let id = recipe
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let algorithm = recipe
        .get("algorithmId")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let mut blockers = Vec::new();

    if id.is_empty() {
        blockers.push(recipe_blocker(
            "recipe_id_missing",
            "recipe.id",
            "Optimizers",
            "The recipe has no stable id.",
            false,
        ));
    }
    if algorithm.is_empty() {
        blockers.push(recipe_blocker(
            "algorithm_id_missing",
            "recipe.algorithmId",
            "Optimizers",
            "The recipe has no algorithm owner.",
            false,
        ));
    }

    if recipe.get("availability").and_then(Value::as_str) != Some("available") {
        let detail = recipe
            .get("availabilityReason")
            .and_then(Value::as_str)
            .filter(|reason| !reason.trim().is_empty())
            .unwrap_or(
                "A required runtime, service, credential, or packaged asset is unavailable.",
            );
        let (code, contract, owner) = if detail.contains("cookbook") {
            ("cookbook_unavailable", "assets.cookbook", "Optimizers")
        } else if detail.contains("runtime is not installed") {
            ("runtime_unavailable", "runtime.local", "Optimizers")
        } else {
            (
                "dependency_unavailable",
                "recipe.dependencies",
                "Optimizers",
            )
        };
        blockers.push(recipe_blocker(code, contract, owner, detail, true));
    }

    if super::eval_recipes::is_eval_recipe(&id)
        && recipe.get("availability").and_then(Value::as_str) == Some("available")
        && recipe
            .pointer("/limits/trials")
            .and_then(Value::as_u64)
            .filter(|trials| *trials > 0)
            .is_none()
    {
        blockers.push(recipe_blocker(
            "trial_limit_missing",
            "limits.trials",
            "Optimizers",
            "The eval recipe must publish a positive per-candidate trial count.",
            false,
        ));
    }

    if id.starts_with("gepa.")
        && recipe.get("availability").and_then(Value::as_str) == Some("available")
        && recipe
            .pointer("/limits/maxTotalRollouts")
            .and_then(Value::as_u64)
            .filter(|rollouts| *rollouts > 0)
            .is_none()
    {
        blockers.push(recipe_blocker(
            "rollout_limit_missing",
            "limits.maxTotalRollouts",
            "Optimizers",
            "The GEPA recipe must publish a positive total rollout ceiling.",
            false,
        ));
    }

    let ready = blockers.is_empty();
    if let Some(object) = recipe.as_object_mut() {
        if !ready {
            object.insert("availability".into(), json!("unavailable"));
            if object
                .get("availabilityReason")
                .and_then(Value::as_str)
                .map_or(true, |reason| reason.trim().is_empty())
            {
                object.insert("availabilityReason".into(), blockers[0]["message"].clone());
            }
        }
        object.insert(
            "readiness".into(),
            json!({
                "ready": ready,
                "status": if ready { "ready" } else { "blocked" },
                "blockers": blockers,
            }),
        );
    }
    recipe
}

fn recipe_blocker(
    code: &str,
    contract: &str,
    owner: &str,
    message: &str,
    retryable: bool,
) -> Value {
    json!({
        "code": code,
        "contract": contract,
        "owner": owner,
        "message": message,
        "retryable": retryable,
    })
}

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

/// One chat-owned artifact publication: mint-or-reuse, bind, show, select, and
/// shelve, under a single run identity. See
/// [`OptimizerService::publish_chat_owned_visual`].
#[derive(Clone)]
pub(super) struct ChatVisualPublication {
    pub run_id: String,
    /// The conversation that owns the artifact. `None` falls back to the run's
    /// own `session_ref`; it never means "whichever chat is focused".
    pub session_ref: Option<String>,
    pub template_id: String,
    pub title: String,
    pub bindings: Value,
    pub metadata: Value,
    pub status: crate::visuals::VisualStatus,
    /// Which visual of the run this is. Reuse is keyed on it, so a run may own a
    /// `primary` pane and, later, a distinct report without either replacing the
    /// other.
    pub role: String,
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
    /// Publish a durable visual event produced by an internal optimizer worker.
    ///
    /// MCP-driven visual updates return their event to the caller, which then
    /// reaches the renderer through the normal request lane. Local recipe
    /// workers have no caller to do that forwarding, so they must place the
    /// already-durable event on the shared bus themselves.
    pub(super) fn publish_visual_event(&self, value: Value) -> Result<()> {
        let event: AppEvent = serde_json::from_value(value)
            .context("optimizer visual update returned an invalid app event")?;
        let _ = self.events_tx.send(event);
        Ok(())
    }

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

    pub(super) fn database(&self) -> &Arc<Database> {
        &self.db
    }

    pub(super) fn visuals(&self) -> &VisualRegistry {
        &self.visuals
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
            json!({"id":"sft","title":"SFT","kind":"training","availability":"available","description":"Supervised fine-tuning. Local MLX on this Mac or hosted through the public Optimizers SFT service. Both placements are admitted by the Optimizers sidecar."}),
            json!({"id":"cispo","title":"CISPO","kind":"training","availability":"available","description":"On-policy CISPO. Local MLX on this Mac, or hosted slime.v1 after the clip-identity canary. Distinct from GEPA/GELO search."}),
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
        recipes.push(super::mlx_sft::recipe_catalog());
        recipes.extend(super::cispo::recipe_catalog());
        // Local eval is the authority for eval.* admission. Older sidecar
        // catalogs can carry compatibility copies of the same ids without the
        // product-owned bounds; a plain concatenation made callers select the
        // stale first copy and reject an otherwise valid recipe.
        recipes.extend(super::eval_recipes::recipe_catalog());
        let mut by_id = BTreeMap::new();
        let mut anonymous = Vec::new();
        for recipe in recipes {
            if let Some(id) = recipe.get("id").and_then(Value::as_str) {
                by_id.insert(id.to_string(), recipe);
            } else {
                anonymous.push(recipe);
            }
        }
        anonymous
            .into_iter()
            .chain(by_id.into_values())
            .map(project_recipe_readiness)
            .collect()
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
            super::recipes::BANKING77_EVAL_BASELINE_RECIPE
            | super::recipes::HEALTHBENCH_EVAL_SMOKE_RECIPE => {
                super::recipes::start_container_eval(self, request).await
            }
            super::sft_recipes::CRAFTAX_SFT_SMOKE_RECIPE => {
                super::sft_recipes::start(self, request).await
            }
            super::hosted_gelo::HOSTED_GELO_CRAFTAX_RECIPE => {
                super::hosted_gelo::start(self, request).await
            }
            super::hosted_sft::HOSTED_SFT_CRAFTAX_NEMOTRON_RECIPE
            | super::hosted_sft::HOSTED_SFT_BANKING77_RECIPE => {
                super::hosted_sft::start(self, request).await
            }
            super::mlx_sft::QWEN_MLX_SFT_RECIPE => super::mlx_sft::start(self, request).await,
            super::sidecar_training::LOCAL_MLX_CISPO_RECIPE
            | super::sidecar_training::HOSTED_CISPO_RECIPE => {
                super::cispo::start(self, request).await
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

    /// The run's typed result, dispatched on its authoritative `algorithm_id`.
    ///
    /// Never on which files happen to exist on disk: reading a baseline eval
    /// through GEPA's candidate-materialization path is what answered a
    /// successful 10/10 campaign with "completed GEPA result omitted a
    /// materialized prompt".
    pub async fn get_result(&self, optimizer_run_id: String) -> Result<Value> {
        let run = self.get(optimizer_run_id.clone()).await?;
        let manifest = self.terminal_manifest(optimizer_run_id.clone()).await?;
        if let Some(existing) = run.summary.get("optimizerResult").cloned() {
            let fresh = existing.get("schemaVersion").and_then(Value::as_str)
                == Some(results::RESULT_SCHEMA_VERSION)
                && existing.get("resultKind").and_then(Value::as_str)
                    == Some(results::result_kind(&run.algorithm_id))
                // A result cached before the run settled is a live reading. Once
                // a manifest exists, the answer has to come from it.
                && (manifest.is_none()
                    || existing
                        .get("terminalManifest")
                        .is_some_and(|value| !value.is_null()))
                && (!results::materializes_candidate(&run.algorithm_id)
                    || existing
                        .pointer("/metrics/heldoutMeasurement")
                        .is_some_and(|value| !value.is_null()));
            if fresh {
                return Ok(existing);
            }
        }
        let manifest_ref = manifest.as_ref();
        let result = if results::materializes_candidate(&run.algorithm_id) {
            let materialized = materialize_optimizer_result(self, &run).await?;
            merge_typed_envelope(materialized, &run, manifest_ref)
        } else {
            match run.algorithm_id.as_str() {
                "eval" => results::eval_result(&run, manifest_ref)?,
                "sft" => results::sft_result(&run, manifest_ref)?,
                "environment" => results::environment_result(&run, manifest_ref)?,
                _ => results::generic_result(&run, manifest_ref)?,
            }
        };
        let result = match manifest_ref {
            Some(manifest) => results::with_manifest(result, manifest),
            None => result,
        };
        let stored = result.clone();
        self.patch_run(optimizer_run_id, move |run| {
            let mut summary = run.summary.as_object().cloned().unwrap_or_default();
            summary.insert("optimizerResult".into(), stored);
            run.summary = Value::Object(summary);
            Ok(())
        })
        .await?;
        Ok(result)
    }

    pub(super) async fn register_local_recipe(&self, run_id: String, cancel: watch::Sender<bool>) {
        self.local_recipes.lock().await.insert(run_id, cancel);
    }

    pub(super) async fn unregister_local_recipe(&self, run_id: &str) {
        self.local_recipes.lock().await.remove(run_id);
    }

    pub(super) async fn registered_local_recipes(&self) -> std::collections::HashSet<String> {
        self.local_recipes.lock().await.keys().cloned().collect()
    }

    pub async fn restore_hosted_sft_mirrors(&self) {
        super::hosted_sft::restore_hosted_mirrors(self).await;
        super::mlx_sft::restore_mirrors(self).await;
        super::cispo::restore_mirrors(self).await;
    }

    pub async fn wait_milestone(
        &self,
        optimizer_run_id: String,
        after_seq: u64,
        kinds: Vec<String>,
        timeout_ms: u64,
    ) -> Result<Value> {
        let deadline =
            tokio::time::Instant::now() + std::time::Duration::from_millis(timeout_ms.max(50));
        let mut cursor = after_seq;
        loop {
            let events = self
                .events_after(optimizer_run_id.clone(), cursor, Some(500))
                .await?;
            for event in events {
                cursor = cursor.max(event.sequence_number);
                if let Some(kind) =
                    super::sft_result::sft_milestone_kind(&event.event_type, event.level.as_deref())
                {
                    if kinds.is_empty() || kinds.iter().any(|wanted| wanted == kind) {
                        return Ok(json!({
                            "milestone": kind,
                            "event": event,
                            "cursor": cursor,
                            "timedOut": false
                        }));
                    }
                }
            }
            let run = self.get(optimizer_run_id.clone()).await?;
            if matches!(
                run.status.as_str(),
                "completed" | "failed" | "cancelled" | "canceled"
            ) {
                return Ok(json!({
                    "milestone": "terminal",
                    "status": run.status,
                    "cursor": run.cursor_seq,
                    "timedOut": false
                }));
            }
            if tokio::time::Instant::now() >= deadline {
                bail!("no matching milestone for `{optimizer_run_id}` after sequence {after_seq}");
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    /// Persist a caller-mutated run record without letting it rewind lifecycle.
    ///
    /// Callers read a record, change one field, and write it back — a pattern
    /// that is only safe while nothing else is writing. Workers, admission, and
    /// event appends all write concurrently, and a snapshot taken before the
    /// first event would otherwise restore its `cursor_seq`, un-finish the run,
    /// and drop the visual it had since published. The event stream owns
    /// lifecycle; this merges the caller's fields over the durable one.
    pub(super) async fn persist_run(&self, run: OptimizerRunRecord) -> Result<OptimizerRunRecord> {
        let db = self.db.clone();
        db.run_transaction(move |conn| {
            let mut stored = run;
            preserve_durable_authority(conn, &mut stored)?;
            upsert_run(conn, &stored)?;
            Ok(stored)
        })
        .await
    }

    /// Mutate a run under the durable record, inside one transaction. Preferred
    /// over `persist_run` for new code: the closure never sees a stale snapshot.
    pub(super) async fn patch_run<F>(
        &self,
        optimizer_run_id: String,
        patch: F,
    ) -> Result<OptimizerRunRecord>
    where
        F: FnOnce(&mut OptimizerRunRecord) -> Result<()> + Send + 'static,
    {
        let db = self.db.clone();
        db.run_transaction(move |conn| {
            let mut run = load_run(conn, &optimizer_run_id)?;
            patch(&mut run)?;
            preserve_durable_authority(conn, &mut run)?;
            upsert_run(conn, &run)?;
            Ok(run)
        })
        .await
    }

    pub(crate) async fn attach_paid_compute_approval(
        &self,
        optimizer_run_id: String,
        approval_id: &str,
        max_cost_usd_micros: Option<u64>,
        max_rollouts: Option<u64>,
    ) -> Result<OptimizerRunRecord> {
        let approval_id = approval_id.to_string();
        let db = self.db.clone();
        db.run_transaction(move |conn| {
            // Admission may start a very fast worker before the approval
            // receipt is attached. Always patch the current durable record;
            // persisting the pre-start return value can rewind cursor_seq and
            // erase streamed progress that arrived in the meantime.
            let mut run = load_run(conn, &optimizer_run_id)?;
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
            upsert_run(conn, &run)?;
            Ok(run)
        })
        .await
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
        // An eval worker persists its own event stream. On restart the host may
        // still say `running` even though that log already contains the
        // terminal event; reconcile before deciding that a local run is live.
        if run.source == "local" && run.algorithm_id == super::eval_recipes::EVAL_ALGORITHM_ID {
            run = super::eval_recipes::reconcile_persisted(self, &optimizer_run_id).await?;
        }
        if run.source == "local"
            && run.summary.get("recipeId").and_then(Value::as_str)
                == Some(super::mlx_sft::QWEN_MLX_SFT_RECIPE)
        {
            run = super::mlx_sft::reconcile(self, &optimizer_run_id).await?;
        }
        if run.summary.get("recipeId").and_then(Value::as_str)
            == Some(super::hosted_gelo::HOSTED_GELO_CRAFTAX_RECIPE)
        {
            run = super::hosted_gelo::reconcile_persisted(self, &optimizer_run_id).await?;
        }
        if run.source == "local"
            && matches!(run.status.as_str(), "completed" | "failed" | "cancelled")
        {
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

    /// After a process restart, locally persisted `running`/`queued`/`paused`
    /// projections can be a lie. Walk them and let each algorithm's durable
    /// worker log win before the renderer hydrates Outputs.
    pub async fn reconcile_stale_local_runs(&self) -> Result<Vec<OptimizerRunRecord>> {
        let runs = self
            .list(OptimizerQuery {
                limit: Some(500),
                ..OptimizerQuery::default()
            })
            .await?;
        let mut recovered = Vec::new();
        for run in runs {
            if run.source != "local" || is_terminal_status(&run.status) {
                continue;
            }
            match self.refresh(run.id.clone()).await {
                Ok(next) => recovered.push(next),
                Err(error) => eprintln!(
                    "synth-desktop: failed to reconcile optimizer run {}: {error:#}",
                    run.id
                ),
            }
        }
        Ok(recovered)
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

    /// Append events whose sequence numbers mirror an external log (a hosted
    /// campaign, an imported sidecar feed, a cloud reconcile).
    ///
    /// The batch is validated as a whole before a single row is written, and no
    /// event is ever silently dropped: an event that collides with a different
    /// event already durable at its sequence is an error the producer must see.
    pub async fn append_events(
        &self,
        optimizer_run_id: String,
        events: Vec<OptimizerEventEnvelope>,
    ) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
        self.commit_events(optimizer_run_id, events, SequenceContract::Mirrored)
            .await
    }

    /// Append event *content*. The service owns identity and order.
    ///
    /// This is the only append a local worker should use. Sequence numbers are
    /// allocated inside the transaction that inserts them, so two workers — or
    /// one worker racing a `persist_run` — cannot compute the same number from
    /// two stale snapshots and lose an event to a unique-index collision.
    pub(super) async fn append_event_payloads(
        &self,
        optimizer_run_id: String,
        drafts: Vec<OptimizerEventDraft>,
    ) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
        if drafts.is_empty() {
            let run = self.get(optimizer_run_id).await?;
            return Ok((run, None));
        }
        let db = self.db.clone();
        let events_tx = self.events_tx.clone();
        let result = db
            .run_transaction(move |conn| {
                let run = load_run(conn, &optimizer_run_id)?;
                let sealed_at = Utc::now().to_rfc3339();
                // Allocate above both the cursor and the highest durable event.
                // They can disagree when an older build rewound one of them; the
                // higher of the two is the only safe floor.
                let mut next = run.cursor_seq.max(max_event_sequence(conn, &run.id)?);
                let mut envelopes = Vec::with_capacity(drafts.len());
                for draft in drafts {
                    // An idempotent draft that is already durable seals to
                    // nothing: re-offering a settlement must not mint a second
                    // sequence for the same fact.
                    if let Some(key) = draft.idempotency_key.as_deref() {
                        if event_id_exists(conn, &run.id, &format!("{}:{key}", run.id))? {
                            continue;
                        }
                    }
                    next += 1;
                    envelopes.push(draft.seal(&run.id, next, &sealed_at));
                }
                if envelopes.is_empty() {
                    return Ok((run, None));
                }
                commit_validated_events(conn, run, envelopes, SequenceContract::ServiceAllocated)
            })
            .await?;
        if let Some(event) = &result.1 {
            let _ = events_tx.send(event.clone());
        }
        Ok(result)
    }

    async fn commit_events(
        &self,
        optimizer_run_id: String,
        events: Vec<OptimizerEventEnvelope>,
        contract: SequenceContract,
    ) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
        if events.is_empty() {
            let run = self.get(optimizer_run_id).await?;
            return Ok((run, None));
        }
        let db = self.db.clone();
        let result = db
            .run_transaction(move |conn| {
                let run = load_run(conn, &optimizer_run_id)?;
                commit_validated_events(conn, run, events, contract)
            })
            .await?;
        if let Some(event) = &result.1 {
            let _ = self.events_tx.send(event.clone());
        }
        Ok(result)
    }

    /// Create-or-reuse a run's chat-owned visual, bind it to the run, publish
    /// the durable `visual.show`, and select it for the owning conversation —
    /// as one operation with one identity.
    ///
    /// These used to be five calls in three files, each able to succeed alone:
    /// a visual could be minted and never bound, bound and never shown, shown
    /// into whichever chat happened to be focused, or lost from Outputs after a
    /// restart because only the pane knew about it. Repeating this is safe: an
    /// existing primary visual is reused, never duplicated, and the show is
    /// addressed to the run's own session so a second call cannot move another
    /// chat's pane.
    pub(super) async fn publish_chat_owned_visual(
        &self,
        request: ChatVisualPublication,
    ) -> Result<(String, Option<AppEvent>)> {
        let run = self.get(request.run_id.clone()).await?;
        let session_ref = request
            .session_ref
            .clone()
            .or_else(|| run.session_ref.clone());
        let existing = run
            .visual_refs
            .iter()
            .find(|reference| {
                reference.kind == "visual"
                    && reference.role.as_deref() == Some(request.role.as_str())
            })
            .map(|reference| reference.id.clone());

        let visual_id = match existing {
            Some(visual_id) => visual_id,
            None => {
                let (created, _) = self
                    .visuals
                    .create(VisualCreateRequest {
                        template_id: request.template_id.clone(),
                        title: Some(request.title.clone()),
                        bindings: Some(request.bindings.clone()),
                        id: None,
                        status: Some(request.status),
                        renderer_kind: None,
                        session_id: session_ref.clone(),
                        message_id: None,
                        run_id: None,
                        trace_id: None,
                        parent_visual_id: None,
                        source_agent_id: None,
                        source_model: None,
                        content: None,
                        metadata: Some(request.metadata.clone()),
                    })
                    .await
                    .context("create chat-owned optimizer visual")?;
                created.id
            }
        };

        // Bind before showing. A visual the renderer opens but the run does not
        // reference is a visual that disappears on restart.
        let bound_id = visual_id.clone();
        let template_id = request.template_id.clone();
        let title = request.title.clone();
        let role = request.role.clone();
        let bind = self
            .patch_run(request.run_id.clone(), move |run| {
                if !run
                    .visual_refs
                    .iter()
                    .any(|reference| reference.id == bound_id)
                {
                    run.visual_refs.push(OptimizerResourceRef {
                        kind: "visual".into(),
                        id: bound_id.clone(),
                        digest: None,
                        role: Some(role),
                        title: Some(title),
                        metadata: json!({ "templateId": template_id }),
                    });
                }
                let mut summary = run.summary.as_object().cloned().unwrap_or_default();
                summary.insert("visualId".into(), json!(bound_id));
                summary.remove("visualProjectionError");
                run.summary = Value::Object(summary);
                Ok(())
            })
            .await;
        if let Err(error) = bind {
            self.record_visual_projection_error(&request.run_id, &format!("{error:#}"))
                .await;
            return Err(error.context("bind chat-owned optimizer visual"));
        }

        match self.visuals.show(visual_id.clone(), session_ref).await {
            Ok((_, event)) => Ok((visual_id, serde_json::from_value::<AppEvent>(event).ok())),
            Err(error) => {
                // The visual exists and is bound, but nothing told the chat about
                // it. Say so durably rather than reporting an opened output.
                self.record_visual_projection_error(&request.run_id, &format!("{error:#}"))
                    .await;
                Err(error.context("publish chat-owned optimizer visual"))
            }
        }
    }

    async fn record_visual_projection_error(&self, run_id: &str, message: &str) {
        let message = message.to_string();
        let _ = self
            .patch_run(run_id.to_string(), move |run| {
                let mut summary = run.summary.as_object().cloned().unwrap_or_default();
                summary.insert("visualProjectionError".into(), json!(message));
                run.summary = Value::Object(summary);
                Ok(())
            })
            .await;
    }

    /// The sealed terminal manifest for a run, or `None` while it is live.
    pub async fn terminal_manifest(&self, optimizer_run_id: String) -> Result<Option<Value>> {
        let db = self.db.clone();
        db.run(move |conn| terminal::load(conn, &optimizer_run_id))
            .await
    }

    /// Settle a run whose compute succeeded but whose evidence did not.
    ///
    /// The successful compute records are preserved and the failure is durable
    /// and named, because the alternative — reporting a clean `completed` for a
    /// run with no evidence — is the exact defect this lane exists to remove.
    /// The run becomes terminal in the `degraded` state, which is retryable
    /// without re-running paid compute.
    pub(super) async fn settle_evidence_degraded(
        &self,
        optimizer_run_id: String,
        stage: &str,
        reason: String,
    ) -> Result<OptimizerRunRecord> {
        let stage = stage.to_string();
        let db = self.db.clone();
        let events_tx = self.events_tx.clone();
        let (run, app_event) = db
            .run_transaction(move |conn| {
                let mut run = load_run(conn, &optimizer_run_id)?;
                let degradation = json!({
                    "stage": stage,
                    "reason": reason,
                    "observedAt": Utc::now().to_rfc3339(),
                    "retryable": true,
                    "paidComputePreserved": true,
                });
                if terminal::load(conn, &run.id)?.is_some() {
                    // Already settled: a degradation discovered afterwards is new
                    // information about a sealed run, not a new ending.
                    terminal::amend_degradation(conn, &run.id, degradation)?;
                    return Ok((run, None));
                }
                run.status = "degraded".into();
                run.finished_at = Some(Utc::now().to_rfc3339());
                if let Some(object) = run.summary.as_object_mut() {
                    object.insert("evidenceDegradation".into(), degradation.clone());
                }
                run.error = Some(degradation.clone());
                upsert_run(conn, &run)?;
                let events = load_events_upto(conn, &run.id, run.cursor_seq)?;
                let manifest = terminal::derive(
                    &run,
                    &events,
                    terminal::STATUS_FAILED_EVIDENCE,
                    Some(degradation),
                );
                let sealed = terminal::seal(conn, &run.id, &manifest)?;
                if let Some(object) = run.summary.as_object_mut() {
                    object.insert("terminalManifest".into(), sealed);
                }
                upsert_run(conn, &run)?;
                let app_event = append_event(
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
                            "cursorSeq": run.cursor_seq,
                        }),
                        remote_sequence: None,
                        command_id: None,
                        created_at: None,
                    },
                )?;
                Ok((run, Some(app_event)))
            })
            .await?;
        if let Some(event) = app_event {
            let _ = events_tx.send(event);
        }
        Ok(run)
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
            if matches!(run.algorithm_id.as_str(), "sft" | "cispo") {
                if let Ok(client) =
                    super::sidecar_training::SidecarTrainingClient::from_manager(self.manager())
                        .await
                {
                    let _ = client.cancel(&id).await;
                    return self.command(id, "cancel", "cancelled").await;
                }
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
        if matches!(run.algorithm_id.as_str(), "sft" | "cispo") {
            if let Ok(client) =
                super::sidecar_training::SidecarTrainingClient::from_manager(self.manager()).await
            {
                let _ = client.resume(&id).await;
            }
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
        let title = format!(
            "{} · {}",
            algorithm_label(&run.algorithm_id),
            run.objective.clone().unwrap_or_else(|| run.id.clone())
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
            execution_bindings: imported.execution_bindings.clone(),
            input_refs: {
                let mut refs = imported.input_refs.clone();
                refs.push(OptimizerResourceRef {
                    kind: "local_path".into(),
                    id: imported.source_path.display().to_string(),
                    digest: None,
                    role: Some("event_feed".into()),
                    title: Some("Local optimizer workspace".into()),
                    metadata: json!({}),
                });
                refs
            },
            output_refs: imported.output_refs.clone(),
            visual_refs: vec![],
            summary: {
                let mut summary = imported.summary.as_object().cloned().unwrap_or_default();
                summary.insert(
                    "importedFrom".into(),
                    json!(imported.source_path.display().to_string()),
                );
                Value::Object(summary)
            },
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

    pub async fn search_saved_lora_checkpoints(
        &self,
        query: super::SavedLoraCheckpointQuery,
    ) -> Result<super::SavedLoraCheckpointPage> {
        super::cloud::CloudOptimizerClient::from_config()?
            .search_saved_lora_checkpoints(query)
            .await
    }

    pub async fn list_saved_lora_checkpoints_for_run(
        &self,
        run_id: String,
    ) -> Result<super::SavedLoraRunPage> {
        super::cloud::CloudOptimizerClient::from_config()?
            .saved_lora_checkpoints_for_run(&run_id)
            .await
    }

    pub async fn run_outputs(&self, run_id: String) -> Result<super::OptimizerRunOutputs> {
        super::cloud::CloudOptimizerClient::from_config()?
            .run_outputs(&run_id)
            .await
    }

    pub async fn hosted_training_models(&self) -> Result<super::HostedTrainingModelCatalog> {
        super::cloud::CloudOptimizerClient::from_config()?
            .hosted_training_models()
            .await
    }

    pub async fn archive_saved_lora_checkpoint(
        &self,
        checkpoint_id: String,
    ) -> Result<super::SavedLoraCheckpoint> {
        super::cloud::CloudOptimizerClient::from_config()?
            .archive_saved_lora_checkpoint(&checkpoint_id)
            .await
    }

    pub async fn saved_lora_download(
        &self,
        checkpoint_id: String,
    ) -> Result<super::SavedLoraDownload> {
        super::cloud::CloudOptimizerClient::from_config()?
            .saved_lora_download(&checkpoint_id)
            .await
    }

    /// Persist the canonical hosted-training projection independently of the
    /// legacy optimizer-event cursor.  The backend training ledger is the
    /// authority for CISPO/PPO progress, checkpoint readiness and terminal
    /// truth; keeping its sequence in the durable run summary makes Workshop
    /// reconnect from the same cursor after an app restart.
    pub async fn reconcile_training(&self, optimizer_run_id: String) -> Result<Value> {
        let run = self.get(optimizer_run_id.clone()).await?;
        if !matches!(run.algorithm_id.as_str(), "sft" | "cispo" | "ppo") {
            bail!(
                "canonical training replay is unavailable for algorithm {}",
                run.algorithm_id
            );
        }
        if run.source != "cloud" {
            bail!("canonical training replay requires a cloud run");
        }
        let mut projection: super::TrainingProjection = run
            .summary
            .get("trainingProjection")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .context("decode persisted training projection")?
            .unwrap_or_default();
        let client = super::cloud::CloudOptimizerClient::from_config()?;
        loop {
            let events = client
                .training_events_after(&optimizer_run_id, projection.last_sequence, Some(5000))
                .await?;
            if events.is_empty() {
                break;
            }
            let previous = projection.last_sequence;
            for event in events {
                projection.apply(&event).map_err(anyhow::Error::msg)?;
            }
            if projection.last_sequence == previous {
                break;
            }
        }
        let persisted_projection = serde_json::to_value(&projection)?;
        let result = json!({
            "schemaVersion": "workshop.training_snapshot.v1",
            "runId": optimizer_run_id,
            "projection": persisted_projection,
        });
        let result_for_store = result.clone();
        let lifecycle = projection.lifecycle;
        let provider_usage = projection.provider_usage.clone();
        self.patch_run(optimizer_run_id, move |run| {
            let mut summary = run.summary.as_object().cloned().unwrap_or_default();
            summary.insert(
                "trainingProjection".into(),
                result_for_store["projection"].clone(),
            );
            summary.insert(
                "trainingEventSequence".into(),
                json!(projection.last_sequence),
            );
            if let Some(usage) = provider_usage.as_ref() {
                if let Some(cost) = usage
                    .get("provider_cost_usd")
                    .or_else(|| usage.get("estimated_cost_usd"))
                    .and_then(Value::as_f64)
                {
                    run.usage.cost_usd = Some(cost);
                }
                run.usage.prompt_tokens = usage
                    .get("prompt_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(run.usage.prompt_tokens);
                run.usage.completion_tokens = usage
                    .get("completion_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(run.usage.completion_tokens);
            }
            run.status = training_lifecycle_status(lifecycle).into();
            run.capabilities = OptimizerCapabilities::for_algorithm(&run.algorithm_id);
            Ok(())
        })
        .await?;
        Ok(result)
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

fn training_lifecycle_status(lifecycle: super::TrainingLifecycle) -> &'static str {
    use super::TrainingLifecycle as L;
    match lifecycle {
        L::Draft | L::Validating => "validating",
        L::Queued => "queued",
        L::Provisioning => "provisioning",
        L::Running | L::Checkpointing | L::Evaluating => "running",
        L::EnvUnreachable => "env_unreachable",
        L::Cancelling => "cancelling",
        L::Cancelled => "cancelled",
        L::Paused => "paused",
        L::Completed => "completed",
        L::Degraded => "degraded",
        L::FailedEvidence => "failed_evidence",
        L::Failed => "failed",
        L::InfrastructureLost => "infrastructure_lost",
        L::CapReached => "cap_reached",
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
        "sft" | "cispo" => "optimizer.sft.live.v1",
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

/// Give a GEPA-materialized result the same typed envelope every other
/// algorithm gets, so callers can branch on `resultKind` instead of guessing
/// from which keys happen to be present.
fn merge_typed_envelope(
    materialized: Value,
    run: &OptimizerRunRecord,
    manifest: Option<&Value>,
) -> Value {
    let mut out = results::envelope(run, manifest);
    if let Some(object) = materialized.as_object() {
        for (key, value) in object {
            // The envelope owns identity, status, cursor, and usage; the
            // materializer owns the candidate and its measurements.
            if matches!(
                key.as_str(),
                "schemaVersion"
                    | "resultKind"
                    | "optimizerRunId"
                    | "algorithmId"
                    | "status"
                    | "finalCursor"
                    | "usage"
                    | "completionReceiptId"
            ) {
                continue;
            }
            out.insert(key.clone(), value.clone());
        }
    }
    Value::Object(out)
}

async fn materialize_optimizer_result(
    service: &OptimizerService,
    run: &OptimizerRunRecord,
) -> Result<Value> {
    // Results are algorithm nouns, not a GEPA filesystem convention. Baseline
    // evals, SFT jobs, and environment campaigns must remain readable without
    // inventing a selected prompt or a `best_candidate.json` artifact.
    if run.algorithm_id != "gepa" {
        let manifest = service.terminal_manifest(run.id.clone()).await?;
        let result = match run.algorithm_id.as_str() {
            "eval" => super::results::eval_result(run, manifest.as_ref())?,
            "sft" => super::results::sft_result(run, manifest.as_ref())?,
            "environment" => super::results::environment_result(run, manifest.as_ref())?,
            _ => super::results::generic_result(run, manifest.as_ref())?,
        };
        return Ok(match manifest.as_ref() {
            Some(manifest) => super::results::with_manifest(result, manifest),
            None => result,
        });
    }
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
    if run.algorithm_id == "gepa"
        && run.status == "completed"
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
    // The sealed terminal manifest, not the producer's `result_manifest.json`.
    // The candidate file says what was selected; only the manifest says whether
    // selecting it improved anything, and a search that improved nothing is the
    // common case rather than the exception.
    let sealed = service.terminal_manifest(run.id.clone()).await?;
    let evidence = sealed
        .as_ref()
        .and_then(|sealed| sealed.get("gepaEvidence").cloned())
        .filter(|value| !value.is_null());
    let verdict = evidence
        .as_ref()
        .and_then(|evidence| evidence.get("verdict").cloned())
        .unwrap_or(Value::Null);
    // `frontierMember` is a fact about the search; it is not a recommendation.
    // Keeping the two apart is what stops a retained seed from reading as a
    // promotion.
    let deployment = evidence
        .as_ref()
        .and_then(|evidence| evidence.get("deployment").cloned())
        .unwrap_or_else(|| {
            json!({
                "candidateId": Value::Null,
                "recommended": false,
                "basis": "this run sealed no verdict; nothing is recommended for deployment",
            })
        });
    let mut result = json!({
        "schemaVersion": "optimizer_result.v1",
        "resultKind": "gepa_run_result.v1",
        "optimizerRunId": run.id,
        "algorithmId": run.algorithm_id,
        "status": run.status,
        "finalCursor": sealed
            .as_ref()
            .and_then(|sealed| sealed.get("terminalCursor").cloned())
            .unwrap_or(json!(run.cursor_seq)),
        "selectedCandidate": selected_candidate,
        "verdict": verdict,
        "deployment": deployment,
        "evidence": evidence.clone().unwrap_or(Value::Null),
        "metrics": {
            "selection": sealed
                .as_ref()
                .and_then(|sealed| sealed.get("selection").cloned())
                .filter(|value| !value.is_null())
                .unwrap_or(selection),
            "heldoutMeasurement": heldout
        },
        "usage": serde_json::to_value(&run.usage).unwrap_or(Value::Null),
        "artifactRefs": artifact_refs,
        "completionReceiptId": format!("optimizer_completion_{}", run.id)
    });
    if let Some(sealed) = sealed.as_ref() {
        if let Some(object) = result.as_object().cloned() {
            result = Value::Object(super::terminal::reconcile(object, sealed));
        }
    }
    Ok(result)
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
        out.push(serde_json::from_str(&row?).context("decode optimizer run")?);
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
    Ok(serde_json::from_str(&payload)?)
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

/// Validate a whole batch, then execute it atomically: insert the events,
/// advance the run and its cursor, refresh the cached projections, and — when
/// the batch ends the run — seal the terminal manifest. Every one of those is
/// part of the same transaction, so a projection that cannot be computed rolls
/// back the events that would have implied it rather than leaving a run whose
/// history and whose state slices describe different runs.
fn commit_validated_events(
    conn: &Connection,
    mut run: OptimizerRunRecord,
    events: Vec<OptimizerEventEnvelope>,
    contract: SequenceContract,
) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
    let durable = durable_event_ids(conn, &run.id, &events)?;
    let plan = plan_batch(&run.id, run.cursor_seq, &durable, &events, contract)
        .with_context(|| format!("validate optimizer event batch for {}", run.id))?;
    let mut appended = 0usize;
    for (event, verdict) in events.iter().zip(plan) {
        if verdict == EventVerdict::ConfirmedReplay {
            continue;
        }
        insert_event(conn, event)?;
        apply_event_to_run(&mut run, event);
        run.cursor_seq = event.sequence_number;
        upsert_cursor(conn, &run.id, run.cursor_seq, &event.occurred_at)?;
        appended += 1;
    }
    if appended == 0 {
        // Every event was a confirmed replay. Nothing durable changed, so the
        // bus stays quiet rather than waking every subscriber for no news.
        return Ok((run, None));
    }
    let history = load_events_upto(conn, &run.id, run.cursor_seq)?;
    if is_terminal_status(&run.status) {
        // Sealed at the cursor the terminal event advanced to, in the same
        // transaction that appended it. A later poll cannot replace it.
        let manifest = terminal::derive(&run, &history, &run.status, None);
        let sealed = terminal::seal(conn, &run.id, &manifest)?;
        // Carried on the run record too, so every surface that already reads a
        // run — the progress card, the MCP poll, restart recovery — reads the
        // frozen numbers without a second round trip, and cannot re-derive
        // different ones from a later cursor.
        if let Some(object) = run.summary.as_object_mut() {
            object.insert("terminalManifest".into(), sealed);
        }
    }
    upsert_run(conn, &run)?;
    let projected = project_from_events(&run, &history, None)
        .with_context(|| format!("project optimizer run {} at {}", run.id, run.cursor_seq))?;
    for slice in projected {
        cache_slice(conn, &slice)?;
    }
    let app_event = append_event(
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
    Ok((run, Some(app_event)))
}

/// Restore the fields the durable record is authoritative for onto a
/// caller-supplied snapshot.
///
/// `cursor_seq` never moves backwards, a terminal run never becomes live again,
/// timestamps are never unset, and published artifact references are never
/// dropped — a chat-owned visual that a stale writer forgets about is a visual
/// the user loses from Outputs.
fn preserve_durable_authority(conn: &Connection, run: &mut OptimizerRunRecord) -> Result<()> {
    let payload: Option<String> = conn
        .query_row(
            "SELECT payload_json FROM optimizer_runs WHERE id = ?1",
            params![run.id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(payload) = payload else {
        return Ok(());
    };
    let durable: OptimizerRunRecord = serde_json::from_str(&payload)?;
    if durable.cursor_seq > run.cursor_seq {
        run.cursor_seq = durable.cursor_seq;
        run.status = durable.status.clone();
    } else if is_terminal_status(&durable.status) && !is_terminal_status(&run.status) {
        run.status = durable.status.clone();
    }
    if run.started_at.is_none() {
        run.started_at = durable.started_at.clone();
    }
    if run.finished_at.is_none() {
        run.finished_at = durable.finished_at.clone();
    }
    merge_refs(&mut run.visual_refs, &durable.visual_refs);
    merge_refs(&mut run.output_refs, &durable.output_refs);
    Ok(())
}

/// Union by `(kind, id)`, keeping the caller's version of a reference it also
/// carries. Order is stable: the durable ones the caller dropped are appended.
fn merge_refs(into: &mut Vec<OptimizerResourceRef>, durable: &[OptimizerResourceRef]) {
    for reference in durable {
        let present = into
            .iter()
            .any(|existing| existing.kind == reference.kind && existing.id == reference.id);
        if !present {
            into.push(reference.clone());
        }
    }
}

pub(super) fn is_terminal_status(status: &str) -> bool {
    matches!(
        status,
        "completed" | "succeeded" | "failed" | "cancelled" | "degraded"
    )
}

/// The `sequence -> event_id` map for exactly the sequences a batch touches.
/// Scoped to the batch so validating one settlement does not read a campaign's
/// whole history.
fn durable_event_ids(
    conn: &Connection,
    run_id: &str,
    events: &[OptimizerEventEnvelope],
) -> Result<HashMap<u64, String>> {
    let mut out = HashMap::new();
    let mut stmt = conn.prepare(
        "SELECT sequence_number, event_id FROM optimizer_events
         WHERE optimizer_run_id = ?1 AND sequence_number = ?2",
    )?;
    for event in events {
        let row = stmt
            .query_row(params![run_id, event.sequence_number as i64], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })
            .optional()?;
        if let Some((sequence, event_id)) = row {
            out.insert(sequence as u64, event_id);
        }
    }
    Ok(out)
}

fn max_event_sequence(conn: &Connection, run_id: &str) -> Result<u64> {
    let value: i64 = conn.query_row(
        "SELECT COALESCE(MAX(sequence_number), 0) FROM optimizer_events WHERE optimizer_run_id = ?1",
        params![run_id],
        |row| row.get(0),
    )?;
    Ok(value.max(0) as u64)
}

fn event_id_exists(conn: &Connection, run_id: &str, event_id: &str) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM optimizer_events WHERE optimizer_run_id = ?1 AND event_id = ?2)",
        params![run_id, event_id],
        |row| row.get::<_, bool>(0),
    )
    .map_err(Into::into)
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

/// Insert one validated event.
///
/// Deliberately a plain `INSERT`: `plan_batch` has already decided this event is
/// new, so a conflict here means the plan was computed against a different
/// database state and the batch must fail rather than lose a row. `INSERT OR
/// IGNORE` is what let a colliding terminal event disappear while its run went
/// on to report success.
fn insert_event(conn: &Connection, event: &OptimizerEventEnvelope) -> Result<()> {
    let payload = serde_json::to_string(event)?;
    let event_id = event
        .event_id
        .clone()
        .unwrap_or_else(|| format!("{}:{}", event.optimizer_run_id, event.sequence_number));
    conn.execute(
        "INSERT INTO optimizer_events(
            event_id, optimizer_run_id, sequence_number, event_type,
            algorithm_id, occurred_at, payload_json
         ) VALUES (?1,?2,?3,?4,?5,?6,?7)",
        params![
            event_id,
            event.optimizer_run_id,
            event.sequence_number as i64,
            event.event_type,
            event.algorithm_id,
            event.occurred_at,
            payload
        ],
    )
    .with_context(|| {
        format!(
            "insert optimizer event {} ({}) at sequence {}",
            event_id, event.event_type, event.sequence_number
        )
    })?;
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
            | "sft.checkpoint_evaluation.allocated"
            | "training.evaluation.completed" => {
                checkpoint_evals.push(json!({
                    "sequence": event.sequence_number,
                    "delta": event.delta,
                    "snapshot": event.snapshot,
                    "item": event.item,
                    "role": event.delta.get("role").or_else(|| event.delta.get("phase")).cloned().unwrap_or(json!("selection")),
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

    /// Reopen a service over an existing instance directory: an application
    /// restart, as far as the durable record is concerned.
    pub(in crate::optimizers) async fn reopen(dir: &tempfile::TempDir) -> OptimizerService {
        let storage = Storage::open(dir.path().join("core")).unwrap();
        let journal = EventJournal::new(storage.database().clone());
        let content = ContentStore::new(storage.content_root());
        let visuals = VisualRegistry::new(storage.database().clone(), journal.clone(), content);
        let (events_tx, _) = tokio::sync::broadcast::channel(16);
        let manager = Arc::new(crate::optimizers::OptimizerManager::with_home(
            dir.path().join("optimizer-home"),
        ));
        OptimizerService::new_with_manager(
            storage.database().clone(),
            journal,
            visuals,
            events_tx,
            manager,
        )
    }

    async fn eval_run(svc: &OptimizerService, id: &str, session: &str) -> OptimizerRunRecord {
        let (run, _) = svc
            .create(OptimizerCreateRequest {
                algorithm_id: "eval".into(),
                algorithm_version: Some("1".into()),
                objective: Some("authority probe".into()),
                source: Some("local".into()),
                project_ref: None,
                session_ref: Some(session.into()),
                id: Some(id.into()),
                execution_bindings: None,
                input_refs: None,
                capabilities: Some(OptimizerCapabilities::for_algorithm("eval")),
                summary: Some(json!({ "recipeId": "eval.probe.v1" })),
                open_visual: Some(false),
                seed_fixture: None,
                cloud_config: None,
                local_path: None,
            })
            .await
            .unwrap();
        run
    }

    fn draft(event_type: &str) -> OptimizerEventDraft {
        OptimizerEventDraft::new(event_type, "eval").raw(json!({ "source": "test" }))
    }

    fn gepa_draft(event_type: &str, delta: Value) -> OptimizerEventDraft {
        OptimizerEventDraft::new(event_type, "gepa")
            .delta(delta.as_object().cloned().unwrap_or_default())
            .raw(json!({ "source": "test" }))
    }

    /// A GEPA search that spends its whole budget and keeps its seed is the
    /// common outcome, and `get_result` used to describe it exactly like a win:
    /// a `selectedCandidate` with a materialized prompt and `frontierMember:
    /// true`, no verdict, and nothing saying the winner was the incumbent.
    ///
    /// The result now leads with the verdict and keeps deployment separate from
    /// selection, so "the optimizer picked this" can never be read as "ship it".
    #[tokio::test]
    async fn a_gepa_run_that_kept_its_seed_never_reads_as_a_promotion() {
        let (svc, dir, _) = service().await;
        let run_dir = dir.path().join("gepa_seed_retained");
        std::fs::create_dir_all(&run_dir).unwrap();
        std::fs::write(
            run_dir.join("best_candidate.json"),
            json!({
                "candidate_id": "gepa_seed",
                "payload": { "stage2_system": "Classify the Banking77 intent." }
            })
            .to_string(),
        )
        .unwrap();
        let (run, _) = svc
            .create(OptimizerCreateRequest {
                algorithm_id: "gepa".into(),
                algorithm_version: Some("synth-optimizers-0.2.14".into()),
                objective: Some("Banking77 intent prompt".into()),
                source: Some("local".into()),
                project_ref: None,
                session_ref: Some("chat_gepa".into()),
                id: Some("banking77_gepa_seed_retained".into()),
                execution_bindings: None,
                input_refs: None,
                capabilities: Some(OptimizerCapabilities::for_algorithm("gepa")),
                summary: Some(json!({
                    "recipeId": "gepa.banking77.luna.v1",
                    "runDirectory": run_dir.display().to_string(),
                    "limits": { "proposalsPerGeneration": 10 },
                })),
                open_visual: Some(false),
                cloud_config: None,
                local_path: None,
                seed_fixture: None,
            })
            .await
            .unwrap();

        let mut events = vec![
            gepa_draft("optimizer.run.started", json!({})),
            gepa_draft(
                "candidate.registered",
                json!({ "candidate_id": "gepa_seed", "source": "seed" }),
            ),
        ];
        // The seed scores 1.0 on the minibatch rows and 0.5 on heldout; the one
        // proposal that was made scores 0.0 on the same minibatch and never
        // reaches heldout. That is the shape of every rejected proposal.
        for (candidate, stage, rewards) in [
            ("gepa_seed", "seed_full_train", vec![1.0, 0.0]),
            ("gepa_seed", "parent_minibatch_reference", vec![1.0, 1.0]),
            ("gepa_child", "candidate_minibatch", vec![0.0, 0.0]),
            ("gepa_seed", "heldout", vec![1.0, 0.0]),
        ] {
            for (index, reward) in rewards.into_iter().enumerate() {
                events.push(gepa_draft(
                    "optimizer.candidate_evaluation.allocated",
                    json!({ "candidate_id": candidate, "stage": stage }),
                ));
                events.push(gepa_draft(
                    "optimizer.evaluation_result.received",
                    json!({
                        "candidate_id": candidate,
                        "stage": stage,
                        "evaluation_id": format!("{candidate}:{stage}:{index}"),
                        "reward": reward,
                        "active_workers": 8
                    }),
                ));
            }
        }
        events.push(gepa_draft(
            "candidate.registered",
            json!({
                "candidate_id": "gepa_child",
                "parent_id": "gepa_seed",
                "generation": 0,
                "proposal_index": 0,
                "source": "reflector:parent_variation"
            }),
        ));
        events.push(gepa_draft(
            "proposer.completed",
            json!({ "proposal_count": 1 }),
        ));
        events.push(gepa_draft(
            "heldout.completed",
            json!({ "candidate_id": "gepa_seed", "heldout_reward": 0.5 }),
        ));
        events.push(gepa_draft(
            "frontier.snapshot",
            json!({ "best_candidate_id": "gepa_seed" }),
        ));
        events.push(gepa_draft(
            "gepa.run.finished",
            json!({
                "state": "completed",
                "runtime_summary": {
                    "policy": { "model": "gpt-4.1-nano", "calls": 8, "cost_usd": 0.01 },
                    "proposer": { "model": "gpt-5.6-luna", "calls": 1, "cost_usd": 0.0 }
                }
            }),
        ));
        events.push(gepa_draft("optimizer.run.completed", json!({})));
        svc.append_event_payloads(run.id.clone(), events)
            .await
            .unwrap();

        let sealed = svc
            .terminal_manifest(run.id.clone())
            .await
            .unwrap()
            .expect("a terminal GEPA event seals a manifest");
        // Before this lane the manifest sealed with every one of these null.
        assert_eq!(sealed["work"]["succeeded"], json!(8), "8 scored rollouts");
        assert_eq!(sealed["work"]["unit"], json!("rollouts"));
        assert_eq!(
            sealed["selection"]["verdict"],
            json!("no_measured_improvement")
        );
        assert_eq!(sealed["selection"]["accepted"], json!(false));
        assert_eq!(
            sealed["usage"]["lanes"]["proposer"]["model"],
            json!("gpt-5.6-luna")
        );
        assert_eq!(
            sealed["usage"]["lanes"]["policy"]["model"],
            json!("gpt-4.1-nano")
        );
        assert_eq!(sealed["gepaEvidence"]["proposals"]["requested"], json!(10));
        assert_eq!(sealed["gepaEvidence"]["proposals"]["registered"], json!(1));
        assert_eq!(sealed["gepaEvidence"]["proposals"]["shortfall"], json!(9));
        assert_eq!(
            sealed["gepaEvidence"]["rollouts"]["maxActiveWorkers"],
            json!(8)
        );

        let result = svc.get_result(run.id.clone()).await.unwrap();
        assert_eq!(result["verdict"], json!("no_measured_improvement"));
        assert_eq!(result["deployment"]["recommended"], json!(false));
        assert_eq!(result["deployment"]["candidateId"], Value::Null);
        // The prompt is still materialized — callers need to read what ran — but
        // it is no longer the whole answer.
        assert_eq!(
            result["selectedCandidate"]["materializedValues"]["prompt"],
            json!("Classify the Banking77 intent.")
        );
        assert_eq!(
            result["evidence"]["candidates"].as_array().map(Vec::len),
            Some(2),
            "both the seed and the rejected proposal stay on the record"
        );

        // A late reconcile arrives carrying a frontier snapshot naming a
        // different winner. The run is already sealed: the manifest is
        // write-once, so the settled verdict, counts, and lineage stand and the
        // late event only extends the log.
        svc.append_event_payloads(
            run.id.clone(),
            vec![gepa_draft(
                "frontier.snapshot",
                json!({ "best_candidate_id": "gepa_child" }),
            )],
        )
        .await
        .unwrap();
        let after = svc
            .terminal_manifest(run.id.clone())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            after["selection"]["verdict"],
            json!("no_measured_improvement")
        );
        assert_eq!(
            after["selection"]["selectedCandidateId"],
            json!("gepa_seed")
        );
        assert_eq!(after["work"]["succeeded"], json!(8));
        assert_eq!(after["terminalCursor"], sealed["terminalCursor"]);
        let reread = svc.get_result(run.id.clone()).await.unwrap();
        assert_eq!(
            reread["verdict"],
            json!("no_measured_improvement"),
            "get_result reconciles against the sealed manifest, not the live tail"
        );
    }

    /// The Banking77 loss, end to end at the service boundary.
    ///
    /// A caller holds the record returned before the worker started, the worker
    /// runs to terminal, and then the caller writes its snapshot back. The
    /// snapshot must not un-finish the run, rewind its cursor, or forget the
    /// visual the run published in the meantime — all three of which are what
    /// made the next event collide with sequence 1 and vanish.
    #[tokio::test]
    async fn a_stale_snapshot_cannot_rewind_a_run_that_moved_on() {
        let (svc, _dir, _) = service().await;
        let stale = eval_run(&svc, "opt_eval_stale", "chat_stale").await;
        assert_eq!(stale.cursor_seq, 0);

        svc.append_event_payloads(
            stale.id.clone(),
            vec![
                draft("optimizer.run.started")
                    .delta(Map::from_iter([("status".into(), json!("running"))])),
                draft("eval.run.planned")
                    .snapshot(Map::from_iter([("planned_trials".into(), json!(2))])),
                draft("optimizer.run.completed")
                    .delta(Map::from_iter([("status".into(), json!("completed"))])),
            ],
        )
        .await
        .unwrap();

        let mut writeback = stale.clone();
        writeback.summary = json!({ "recipeId": "eval.probe.v1", "policyPin": "pinned" });
        let persisted = svc.persist_run(writeback).await.unwrap();
        assert_eq!(persisted.cursor_seq, 3, "cursor must not rewind");
        assert_eq!(persisted.status, "completed", "a settled run stays settled");
        assert_eq!(persisted.summary["policyPin"], json!("pinned"));

        // And the next event still lands above the durable cursor.
        let (after, _) = svc
            .append_event_payloads(stale.id.clone(), vec![draft("optimizer.usage")])
            .await
            .unwrap();
        assert_eq!(after.cursor_seq, 4);
        let events = svc.events_after(stale.id, 0, None).await.unwrap();
        assert_eq!(events.len(), 4, "no event was dropped");
    }

    /// Sequence allocation is inside the transaction, so racing appends
    /// interleave without holes and without collisions.
    #[tokio::test]
    async fn concurrent_appends_allocate_contiguous_sequences() {
        let (svc, _dir, _) = service().await;
        let run = eval_run(&svc, "opt_eval_race", "chat_race").await;
        let mut handles = Vec::new();
        for index in 0..12 {
            let svc = svc.clone();
            let run_id = run.id.clone();
            handles.push(tokio::spawn(async move {
                svc.append_event_payloads(
                    run_id,
                    vec![draft("eval.trial.terminal").item(json!({
                        "kind": "trial",
                        "id": format!("trial:{index}"),
                        "valid": true,
                    }))],
                )
                .await
            }));
        }
        for handle in handles {
            handle.await.unwrap().unwrap();
        }
        let events = svc
            .events_after(run.id.clone(), 0, Some(200))
            .await
            .unwrap();
        assert_eq!(events.len(), 12);
        let sequences: Vec<u64> = events.iter().map(|event| event.sequence_number).collect();
        assert_eq!(sequences, (1..=12).collect::<Vec<_>>());
        assert_eq!(svc.get(run.id).await.unwrap().cursor_seq, 12);
    }

    /// A producer writing from a stale cursor is told, not ignored. The old
    /// path skipped the event and advanced anyway.
    #[tokio::test]
    async fn a_colliding_event_is_refused_rather_than_dropped() {
        let (svc, _dir, _) = service().await;
        let run = eval_run(&svc, "opt_eval_collide", "chat_collide").await;
        svc.append_event_payloads(run.id.clone(), vec![draft("optimizer.run.started")])
            .await
            .unwrap();
        let colliding = OptimizerEventEnvelope {
            schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
            event_id: Some("opt_eval_collide:stale-terminal".into()),
            event_type: "optimizer.run.completed".into(),
            sequence_number: 1,
            occurred_at: Utc::now().to_rfc3339(),
            optimizer_run_id: run.id.clone(),
            algorithm_id: "eval".into(),
            level: None,
            item: None,
            delta: Map::from_iter([("status".into(), json!("completed"))]),
            snapshot: None,
            usage_delta: None,
            artifact_refs: vec![],
            error: None,
            raw: json!({}),
        };
        let error = format!(
            "{:#}",
            svc.append_events(run.id.clone(), vec![colliding])
                .await
                .unwrap_err()
        );
        assert!(error.contains("already holds event"), "{error}");
        let settled = svc.get(run.id).await.unwrap();
        assert_ne!(
            settled.status, "completed",
            "a refused terminal event must not settle the run"
        );
    }

    /// Evidence that never persisted is not a success. The compute records
    /// survive, the run is terminal and named, and the failure is retryable.
    #[tokio::test]
    async fn an_evidence_failure_settles_degraded_and_keeps_the_compute() {
        let (svc, _dir, _) = service().await;
        let run = eval_run(&svc, "opt_eval_degraded", "chat_degraded").await;
        svc.append_event_payloads(run.id.clone(), vec![draft("optimizer.run.started")])
            .await
            .unwrap();
        svc.patch_run(run.id.clone(), |run| {
            let mut summary = run.summary.as_object().cloned().unwrap_or_default();
            summary.insert("records".into(), json!([{ "reward": 1.0 }]));
            run.summary = Value::Object(summary);
            Ok(())
        })
        .await
        .unwrap();

        let settled = svc
            .settle_evidence_degraded(
                run.id.clone(),
                "progress_projection",
                "visual registry refused the update".into(),
            )
            .await
            .unwrap();
        assert_eq!(settled.status, "degraded");
        assert_ne!(settled.status, "completed");
        assert_eq!(
            settled.summary["records"].as_array().map(Vec::len),
            Some(1),
            "paid compute records must survive an evidence failure"
        );
        let manifest = svc
            .terminal_manifest(run.id.clone())
            .await
            .unwrap()
            .expect("a degraded run still seals a manifest");
        assert_eq!(manifest["terminalStatus"], json!("failed_evidence"));
        assert_eq!(manifest["degradation"]["retryable"], json!(true));
        assert_eq!(
            manifest["degradation"]["stage"],
            json!("progress_projection")
        );
    }

    /// The manifest is written once. A later poll — with an older cursor, or a
    /// different opinion — reads the sealed record rather than replacing it.
    #[tokio::test]
    async fn a_sealed_terminal_manifest_is_never_replaced() {
        let (svc, _dir, _) = service().await;
        let run = eval_run(&svc, "opt_eval_sealed", "chat_sealed").await;
        svc.append_event_payloads(
            run.id.clone(),
            vec![
                draft("optimizer.run.started"),
                draft("eval.run.planned")
                    .snapshot(Map::from_iter([("planned_trials".into(), json!(3))])),
                draft("eval.trial.terminal").item(json!({ "id": "t1", "valid": true })),
                draft("optimizer.run.completed"),
            ],
        )
        .await
        .unwrap();
        let sealed = svc
            .terminal_manifest(run.id.clone())
            .await
            .unwrap()
            .expect("terminal event seals a manifest");
        assert_eq!(sealed["terminalCursor"], json!(4));
        assert_eq!(sealed["work"]["planned"], json!(3));
        assert_eq!(sealed["work"]["succeeded"], json!(1));
        assert_eq!(sealed["work"]["skipped"], json!(2));

        // A late degradation amends the lane; it does not re-end the run.
        svc.settle_evidence_degraded(run.id.clone(), "late_probe", "arrived after sealing".into())
            .await
            .unwrap();
        let again = svc
            .terminal_manifest(run.id.clone())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(again["terminalStatus"], json!("completed"));
        assert_eq!(again["terminalCursor"], json!(4));
        assert_eq!(again["degradation"][0]["stage"], json!("late_probe"));
    }

    /// `get_result` on an eval answers with eval facts and never reaches for a
    /// candidate. This is the reported failure, inverted.
    #[tokio::test]
    async fn get_result_answers_an_eval_without_gepa_materialization() {
        let (svc, _dir, _) = service().await;
        let run = eval_run(&svc, "opt_eval_result", "chat_result").await;
        svc.append_event_payloads(
            run.id.clone(),
            vec![
                draft("optimizer.run.started"),
                draft("eval.run.planned")
                    .snapshot(Map::from_iter([("planned_trials".into(), json!(1))])),
                draft("eval.trial.terminal").item(json!({ "id": "t1", "valid": true })),
                draft("optimizer.run.completed"),
            ],
        )
        .await
        .unwrap();
        let result = svc.get_result(run.id.clone()).await.unwrap();
        assert_eq!(result["resultKind"], json!("eval_run_result.v1"));
        assert_eq!(result["trials"]["succeeded"], json!(1));
        assert_eq!(result["finalCursor"], json!(4));
        assert!(result.get("selectedCandidate").is_none());
    }

    /// GEPA, eval, and SFT settle into their own typed results. A shared
    /// GEPA-shaped reader is what made a baseline eval demand a prompt it was
    /// never designed to have.
    #[tokio::test]
    async fn terminal_results_stay_algorithm_specific() {
        let (svc, _dir, _) = service().await;
        let (gepa, _) = svc
            .seed_fixture("gepa", Some("chat_kinds".into()))
            .await
            .unwrap();
        let (sft, _) = svc
            .seed_fixture("sft", Some("chat_kinds".into()))
            .await
            .unwrap();
        let eval = eval_run(&svc, "opt_eval_kinds", "chat_kinds").await;
        svc.append_event_payloads(
            eval.id.clone(),
            vec![
                draft("optimizer.run.started"),
                draft("eval.run.planned")
                    .snapshot(Map::from_iter([("planned_trials".into(), json!(2))])),
                draft("optimizer.run.completed"),
            ],
        )
        .await
        .unwrap();

        assert_eq!(
            svc.get_result(eval.id).await.unwrap()["resultKind"],
            json!("eval_run_result.v1")
        );
        assert_eq!(
            svc.get_result(sft.id).await.unwrap()["resultKind"],
            json!("sft_run_result.v1")
        );
        // GEPA still materializes a candidate, and still fails closed when a
        // completed optimization has none — that safeguard was correct for GEPA
        // and only wrong when applied to everything else.
        let gepa_result = svc.get_result(gepa.id).await;
        match gepa_result {
            Ok(value) => assert_eq!(value["resultKind"], json!("gepa_run_result.v1")),
            Err(error) => assert!(
                format!("{error:#}").contains("materialized prompt"),
                "{error:#}"
            ),
        }
    }

    #[test]
    fn recipe_readiness_names_missing_contract_and_owner() {
        let projected = project_recipe_readiness(json!({
            "id": "eval.fixture.policy-smoke.v1",
            "algorithmId": "eval",
            "availability": "available",
            "limits": {},
        }));
        assert_eq!(projected["availability"], json!("unavailable"));
        assert_eq!(projected["readiness"]["ready"], json!(false));
        assert_eq!(
            projected["readiness"]["blockers"][0]["contract"],
            json!("limits.trials")
        );
        assert_eq!(
            projected["readiness"]["blockers"][0]["owner"],
            json!("Optimizers")
        );
    }

    #[test]
    fn recipe_readiness_preserves_a_structured_cookbook_blocker() {
        let projected = project_recipe_readiness(json!({
            "id": "gepa.craftax.smoke.v1",
            "algorithmId": "gepa",
            "availability": "unavailable",
            "availabilityReason": "craftax cookbook is unavailable",
            "limits": {"maxTotalRollouts": 6},
        }));
        assert_eq!(
            projected["readiness"]["blockers"][0]["code"],
            json!("cookbook_unavailable")
        );
        assert_eq!(
            projected["readiness"]["blockers"][0]["contract"],
            json!("assets.cookbook")
        );
        assert_eq!(
            projected["readiness"]["blockers"][0]["retryable"],
            json!(true)
        );
    }

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
        // Visual selection never consults the plugin. Execution still checks
        // the handshake; the sidecar now advertises sft/cispo placements from
        // the training routes it serves.
        assert_eq!(negotiate_visual_template("gepa"), "optimizer.gepa.live.v1");
        assert_eq!(negotiate_visual_template("sft"), "optimizer.sft.live.v1");
        assert_eq!(negotiate_visual_template("cispo"), "optimizer.sft.live.v1");
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
        let started = OptimizerEventEnvelope {
            schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
            event_id: Some("gepa_cap_probe:started".into()),
            event_type: "optimizer.run.started".into(),
            sequence_number: 1,
            occurred_at: chrono::Utc::now().to_rfc3339(),
            optimizer_run_id: run.id.clone(),
            algorithm_id: "gepa".into(),
            level: Some("info".into()),
            item: None,
            delta: Map::from_iter([("status".into(), json!("running"))]),
            snapshot: None,
            usage_delta: None,
            artifact_refs: vec![],
            error: None,
            raw: json!({}),
        };
        svc.append_events(run.id.clone(), vec![started])
            .await
            .unwrap();
        let run = svc
            .attach_paid_compute_approval(run.id, "approval-paid", Some(500_000), Some(4))
            .await
            .unwrap();
        assert_eq!(run.cursor_seq, 1, "approval patch must not rewind progress");
        let event = OptimizerEventEnvelope {
            schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
            event_id: Some("gepa_cap_probe:1".into()),
            event_type: "optimizer.usage".into(),
            sequence_number: 2,
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
    async fn reports_cispo_as_available_algorithm_and_lists_bounded_recipes() {
        let (svc, _dir, _) = service().await;
        let cispo = svc
            .list_algorithms()
            .into_iter()
            .find(|a| a.get("id") == Some(&json!("cispo")))
            .unwrap();
        assert_eq!(cispo.get("availability"), Some(&json!("available")));
        let recipes = svc.list_recipes();
        assert!(recipes
            .iter()
            .any(|item| item.get("id") == Some(&json!("cispo.banking77.mlx.v1"))));
        assert!(recipes
            .iter()
            .any(|item| item.get("id") == Some(&json!("cispo.slime.hosted.v1"))));
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
}
