use super::events::{plan_batch, EventVerdict, OptimizerEventDraft, SequenceContract};
use super::models::{
    EffectiveContract, OptimizerArtifactPage, OptimizerArtifactRange, OptimizerCapabilities,
    OptimizerCreateRequest, OptimizerEventEnvelope, OptimizerQuery, OptimizerRelationship,
    OptimizerResourceRef, OptimizerRunRecord, OptimizerRunStatus, OptimizerStateSlice,
    OptimizerUsageSummary, EFFECTIVE_CONTRACT_SCHEMA_VERSION, OPTIMIZER_EVENT_SCHEMA_VERSION,
    OPTIMIZER_RUN_SCHEMA_VERSION, OPTIMIZER_STATE_SLICE_SCHEMA_VERSION,
};
use super::results;
use super::terminal;
use super::training_adapter::is_step_metrics_event;
use crate::storage::{
    append_event, AppEvent, ContentStore, Database, EventAppend, EventJournal, EventSource,
};
use crate::visuals::{VisualCreateRequest, VisualRegistry, VISUAL_BINDINGS_SCHEMA_VERSION};
use anyhow::{anyhow, bail, Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, Mutex};
use uuid::Uuid;


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
        let (code, contract, owner) = if detail.contains("workspace recipe") {
            (
                "workspace_recipe_unavailable",
                "assets.workspace_recipe",
                "Optimizers",
            )
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

/// Admission for a control command: the capability must exist, the run must
/// not already be settled, and `current -> next` must be a transition
/// [`OptimizerRunStatus`] allows. `next` is passed in rather than derived so
/// this and [`OptimizerService::command`], which performs the write, cannot
/// disagree about what the command means.
/// The terminal kind a sealed manifest records, across both manifest schemas.
/// Legacy v1 statuses that widened `failed` (interrupted, infrastructure_lost,
/// failed_evidence) read as failed; cap_reached reads as degraded.
fn manifest_terminal_kind(manifest: &Value) -> super::kernel::TerminalKind {
    use super::kernel::TerminalKind;
    let status = manifest
        .pointer("/terminal/kind")
        .and_then(Value::as_str)
        .or_else(|| manifest.get("terminalStatus").and_then(Value::as_str))
        .unwrap_or("failed");
    match status {
        "completed" => TerminalKind::Completed,
        "cancelled" => TerminalKind::Cancelled,
        "degraded" | "cap_reached" => TerminalKind::Degraded,
        _ => TerminalKind::Failed,
    }
}

fn credential_revocation_amendment(
    run: &OptimizerRunRecord,
    terminal_sequence: u64,
    capability_ids: Vec<String>,
    cancellation: Option<&std::sync::Arc<super::kernel::CancellationRequest>>,
) -> OptimizerEventDraft {
    OptimizerEventDraft::new("optimizer.evidence.amended", &run.algorithm_id)
        .idempotency_key(format!("credential-revoked:{terminal_sequence}"))
        .level("info")
        .delta(Map::from_iter([
            ("terminalSequence".into(), json!(terminal_sequence)),
            (
                "credentialRevocation".into(),
                json!({
                    "kind": "credential.capability.revoked",
                    "capabilityIds": capability_ids,
                    "cause": "run_terminal",
                    "cancellationRequestId": cancellation
                        .map(|request| request.request_id.clone()),
                }),
            ),
        ]))
        .raw(json!({ "source": "settle_run" }))
}

fn validate_control(
    run: &OptimizerRunRecord,
    command: &str,
    next: OptimizerRunStatus,
) -> Result<()> {
    match command {
        "cancel" if !run.capabilities.cancel => bail!("cancel is not available for this run"),
        "pause" if !run.capabilities.pause => bail!("pause is not available for this run"),
        "resume" if !run.capabilities.resume => bail!("resume is not available for this run"),
        _ => {}
    }
    let Some(status) = OptimizerRunStatus::parse(&run.status) else {
        bail!(
            "{command} is not available for a run in unrecognised status {}",
            run.status
        );
    };
    if status.is_terminal() {
        bail!("{command} is not available for a {} run", run.status);
    }
    // Source-state rules the transition table cannot express: `queued -> running`
    // is a legal transition but it is a *start*, not a resume, and pausing an
    // already-paused run is a no-op the caller should not be told succeeded.
    match command {
        "pause" if status != OptimizerRunStatus::Running => bail!(
            "pause requires a running optimizer; current status is {}",
            run.status
        ),
        "resume" if status != OptimizerRunStatus::Paused => bail!(
            "resume requires a paused optimizer; current status is {}",
            run.status
        ),
        _ => {}
    }
    if !status.can_transition_to(next) {
        bail!(
            "{command} cannot move a {} run to {}",
            run.status,
            next.as_str()
        );
    }
    Ok(())
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

/// One retained media object, as the run's media index records it.
///
/// Written by the relay at the moment the bytes land in the content store, and
/// read by the media bridge to decide whether a visual bound to this run is
/// allowed to see them.
#[derive(Clone, Debug)]
pub(super) struct RunMediaRow {
    pub cas_digest: String,
    pub kind: &'static str,
    pub media_type: &'static str,
    pub byte_size: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub rollout_id: Option<String>,
    pub trial_id: Option<String>,
    pub step: Option<i64>,
    pub producer_digest: Option<String>,
}

/// A media object the host has agreed to serve, with the identity that made it
/// serveable. Never carries bytes: the caller reads those from the store.
#[derive(Clone, Debug)]
pub struct GrantedRunMedia {
    pub optimizer_run_id: String,
    pub cas_digest: String,
    pub kind: String,
    pub media_type: String,
    pub byte_size: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub rollout_id: Option<String>,
    pub step: Option<i64>,
}

#[derive(Clone)]
pub struct OptimizerService {
    db: Arc<Database>,
    frame_store: ContentStore,
    #[allow(dead_code)]
    journal: EventJournal,
    visuals: VisualRegistry,
    local_recipes: Arc<Mutex<HashMap<String, super::CancelSignal>>>,
    events_tx: broadcast::Sender<AppEvent>,
    manager: Arc<super::OptimizerManager>,
    /// Attached once by the composition root. Optimizer lifecycle failures are
    /// already recorded as bounded run evidence; this lets the same failure
    /// also be correlated with the container, stream, and visual around it —
    /// without inventing a second source of truth for the run itself.
    diagnostics: Arc<std::sync::OnceLock<Arc<crate::diagnostics::DiagnosticsService>>>,
}

/// Live claim for one optimizer worker. Heartbeats every
/// [`crate::recovery::HEARTBEAT_INTERVAL`] and releases the row on drop.
pub(super) struct OptimizerRunOwnershipGuard {
    db: Arc<Database>,
    run_id: String,
    heartbeat: Option<tokio::task::JoinHandle<()>>,
}

impl OptimizerRunOwnershipGuard {
    fn arm(db: Arc<Database>, run_id: String) -> Result<Self> {
        let instance_id = crate::instance::boot_epoch().to_string();
        let pid = std::process::id();
        let identity = super::manager::process_start_identity(pid);
        let claimed_run = run_id.clone();
        let claimed_instance = instance_id.clone();
        let claimed_identity = identity.clone();
        db.with_conn(move |conn| {
            crate::recovery::ownership::claim_optimizer_run(
                conn,
                &claimed_run,
                &claimed_instance,
                &claimed_instance,
                Some(pid),
                claimed_identity.as_deref(),
                Utc::now(),
            )
        })?;
        let db_hb = db.clone();
        let run_hb = run_id.clone();
        let instance_hb = instance_id;
        let heartbeat = tokio::spawn(async move {
            let period = crate::recovery::HEARTBEAT_INTERVAL
                .to_std()
                .unwrap_or(Duration::from_secs(5));
            let mut ticks = tokio::time::interval(period);
            ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            ticks.tick().await;
            loop {
                ticks.tick().await;
                let db = db_hb.clone();
                let run_id = run_hb.clone();
                let instance_id = instance_hb.clone();
                let _ = db.with_conn(move |conn| {
                    crate::recovery::ownership::heartbeat_optimizer_run(
                        conn,
                        &run_id,
                        &instance_id,
                        Utc::now(),
                    )
                });
            }
        });
        Ok(Self {
            db,
            run_id,
            heartbeat: Some(heartbeat),
        })
    }
}

impl Drop for OptimizerRunOwnershipGuard {
    fn drop(&mut self) {
        if let Some(task) = self.heartbeat.take() {
            task.abort();
        }
        let run_id = self.run_id.clone();
        let _ = self.db.with_conn(move |conn| {
            crate::recovery::ownership::release_optimizer_run(conn, &run_id)
        });
    }
}

impl OptimizerService {
    /// Deliver the newest durable projection wake-up for each selected run.
    /// Missing renderer subscribers become retryable outbox state rather than
    /// rolling back the already-committed projection.
    async fn sweep_projection_outbox(
        &self,
        only_run_id: Option<String>,
        event_hint: Option<&AppEvent>,
    ) -> Result<usize> {
        let filter = only_run_id.clone();
        let db = self.db.clone();
        let pending = db
            .run(move |conn| super::kernel::outbox::pending_latest(conn, filter.as_deref()))
            .await?;
        let mut latest_by_run = BTreeMap::<String, u64>::new();
        for row in pending {
            latest_by_run
                .entry(row.run_id)
                .and_modify(|revision| *revision = (*revision).max(row.projection_revision))
                .or_insert(row.projection_revision);
        }
        let mut delivered = 0usize;
        for (run_id, revision) in latest_by_run {
            let event = event_hint
                .filter(|event| event.payload["optimizerRunId"].as_str() == Some(run_id.as_str()))
                .cloned()
                .unwrap_or_else(|| AppEvent {
                    schema_version: crate::storage::APP_EVENT_SCHEMA_VERSION.into(),
                    sequence: 0,
                    event_id: format!("projection_outbox_{}", Uuid::new_v4().simple()),
                    session_id: None,
                    session_sequence: None,
                    run_id: None,
                    source: EventSource::System,
                    kind: "optimizer.run.updated".into(),
                    payload: json!({
                        "optimizerRunId": run_id,
                        "projectionRevision": revision,
                        "delivery": "outbox_retry",
                    }),
                    remote_sequence: None,
                    command_id: None,
                    created_at: Utc::now().to_rfc3339(),
                });
            let db = self.db.clone();
            let changed = match self.events_tx.send(event) {
                Ok(_) => {
                    let marked_run = run_id.clone();
                    db.run(move |conn| {
                        super::kernel::outbox::mark_delivered(conn, &marked_run, revision)
                    })
                    .await?
                }
                Err(error) => {
                    let marked_run = run_id.clone();
                    let message = error.to_string();
                    db.run(move |conn| {
                        super::kernel::outbox::mark_failed(conn, &marked_run, revision, &message)
                    })
                    .await?;
                    0
                }
            };
            delivered += changed;
        }
        Ok(delivered)
    }

    pub(super) async fn record_visual_projection_delivery_failure(
        &self,
        run_id: &str,
        error: &anyhow::Error,
    ) -> Result<()> {
        let run_id = run_id.to_string();
        let message = format!("{error:#}");
        let db = self.db.clone();
        db.run(move |conn| {
            super::kernel::outbox::mark_visual_failed(conn, &run_id, &message)?;
            Ok(())
        })
        .await
    }

    /// Publish a durable visual event produced by an internal optimizer worker.
    ///
    /// MCP-driven visual updates return their event to the caller, which then
    /// reaches the renderer through the normal request lane. Local recipe
    /// workers have no caller to do that forwarding, so they must place the
    /// already-durable event on the shared bus themselves.
    pub(super) fn publish_visual_event(&self, value: Value) -> Result<()> {
        let event: AppEvent = serde_json::from_value(value)
            .context("optimizer visual update returned an invalid app event")?;
        // No receiver is a normal offline state: the durable projection outbox
        // remains pending for the next sweep. If a receiver is present, a send
        // failure is no longer allowed to disappear into a best-effort `let _`.
        if self.events_tx.receiver_count() > 0 {
            self.events_tx
                .send(event)
                .map_err(|error| anyhow!("publish optimizer visual event: {error}"))?;
        }
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
        let frame_store = ContentStore::new(
            db.path()
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("store"),
        );
        Self {
            db,
            frame_store,
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

    pub(super) fn hold_run_ownership(&self, run_id: &str) -> Result<OptimizerRunOwnershipGuard> {
        OptimizerRunOwnershipGuard::arm(self.db.clone(), run_id.to_string())
    }

    /// The content store behind the visual registry.
    ///
    /// One store, not a second one: a relayed frame and a rendered chart must
    /// be addressable by the same digest from the same place, or the media
    /// bridge would have to know which of two roots a digest came from.
    pub(super) fn content(&self) -> &crate::storage::ContentStore {
        self.visuals.content()
    }

    pub(super) fn visuals(&self) -> &VisualRegistry {
        &self.visuals
    }

    /// Index one retained media object against the run that produced it.
    ///
    /// Idempotent by digest: the same frame relayed twice — a retried page, a
    /// resumed worker — is one row, and two steps that rendered identical
    /// pixels share it, which is exactly the physical deduplication the content
    /// store already performs.
    pub(super) async fn record_run_media(&self, run_id: &str, row: &RunMediaRow) -> Result<()> {
        const MAX_BUSY_ATTEMPTS: usize = 3;
        for attempt in 1..=MAX_BUSY_ATTEMPTS {
            let run_id = run_id.to_string();
            let row = row.clone();
            let result = self
                .db
                .clone()
                .run_transaction(move |conn| {
                    conn.execute(
                        "INSERT INTO optimizer_run_media(
                        optimizer_run_id, cas_digest, kind, media_type, byte_size,
                        width, height, rollout_id, trial_id, step, producer_digest, created_at)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,datetime('now'))
                     ON CONFLICT(optimizer_run_id, cas_digest) DO NOTHING",
                        params![
                            run_id,
                            row.cas_digest,
                            row.kind,
                            row.media_type,
                            row.byte_size as i64,
                            row.width.map(i64::from),
                            row.height.map(i64::from),
                            row.rollout_id,
                            row.trial_id,
                            row.step,
                            row.producer_digest,
                        ],
                    )?;
                    Ok(())
                })
                .await;
            match result {
                Ok(()) => return Ok(()),
                Err(error)
                    if attempt < MAX_BUSY_ATTEMPTS
                        && error.to_string().contains("database is locked") =>
                {
                    // Five live Craftax relays can finish frames at the same
                    // instant. SQLite's busy timeout normally serializes them,
                    // but a long event projection transaction can outlive that
                    // timeout. A frame is durable in CAS already, so retrying
                    // this idempotent index insert is safer than drawing a
                    // permanent hole in the live trace.
                    tokio::time::sleep(std::time::Duration::from_millis(25 * attempt as u64)).await;
                }
                Err(error) => return Err(error),
            }
        }
        unreachable!("bounded media-index retry loop always returns")
    }

    /// Durable media counts for one trial. Receipts use this index, never the
    /// relay task's in-memory counters, so restart and replay report the same
    /// retained objects and bytes.
    pub(super) async fn run_media_totals(
        &self,
        run_id: &str,
        trial_id: &str,
    ) -> Result<(u64, u64)> {
        let run_id = run_id.to_string();
        let trial_id = trial_id.to_string();
        self.db
            .clone()
            .run(move |conn| {
                conn.query_row(
                    "SELECT COUNT(*), COALESCE(SUM(m.byte_size), 0)
                     FROM optimizer_run_media m
                     WHERE m.optimizer_run_id=?1 AND m.media_type='image/png'
                       AND EXISTS (
                         SELECT 1 FROM optimizer_events e
                         WHERE e.optimizer_run_id=m.optimizer_run_id
                           AND json_extract(e.payload_json,'$.delta.trial_id')=?2
                           AND json_extract(
                             e.payload_json,
                             '$.delta.container_event.payload.media.casDigest'
                           )=m.cas_digest
                       )",
                    params![run_id, trial_id],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?.max(0) as u64,
                            row.get::<_, i64>(1)?.max(0) as u64,
                        ))
                    },
                )
                .map_err(Into::into)
            })
            .await
    }

    /// Decide whether `cas_digest` is media this run actually produced.
    ///
    /// The whole authorization for the media bridge lives here, and it is a
    /// lookup rather than a scan on purpose: a gate that has to re-derive its
    /// answer from event payloads stops matching the moment the payload shape
    /// moves, and stops matching *open*.
    pub async fn granted_run_media(
        &self,
        run_id: &str,
        cas_digest: &str,
    ) -> Result<Option<GrantedRunMedia>> {
        let run_id = run_id.to_string();
        let digest = cas_digest.to_ascii_lowercase();
        if digest.len() != 64 || !digest.chars().all(|value| value.is_ascii_hexdigit()) {
            bail!("media digest must be a 64-character SHA-256");
        }
        let owned_run = run_id.clone();
        self.db
            .clone()
            .run(move |conn| {
                conn.query_row(
                    "SELECT kind, media_type, byte_size, width, height, rollout_id, step
                     FROM optimizer_run_media
                     WHERE optimizer_run_id=?1 AND cas_digest=?2",
                    params![owned_run, digest],
                    |row| {
                        Ok(GrantedRunMedia {
                            optimizer_run_id: run_id.clone(),
                            cas_digest: digest.clone(),
                            kind: row.get(0)?,
                            media_type: row.get(1)?,
                            byte_size: row.get::<_, i64>(2)?.max(0) as u64,
                            width: row.get::<_, Option<i64>>(3)?.map(|value| value as u32),
                            height: row.get::<_, Option<i64>>(4)?.map(|value| value as u32),
                            rollout_id: row.get(5)?,
                            step: row.get(6)?,
                        })
                    },
                )
                .optional()
                .map_err(Into::into)
            })
            .await
    }

    /// Read granted media bytes out of the content store.
    pub fn read_media_bytes(&self, granted: &GrantedRunMedia) -> Result<Vec<u8>> {
        self.visuals
            .content()
            .get_bytes(&granted.kind, &granted.cas_digest)
    }

    /// Import a rollout's sealed Trace V5 bundle by container identity.
    ///
    /// The eval worker calls this as each rollout finishes, so replay survives
    /// the container being stopped and Workshop being restarted. The machinery
    /// already existed in the visuals IPC lane; nothing on the eval path ever
    /// invoked it, which is why a finished seed had frames on disk inside a
    /// container and nothing durable in Workshop.
    pub(super) async fn import_container_trace(
        &self,
        container_id: &str,
        rollout_id: &str,
        run_id: &str,
        trial_id: &str,
    ) -> Result<Value> {
        let data = crate::data::DataStore::new(self.db.clone(), self.visuals.content().clone());
        let (mut result, event, frames) =
            crate::visuals_ipc::import_container_trace_into(&data, container_id, rollout_id)
                .await?;
        for frame in &frames {
            let cas_digest = self
                .content()
                .put_bytes("eval_frames", &frame.bytes)
                .context("store bundled Trace V5 frame in eval CAS")?;
            let expected = frame
                .digest
                .strip_prefix("sha256:")
                .unwrap_or(&frame.digest);
            if cas_digest != expected {
                bail!("bundled Trace V5 frame changed digest during CAS import");
            }
            self.record_run_media(
                run_id,
                &RunMediaRow {
                    cas_digest,
                    kind: "eval_frames",
                    media_type: "image/png",
                    byte_size: frame.bytes.len() as u64,
                    width: Some(frame.width),
                    height: Some(frame.height),
                    rollout_id: Some(rollout_id.to_string()),
                    trial_id: Some(trial_id.to_string()),
                    step: Some(frame.step),
                    producer_digest: frame.producer_digest.clone(),
                },
            )
            .await?;
        }
        result["importedFrameCount"] = json!(frames.len());
        result["importedFrameSteps"] =
            json!(frames.iter().map(|frame| frame.step).collect::<Vec<_>>());
        let run = self.get(run_id.to_string()).await?;
        super::container_eval::bind_imported_trace_provenance(&mut result, &run.summary)?;
        result["imported"] = json!(true);
        if let Some(event) = event.filter(|_| self.events_tx.receiver_count() > 0) {
            self.events_tx
                .send(event)
                .map_err(|error| anyhow!("publish imported optimizer frame event: {error}"))?;
        }
        Ok(result)
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
            json!({"id":"eval","title":"Eval","availability":"available","description":"Baseline and comparative evaluation. An evaluation is an optimizer run whose algorithm is eval."}),
            json!({"id":"gepa","title":"GEPA","availability":"available","description":"Genetic-Pareto prompt optimization"}),
            json!({"id":"go-ex","title":"GELO","availability":"available","description":"Hosted GO-EX exploration. Canonical algorithm id is go-ex; GELO is the display label and recipe name gelo.craftax.hosted.v1."}),
            json!({"id":"sft","title":"SFT","kind":"training","availability":"available","description":"Supervised fine-tuning. Local MLX on this Mac or hosted through the public Optimizers SFT service. Both placements share one SFT projection."}),
            json!({"id":"cispo","title":"CISPO","kind":"training","availability":"available","description":"On-policy CISPO. Local MLX on this Mac, or hosted slime.v1. Both placements share one CISPO projection."}),
        ]
    }

    pub fn list_recipes(&self) -> Vec<Value> {
        self.list_recipes_for_session(None)
    }

    pub fn list_recipes_for_session(&self, session_ref: Option<&str>) -> Vec<Value> {
        let mut recipes = Vec::new();
        if let Some(session) = session_ref.map(str::trim).filter(|value| !value.is_empty()) {
            // Diagnostics are catalog entries, never discarded: a declared
            // recipe that fails validation must not silently disappear and be
            // reported later as `unknown optimizer recipe`.
            match super::workspace_recipe::session_workspace(&self.db, session) {
                Ok(Some(workspace)) => {
                    match super::workspace_recipe::load_recipes_with_diagnostics(&workspace) {
                        Ok(outcome) => {
                            recipes.extend(
                                outcome
                                    .recipes
                                    .iter()
                                    .map(super::workspace_recipe::catalog_entry),
                            );
                            recipes.extend(
                                outcome
                                    .diagnostics
                                    .iter()
                                    .map(super::workspace_recipe::invalid_catalog_entry),
                            );
                        }
                        Err(error) => recipes.push(json!({
                            "source": "workspace",
                            "availability": "unavailable",
                            "availabilityReason": format!(
                                "workspace recipe catalog could not be read: {error:#}"
                            ),
                            "diagnosticCode": "workspace_recipes_unreadable",
                        })),
                    }
                }
                Ok(None) => {}
                Err(error) => recipes.push(json!({
                    "source": "workspace",
                    "availability": "unavailable",
                    "availabilityReason": format!(
                        "workspace recipe catalog is unavailable: the session workspace could \
                         not be resolved: {error:#}"
                    ),
                    "diagnosticCode": "workspace_unavailable",
                })),
            }
        }
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
            _ => super::recipes::start(self, request).await,
        }
    }

    /// Freeze workspace policy source into an immutable content-addressed set
    /// before any local eval recipe can start.
    pub async fn stage_eval_candidates(
        &self,
        request: super::eval_candidates::EvalStageCandidatesRequest,
    ) -> Result<Value> {
        super::eval_candidates::stage(request).await
    }

    pub async fn prepare_recipe(
        &self,
        request: super::models::OptimizerRecipeRunRequest,
    ) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
        super::recipes::prepare(self, request).await
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

    /// Settle the run through its durable algorithm projection.
    ///
    /// Files and mutable summary JSON are evidence only. They never select a
    /// result type or substitute for the algorithm-owned projection.
    pub async fn get_result(&self, optimizer_run_id: String) -> Result<Value> {
        let run = self.get(optimizer_run_id.clone()).await?;
        let manifest = self.terminal_manifest(optimizer_run_id.clone()).await?;
        let db = self.db.clone();
        let state = db
            .run(move |conn| {
                super::kernel::persist::load_state(conn, &optimizer_run_id)?.ok_or_else(|| {
                    anyhow!(
                        "optimizer run {} has no durable kernel projection",
                        optimizer_run_id
                    )
                })
            })
            .await?;
        let settled = super::kernel::settle_result(&state).map_err(|error| anyhow!("{error}"))?;
        results::from_kernel(&run, &state, settled, manifest.as_ref())
    }

    pub(super) async fn register_local_recipe(&self, run_id: String, cancel: super::CancelSignal) {
        self.local_recipes.lock().await.insert(run_id, cancel);
    }

    /// Claim one local run watcher without replacing an existing owner.
    ///
    /// Refresh/get-result calls may race the watcher started with the recipe.
    /// Replacing its cancellation sender and starting a second poller makes
    /// both workers append the same durable event page concurrently, which can
    /// turn a healthy local training run into a transient SQLite-lock failure.
    pub(super) async fn try_register_local_recipe(
        &self,
        run_id: String,
        cancel: super::CancelSignal,
    ) -> bool {
        let mut recipes = self.local_recipes.lock().await;
        if recipes.contains_key(&run_id) {
            return false;
        }
        recipes.insert(run_id, cancel);
        true
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
            if is_terminal_status(&run.status) {
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
            let eval_status_before = run.summary.get("evalStatus").cloned();
            let progress_before = run.summary.get("progress").cloned();
            patch(&mut run)?;
            // Once a terminal manifest exists, the summary progress lane is
            // frozen by the sealing transaction. A racing worker projection
            // must not rewrite it back to a pre-terminal reading — this was
            // how `evalStatus=running` survived forever beside a sealed run.
            let progress_changed = run.summary.get("evalStatus") != eval_status_before.as_ref()
                || run.summary.get("progress") != progress_before.as_ref();
            if progress_changed && terminal::load(conn, &run.id)?.is_some() {
                anyhow::bail!(
                    "optimizer run {} has a sealed terminal manifest; refusing a post-terminal \
                     summary progress rewrite",
                    run.id
                );
            }
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

    /// Copy the in-memory credential receipt chain onto the durable run record.
    pub(super) async fn persist_credential_chain(&self, run_id: &str) -> Result<()> {
        let Some(secrets) = crate::secrets::live() else {
            return Ok(());
        };
        let Some(chain) = secrets.chain_for_run(run_id) else {
            return Ok(());
        };
        let chain = chain.clone();
        self.patch_run(run_id.to_string(), move |run| {
            if let Some(object) = run.summary.as_object_mut() {
                object.insert("credentialChain".into(), chain);
            }
            Ok(())
        })
        .await?;
        Ok(())
    }

    /// Revoke the run's capability and seal that fact into the run summary
    /// before the terminal event is appended.
    pub(super) async fn seal_credential_chain(&self, run_id: &str) -> Result<()> {
        let Some(secrets) = crate::secrets::live() else {
            return Ok(());
        };
        let Some(chain) = secrets.seal_run_chain(run_id)? else {
            return Ok(());
        };
        let chain = chain.clone();
        let provider_usage = chain.get("providerUsage").cloned();
        self.patch_run(run_id.to_string(), move |run| {
            if let Some(object) = run.summary.as_object_mut() {
                object.insert("credentialChain".into(), chain);
                if let Some(provider_usage) = provider_usage.clone() {
                    object.insert("providerUsage".into(), provider_usage.clone());
                    let usage_lanes = object
                        .entry("usageLanes")
                        .or_insert_with(|| serde_json::json!({}));
                    if let Some(lanes) = usage_lanes.as_object_mut() {
                        lanes.insert("provider".into(), provider_usage.clone());
                    }
                    run.usage
                        .extra
                        .insert("providerUsage".into(), provider_usage);
                }
            }
            Ok(())
        })
        .await?;
        Ok(())
    }

    pub async fn list(&self, query: OptimizerQuery) -> Result<Vec<OptimizerRunRecord>> {
        let db = self.db.clone();
        db.run(move |conn| list_runs(conn, &query)).await
    }

    pub async fn get(&self, optimizer_run_id: String) -> Result<OptimizerRunRecord> {
        let db = self.db.clone();
        db.run(move |conn| load_run(conn, &optimizer_run_id)).await
    }

    pub(super) fn negotiate_effective_contract(
        &self,
        optimizer_run_id: &str,
        container_id: &str,
        task_family: Option<&str>,
        metadata: &Value,
    ) -> Result<EffectiveContract> {
        let templates = self.visuals.list_templates(None)?;
        super::effective_contract::negotiate(
            optimizer_run_id,
            container_id,
            task_family,
            metadata,
            &templates,
        )
    }

    pub async fn artifacts_list(
        &self,
        optimizer_run_id: String,
        after_sequence: u64,
        limit: Option<i64>,
    ) -> Result<OptimizerArtifactPage> {
        let db = self.db.clone();
        db.run(move |conn| {
            load_run(conn, &optimizer_run_id)?;
            super::artifacts::list(
                conn,
                &optimizer_run_id,
                after_sequence,
                limit.unwrap_or(100),
            )
        })
        .await
    }

    pub async fn artifact_read_range(
        &self,
        optimizer_run_id: String,
        artifact_id: String,
        offset: u64,
        length: u64,
    ) -> Result<OptimizerArtifactRange> {
        let db = self.db.clone();
        db.run(move |conn| {
            super::artifacts::read_range(conn, &optimizer_run_id, &artifact_id, offset, length)
        })
        .await
    }

    /// Versioned backend projection. Raw events do not determine this view.
    pub async fn run_view_v2(
        &self,
        optimizer_run_id: String,
    ) -> Result<super::kernel::OptimizerRunViewV2> {
        let db = self.db.clone();
        db.run_transaction(move |conn| {
            let run = load_run(conn, &optimizer_run_id)?;
            if let Some(state) = super::kernel::persist::load_state(conn, &optimizer_run_id)? {
                let context = run_view_context(conn, &run)?;
                return Ok(super::kernel::project_view_with_context(&state, &context));
            }

            // One-time repair for a historical row that predates the kernel
            // projection. Replay happens here in CoreRuntime and is committed
            // before the view is returned; the renderer never receives raw
            // events as a competing state authority.
            super::kernel::AlgorithmKind::parse_wire(&run.algorithm_id)
                .map_err(|error| anyhow!("{error}"))?;
            let events = load_events_upto(conn, &run.id, run.cursor_seq)?;
            persist_kernel_projection(conn, &run, &events)?;
            let state =
                super::kernel::persist::load_state(conn, &optimizer_run_id)?.ok_or_else(|| {
                    anyhow!(
                        "optimizer run {} did not produce a durable kernel projection",
                        optimizer_run_id
                    )
                })?;
            let context = run_view_context(conn, &run)?;
            Ok(super::kernel::project_view_with_context(&state, &context))
        })
        .await
    }

    pub async fn create(
        &self,
        request: OptimizerCreateRequest,
    ) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
        if request.seed_fixture.is_some() {
            bail!("optimizer seed fixtures are not available in production");
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
        let algorithm = super::kernel::AlgorithmKind::parse_wire(&request.algorithm_id)
            .map_err(|error| anyhow!("{error}"))?;
        let algorithm_id = algorithm.wire_id().to_string();
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
                let event = persist_admitted_run(
                    conn,
                    &inserted,
                    format!("workshop:authorized-run-start:{}", inserted.id),
                )?;
                Ok((inserted, event))
            })
            .await?;
        if request.open_visual.unwrap_or(true) {
            run = self.open_visual(run.id.clone()).await?.0;
        }
        Ok((run, Some(event)))
    }

    /// Create an eval optimizer run only by consuming an approved kernel draft.
    ///
    /// Admission, the run row, the sealed spec, and worker progress are one
    /// transaction. No optimizer run exists if that transaction rolls back.
    pub async fn create_admitted_eval(
        &self,
        mut request: OptimizerCreateRequest,
        approved: super::admission::ApprovedExecutionSpec,
        declared_rollouts: usize,
    ) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
        let now = Utc::now().to_rfc3339();
        let id = request
            .id
            .clone()
            .unwrap_or_else(|| format!("opt_{}", Uuid::new_v4()));
        let draft_id = format!("draft_{id}");
        if let Some(object) = request
            .summary
            .get_or_insert_with(|| json!({}))
            .as_object_mut()
        {
            object.insert(
                "executionSpecDigest".into(),
                json!(approved.digest().as_str()),
            );
        }
        let capabilities = request
            .capabilities
            .clone()
            .unwrap_or_else(|| OptimizerCapabilities::for_algorithm("eval"));
        let binding = approved.binding();
        let approved_rollouts = u64::from(binding.approved_rollouts.0.get());
        anyhow::ensure!(
            declared_rollouts as u64 == approved_rollouts,
            "declared rollout count {declared_rollouts} does not match approved rollout count {approved_rollouts}"
        );
        let mut usage = OptimizerUsageSummary::default();
        usage.extra.insert(
            "paidComputeApproval".into(),
            json!({
                "approvalId": binding.receipt_id.as_str(),
                "cap": {
                    "maxCostUsdMicros": binding.approved_cost_micros.0.get(),
                    "maxRollouts": approved_rollouts,
                },
                "receiptViolation": false,
            }),
        );
        let run = OptimizerRunRecord {
            schema_version: OPTIMIZER_RUN_SCHEMA_VERSION.into(),
            id: id.clone(),
            algorithm_id: "eval".into(),
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
            usage,
            error: None,
        };
        let db = self.db.clone();
        db.run_transaction(move |conn| {
            super::admission::stage_approved_eval_draft(conn, &draft_id, &approved)?;
            let event = persist_new_run(conn, &run)?;
            super::admission::consume_approved_eval_draft(conn, &draft_id, &run.id, &now)?;
            Ok((run, Some(event)))
        })
        .await
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
        run = reconcile_via_driver(self, run).await?;
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

    /// Re-ingest already-sealed evidence for a terminal inline evaluation and
    /// rebuild its authoritative projections. This operation never starts a
    /// rollout and never accesses provider credentials.
    pub async fn reconcile_evaluation_evidence(
        &self,
        optimizer_run_id: String,
    ) -> Result<OptimizerRunRecord> {
        super::container_eval::reconcile_evidence(self, &optimizer_run_id).await
    }

    /// After a process restart, locally persisted `running`/`queued`/`paused`
    /// projections can be a lie. Walk them and let each algorithm's durable
    /// worker log win before the renderer hydrates Outputs.
    pub async fn reconcile_stale_local_runs(&self) -> Result<Vec<OptimizerRunRecord>> {
        let db = self.db.clone();
        let instance_id = crate::instance::boot_epoch().to_string();
        let recovered = db
            .run_transaction(move |conn| {
                reconcile_stale_local_runs_in_tx(conn, &instance_id, Utc::now())
            })
            .await?;
        self.sweep_projection_outbox(None, None).await?;
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
        let mut events = db
            .run(move |conn| {
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
            .await?;
        super::strip_frame_bodies_for_ipc(&mut events);
        Ok(events)
    }

    /// Latest changed native frame per seed after a durable frame cursor.
    /// Base64 bodies never enter the shared optimizer-event subscription.
    pub async fn frames_latest(
        &self,
        optimizer_run_id: String,
        after_frame_sequence: u64,
    ) -> Result<super::OptimizerFrameDelta> {
        let db = self.db.clone();
        db.run(move |conn| super::frames::latest(conn, &optimizer_run_id, after_frame_sequence))
            .await
    }

    /// Bounded newest-first frame metadata for one seed. Bodies are fetched
    /// separately when a thumbnail or scrubber position is actually viewed.
    pub async fn frames_list(
        &self,
        optimizer_run_id: String,
        seed: i64,
        before_frame_sequence: Option<u64>,
        limit: Option<i64>,
    ) -> Result<Vec<super::OptimizerFrameRef>> {
        let db = self.db.clone();
        db.run(move |conn| {
            super::frames::list(
                conn,
                &optimizer_run_id,
                seed,
                before_frame_sequence,
                limit.unwrap_or(100),
            )
        })
        .await
    }

    /// Verified PNG content for one catalogued frame.
    pub async fn frame_content(
        &self,
        optimizer_run_id: String,
        seed: i64,
        frame_sequence: u64,
    ) -> Result<super::OptimizerFrameContent> {
        let db = self.db.clone();
        let store = self.frame_store.clone();
        db.run(move |conn| {
            super::frames::content(conn, &store, &optimizer_run_id, seed, frame_sequence)
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
        let delivery_run_id = optimizer_run_id.clone();
        let db = self.db.clone();
        let frame_store = self.frame_store.clone();
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
                commit_validated_events(
                    conn,
                    &frame_store,
                    run,
                    envelopes,
                    SequenceContract::ServiceAllocated,
                )
            })
            .await?;
        self.sweep_projection_outbox(Some(delivery_run_id), result.1.as_ref())
            .await?;
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
        let delivery_run_id = optimizer_run_id.clone();
        let db = self.db.clone();
        let frame_store = self.frame_store.clone();
        let result = db
            .run_transaction(move |conn| {
                let run = load_run(conn, &optimizer_run_id)?;
                commit_validated_events(conn, &frame_store, run, events, contract)
            })
            .await?;
        self.sweep_projection_outbox(Some(delivery_run_id), result.1.as_ref())
            .await?;
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
        let mut publication_metadata = request.metadata.as_object().cloned().unwrap_or_default();
        publication_metadata.insert(
            "optimizerVisualRole".into(),
            Value::String(request.role.clone()),
        );

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
                        metadata: Some(Value::Object(publication_metadata)),
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
        let summary_role = request.role.clone();
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
                // `visualId` names the run's *primary* pane, and only that.
                // A run may own several chat visuals — an overview and the
                // workstation a seed row opens — and letting whichever
                // published last claim this key would silently repoint the
                // eval result's own evidence reference at a drill-down.
                if summary_role == "primary" {
                    summary.insert("visualId".into(), json!(bound_id));
                }
                // Secondary panes are addressed by role, so a consumer asks for
                // the one it means instead of guessing from a list.
                let mut by_role = summary
                    .get("visualIds")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                by_role.insert(summary_role.clone(), json!(bound_id));
                summary.insert("visualIds".into(), Value::Object(by_role));
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
    /// Settle a run through one typed command.
    ///
    /// The terminal event is appended through the standard transactional
    /// append, so sealing, projection persistence, and the summary rewrite
    /// all ride the same commit. A second settlement with the same terminal
    /// kind is idempotent success; a different kind is a typed refusal. After
    /// the terminal fact is durable, the run's provider capabilities are
    /// revoked and the revocation is journaled on the amendment lane —
    /// consequence of settlement, never its cause.
    pub(super) async fn settle_run(
        &self,
        optimizer_run_id: String,
        cause: super::kernel::SettleCause,
        error: Option<Value>,
    ) -> Result<OptimizerRunRecord> {
        if let Some(manifest) = self.terminal_manifest(optimizer_run_id.clone()).await? {
            cause
                .accept_sealed(manifest_terminal_kind(&manifest), &optimizer_run_id)
                .map_err(|error| anyhow!("{error}"))?;
            // A compatible concurrent settlement still participates in the
            // post-terminal cleanup. Revocation is idempotent, and this makes
            // a crash between seal and cleanup repairable by retrying the
            // settlement command.
            self.revoke_credentials_post_terminal(&optimizer_run_id, cause.cancellation())
                .await;
            return self.get(optimizer_run_id).await;
        }
        let run = self.get(optimizer_run_id.clone()).await?;
        if let Some(request) = cause.cancellation() {
            self.record_cancellation_request(&optimizer_run_id, request)
                .await?;
            let request_event =
                OptimizerEventDraft::new("optimizer.run.cancel.requested", &run.algorithm_id)
                    .idempotency_key(format!("cancel-request:{}", request.request_id))
                    .delta(Map::from_iter([(
                        "cancellation".into(),
                        json!(request.as_ref()),
                    )]))
                    .raw(json!({ "source": "settle_run" }));
            self.append_event_payloads(optimizer_run_id.clone(), vec![request_event])
                .await?;
        }
        // The error rides the terminal event itself, so `error_json` and the
        // manifest's `error` are written by the same transaction. Evidence
        // degradation also remains in the historical delta shape consumed by
        // reducers; it is duplicated deliberately, not written later.
        let error_payload = error.or_else(|| {
            cause
                .detail()
                .filter(|detail| !detail.trim().is_empty())
                .map(|detail| json!({ "message": detail }))
        });
        let mut delta = Map::from_iter([("status".into(), json!(cause.status()))]);
        if let Some(request) = cause.cancellation() {
            delta.insert("cancellation".into(), json!(request.as_ref()));
        }
        if cause.kind() == super::kernel::TerminalKind::Degraded {
            if let Some(degradation) = error_payload.as_ref() {
                delta.insert("degradation".into(), degradation.clone());
            }
        }
        let mut draft = OptimizerEventDraft::new(cause.event_type(), &run.algorithm_id)
            .idempotency_key(format!("settle:{}", cause.status()))
            .delta(delta)
            .raw(json!({ "source": "settle_run" }));
        draft = match cause.kind() {
            super::kernel::TerminalKind::Failed => draft.level("error"),
            super::kernel::TerminalKind::Degraded => draft.level("warn"),
            _ => draft,
        };
        if let Some(error_payload) = error_payload {
            draft = draft.error(error_payload);
        }
        let run = match self
            .append_event_payloads(optimizer_run_id.clone(), vec![draft])
            .await
        {
            Ok((run, _)) => run,
            Err(append_error) => {
                // A concurrent writer may have sealed between the check and
                // the append. Its seal wins when compatible; otherwise the
                // refusal stands.
                let Some(manifest) = self.terminal_manifest(optimizer_run_id.clone()).await? else {
                    return Err(append_error);
                };
                cause
                    .accept_sealed(manifest_terminal_kind(&manifest), &optimizer_run_id)
                    .map_err(|error| anyhow!("{error}"))?;
                self.get(optimizer_run_id.clone()).await?
            }
        };
        self.revoke_credentials_post_terminal(&optimizer_run_id, cause.cancellation())
            .await;
        // Return the post-cleanup row: the revocation amendment advances the
        // final cursor and the credential chain is now sealed in the summary.
        self.get(optimizer_run_id).await.or(Ok(run))
    }

    /// F3: capability revocation lives inside settlement. Runs only after the
    /// terminal event is durable; the revocation is recorded on the amendment
    /// lane linked to the terminal sequence, so the journal proves revocation
    /// was a consequence of the run ending. Best-effort: a missing vault or a
    /// failed amendment append never un-settles a settled run.
    async fn revoke_credentials_post_terminal(
        &self,
        optimizer_run_id: &str,
        cancellation: Option<&std::sync::Arc<super::kernel::CancellationRequest>>,
    ) {
        let Some(secrets) = crate::secrets::live() else {
            return;
        };
        let capability_ids = match secrets.revoke_run(optimizer_run_id) {
            Ok(ids) => ids,
            Err(error) => {
                crate::platform::logging::report(
                    "optimizers",
                    "eprintln",
                    format!("revoke credentials for settled run {optimizer_run_id}: {error:#}"),
                );
                return;
            }
        };
        if capability_ids.is_empty() {
            // A retry after revocation still has to seal/persist the chain;
            // there may be no newly-revoked ids on this attempt.
            if let Err(error) = self.seal_credential_chain(optimizer_run_id).await {
                crate::platform::logging::report(
                    "optimizers",
                    "eprintln",
                    format!("seal credential chain for settled run {optimizer_run_id}: {error:#}"),
                );
            }
            return;
        }
        let terminal_sequence = match self.terminal_manifest(optimizer_run_id.to_string()).await {
            Ok(Some(manifest)) => manifest.get("terminalCursor").and_then(Value::as_u64),
            _ => None,
        };
        let Some(terminal_sequence) = terminal_sequence else {
            return;
        };
        let run = match self.get(optimizer_run_id.to_string()).await {
            Ok(run) => run,
            Err(_) => return,
        };
        let draft =
            credential_revocation_amendment(&run, terminal_sequence, capability_ids, cancellation);
        if let Err(error) = self
            .append_event_payloads(optimizer_run_id.to_string(), vec![draft])
            .await
        {
            crate::platform::logging::report(
                "optimizers",
                "eprintln",
                format!(
                    "journal credential revocation for settled run {optimizer_run_id}: {error:#}"
                ),
            );
        }
        if let Err(error) = self.seal_credential_chain(optimizer_run_id).await {
            crate::platform::logging::report(
                "optimizers",
                "eprintln",
                format!("seal credential chain for settled run {optimizer_run_id}: {error:#}"),
            );
        }
    }

    pub(super) async fn settle_evidence_degraded(
        &self,
        optimizer_run_id: String,
        stage: &str,
        reason: String,
    ) -> Result<OptimizerRunRecord> {
        use sha2::{Digest, Sha256};

        let run = self.get(optimizer_run_id.clone()).await?;
        let state = self
            .db
            .clone()
            .run({
                let optimizer_run_id = optimizer_run_id.clone();
                move |conn| super::kernel::persist::load_state(conn, &optimizer_run_id)
            })
            .await?
            .context("evidence degradation requires a durable kernel projection")?;
        let observed_at = Utc::now().to_rfc3339();
        let degradation = json!({
            "stage": stage,
            "reason": reason.clone(),
            "observedAt": observed_at,
            "retryable": true,
            "paidComputePreserved": true,
        });
        let draft = if let Some(terminal) = state.terminal.as_ref() {
            let fingerprint = Sha256::digest(
                format!("{}\0{}\0{}", terminal.final_sequence, stage, reason).as_bytes(),
            );
            OptimizerEventDraft::new("optimizer.evidence.amended", &run.algorithm_id)
                .idempotency_key(format!("evidence-amendment:{fingerprint:x}"))
                .level("warn")
                .delta(Map::from_iter([
                    ("terminalSequence".into(), json!(terminal.final_sequence)),
                    ("degradation".into(), degradation),
                ]))
                .raw(json!({ "source": "core_runtime" }))
        } else {
            return self
                .settle_run(
                    optimizer_run_id,
                    super::kernel::SettleCause::Degraded {
                        detail: reason.clone(),
                    },
                    Some(degradation),
                )
                .await;
        };
        let (run, _) = self
            .append_event_payloads(optimizer_run_id, vec![draft])
            .await?;
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
                if cached.cursor_seq == cursor
                    && cached.projection_schema_version == super::kernel::RUN_VIEW_SCHEMA_VERSION
                {
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

    pub async fn cancel(
        &self,
        id: String,
        request: super::kernel::CancellationRequest,
    ) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
        let request = std::sync::Arc::new(request);
        let run = self.get(id.clone()).await?;
        if run.source == "cloud" {
            self.record_cancellation_request(&id, &request).await?;
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
        // A run already sealed cancelled treats a second cancel as idempotent
        // success; any other sealed outcome refuses — cancel cannot re-decide
        // a settled record.
        if let Some(manifest) = self.terminal_manifest(id.clone()).await? {
            return match manifest_terminal_kind(&manifest) {
                super::kernel::TerminalKind::Cancelled => Ok((run, None)),
                sealed => Err(anyhow!(
                    "cancel is not available for a run sealed {}",
                    sealed.as_str()
                )),
            };
        }
        // The request becomes durable before anything acts on it: a receipt
        // row, and a journal fact that owns a sequence. The sealing
        // transaction later backfills `settled_sequence`, turning the request
        // into a receipt.
        self.record_cancellation_request(&id, &request).await?;
        let drafts = vec![
            OptimizerEventDraft::new("optimizer.run.cancel.requested", &run.algorithm_id)
                .idempotency_key(format!("cancel-request:{}", request.request_id))
                .delta(Map::from_iter([(
                    "cancellation".into(),
                    json!(request.as_ref()),
                )]))
                .raw(json!({ "source": "cancel" })),
            OptimizerEventDraft::new("optimizer.run.cancelling", &run.algorithm_id)
                .idempotency_key("cancel:cancelling")
                .delta(Map::from_iter([("status".into(), json!("cancelling"))]))
                .raw(json!({ "source": "cancel" })),
        ];
        let _ = self.append_event_payloads(id.clone(), drafts).await;
        if let Some(cancel) = self.local_recipes.lock().await.get(&id).cloned() {
            if cancel.send(Some(request.clone())).is_ok() {
                // A live worker owns settlement: it drains its children and
                // routes through settle_run. The row is not written here.
                return Ok((self.get(id).await?, None));
            }
            // The sender is registered but its worker is gone; fall through
            // and settle directly.
        }
        if matches!(run.algorithm_id.as_str(), "sft" | "cispo") {
            if let Ok(client) =
                super::sidecar_training::SidecarTrainingClient::from_manager(self.manager()).await
            {
                let _ = client.cancel(&id).await;
            }
        }
        let run = self
            .settle_run(
                id,
                super::kernel::SettleCause::Cancelled {
                    request: request.clone(),
                },
                None,
            )
            .await?;
        Ok((run, None))
    }

    /// Durably record one typed cancellation request. Idempotent on
    /// request_id; `settled_sequence` stays NULL until the sealing
    /// transaction backfills it.
    async fn record_cancellation_request(
        &self,
        run_id: &str,
        request: &std::sync::Arc<super::kernel::CancellationRequest>,
    ) -> Result<()> {
        let db = self.db.clone();
        let run_id = run_id.to_string();
        let request = request.clone();
        db.run_transaction(move |conn| {
            conn.execute(
                "INSERT OR IGNORE INTO optimizer_cancellation_requests(
                    request_id, run_id, cause, requested_by, requested_at, scope, reason_code
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7)",
                params![
                    request.request_id,
                    run_id,
                    request.cause.as_str(),
                    request.requested_by,
                    request.requested_at,
                    request.scope,
                    request.reason_code,
                ],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn pause(&self, id: String) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
        let run = self.get(id.clone()).await?;
        validate_control(&run, "pause", OptimizerRunStatus::Paused)?;
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
        validate_control(&run, "resume", OptimizerRunStatus::Running)?;
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
            "inputs": [{
                "input": "optimizer_run",
                "kind": "optimizer_run",
                "source": run.id,
                "schema": OPTIMIZER_RUN_SCHEMA_VERSION
            }]
        });
        let existing = run
            .visual_refs
            .iter()
            .find(|r| r.kind == "visual" && r.role.as_deref() == Some("primary"))
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
        let template_id = run
            .summary
            .pointer("/effectiveContract/primaryVisual/templateId")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| negotiate_visual_template(&run.algorithm_id));
        // A persisted contract may only name a template still registered in
        // this instance. Managed-template removal is an explicit empty-state
        // failure, not an invitation to silently select another family.
        self.visuals.get_template(&template_id)?;
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
                        "templateDigest": template_digest,
                        "optimizerVisualRole": "primary"
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
                session_id: presentation_session_ref.clone(),
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
        if let Some(trace_template) = run
            .summary
            .pointer("/effectiveContract/traceVisual/templateId")
            .and_then(Value::as_str)
            .filter(|template| *template != template_id)
        {
            let trace_template = trace_template.to_string();
            self.visuals.get_template(&trace_template)?;
            let _ = self
                .publish_chat_owned_visual(ChatVisualPublication {
                    run_id: run.id.clone(),
                    session_ref: presentation_session_ref.clone(),
                    template_id: trace_template,
                    title: format!("{} · trace", algorithm_label(&run.algorithm_id)),
                    bindings: json!({
                        "schemaVersion": VISUAL_BINDINGS_SCHEMA_VERSION,
                        "inputs": [{
                            "input": "optimizer_run",
                            "kind": "optimizer_run",
                            "source": run.id,
                            "schema": OPTIMIZER_RUN_SCHEMA_VERSION
                        }]
                    }),
                    metadata: json!({
                        "optimizerRunId": run.id,
                        "effectiveContract": EFFECTIVE_CONTRACT_SCHEMA_VERSION,
                        "emptyState": "waiting_for_declared_trace_events"
                    }),
                    status: crate::visuals::VisualStatus::Live,
                    role: "trace".into(),
                })
                .await?;
        }
        Ok((self.get(run.id).await?, event))
    }


    /// Import a local OSS GEPA event feed or optimizers-beta/GELO workspace.
    pub async fn import_local(
        &self,
        request: super::models::OptimizerImportLocalRequest,
    ) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
        let imported = super::local::import_local_path(&request.path)?;
        let algorithm_id = super::kernel::AlgorithmKind::parse_wire(&imported.algorithm_id)
            .map_err(|error| anyhow!("{error}"))?
            .wire_id()
            .to_string();
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
            persist_admission_not_required_run(conn, &seed, "local-import")?;
            upsert_cursor(conn, &seed.id, 0, &seed.created_at)?;
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
        let is_new_mirror = existing.is_none();
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
            if is_new_mirror {
                persist_admission_not_required_run(conn, &seed, "cloud-attach")?;
            } else {
                upsert_run(conn, &seed)?;
            }
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
            // Hosted terminal facts enter the same settlement command as
            // local ones. Events before the terminal preserve their assigned
            // order; genuinely later producer facts become linked
            // amendments instead of violating the sealed-run journal.
            let terminal_at = events.iter().position(|event| {
                matches!(
                    event.event_type.as_str(),
                    "optimizer.run.completed"
                        | "optimizer.run.failed"
                        | "optimizer.run.degraded"
                        | "optimizer.run.cancelled"
                )
            });
            let (mut run, event) = if let Some(terminal_at) = terminal_at {
                let terminal = events[terminal_at].clone();
                let before = events[..terminal_at].to_vec();
                let after = events[terminal_at + 1..].to_vec();
                let event = if before.is_empty() {
                    None
                } else {
                    self.append_events(id.clone(), before).await?.1
                };
                let cause = match terminal.event_type.as_str() {
                    "optimizer.run.completed" => super::kernel::SettleCause::Completed,
                    "optimizer.run.degraded" => super::kernel::SettleCause::Degraded {
                        detail: terminal
                            .error
                            .as_ref()
                            .and_then(|value| value.get("message"))
                            .and_then(Value::as_str)
                            .unwrap_or("hosted run degraded")
                            .to_string(),
                    },
                    "optimizer.run.cancelled" => super::kernel::SettleCause::Cancelled {
                        request: std::sync::Arc::new(super::kernel::CancellationRequest::new(
                            super::kernel::CancellationCause::ContainerRequested,
                            "cloud:remote",
                            format!("run:{id}"),
                        )),
                    },
                    _ => super::kernel::SettleCause::Failed {
                        detail: terminal
                            .error
                            .as_ref()
                            .and_then(|value| value.get("message"))
                            .and_then(Value::as_str)
                            .unwrap_or("hosted run failed")
                            .to_string(),
                    },
                };
                let mut settled = self
                    .settle_run(id.clone(), cause, terminal.error.clone())
                    .await?;
                if !after.is_empty() {
                    let terminal_sequence = self
                        .terminal_manifest(id.clone())
                        .await?
                        .and_then(|manifest| manifest.get("terminalCursor").and_then(Value::as_u64))
                        .context("hosted terminal seal is missing terminalCursor")?;
                    let amendments = after
                        .into_iter()
                        .map(|fact| {
                            OptimizerEventDraft::new("optimizer.evidence.amended", &algorithm_id)
                                .idempotency_key(format!(
                                    "cloud-post-terminal:{}",
                                    fact.event_id
                                        .clone()
                                        .unwrap_or_else(|| fact.sequence_number.to_string())
                                ))
                                .delta(Map::from_iter([
                                    ("terminalSequence".into(), json!(terminal_sequence)),
                                    ("postTerminalFact".into(), json!(fact)),
                                ]))
                                .raw(json!({"source":"cloud_reconcile"}))
                        })
                        .collect();
                    settled = self.append_event_payloads(id.clone(), amendments).await?.0;
                }
                (settled, event)
            } else {
                self.append_events(id.clone(), events).await?
            };
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
        let placement = query
            .placement
            .as_deref()
            .unwrap_or("all")
            .trim()
            .to_ascii_lowercase();
        let limit = query.limit.unwrap_or(50).clamp(1, 100);
        let offset = query.offset.unwrap_or(0);
        let include_local = matches!(placement.as_str(), "all" | "this_mac" | "local" | "mlx");
        let include_hosted = matches!(placement.as_str(), "all" | "hosted" | "cloud");
        let local = if include_local {
            let query = query.clone();
            self.db
                .run(move |conn| super::local_lora::search_local_loras(conn, &query))
                .await
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let hosted = if include_hosted {
            match super::cloud::CloudOptimizerClient::from_config() {
                Ok(client) => client
                    .search_saved_lora_checkpoints(query.clone())
                    .await
                    .map(|page| page.items)
                    .unwrap_or_default(),
                Err(_) => Vec::new(),
            }
        } else {
            Vec::new()
        };
        let hosted = self.project_hosted_loras(hosted).await;
        let mut items = local;
        items.extend(hosted);
        if let Some(provider) = query.provider.as_deref().filter(|value| *value != "all") {
            items.retain(|item| item.provider == provider);
        }
        let total = items.len() as u64;
        let items = items
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .collect::<Vec<_>>();
        Ok(super::SavedLoraCheckpointPage {
            schema_version: "saved_lora_checkpoint.page.v1".into(),
            items,
            total,
            limit,
            offset,
        })
    }

    pub async fn list_saved_lora_checkpoints_for_run(
        &self,
        run_id: String,
    ) -> Result<super::SavedLoraRunPage> {
        let mut page = super::cloud::CloudOptimizerClient::from_config()?
            .saved_lora_checkpoints_for_run(&run_id)
            .await?;
        page.items = self.project_hosted_loras(page.items).await;
        Ok(page)
    }

    pub async fn run_outputs(&self, run_id: String) -> Result<super::OptimizerRunOutputs> {
        let mut outputs = super::cloud::CloudOptimizerClient::from_config()?
            .run_outputs(&run_id)
            .await?;
        outputs.model_checkpoints = self.project_hosted_loras(outputs.model_checkpoints).await;
        Ok(outputs)
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
        let id = checkpoint_id.clone();
        let local = self
            .db
            .run(move |conn| super::local_lora::get_local_lora(conn, &id))
            .await?;
        if local.is_some() {
            let id = checkpoint_id;
            return self
                .db
                .run(move |conn| super::local_lora::archive_local_lora(conn, &id))
                .await;
        }
        super::cloud::CloudOptimizerClient::from_config()?
            .archive_saved_lora_checkpoint(&checkpoint_id)
            .await
            .map(annotate_hosted_lora)
    }

    pub async fn saved_lora_download(
        &self,
        checkpoint_id: String,
    ) -> Result<super::SavedLoraDownload> {
        let id = checkpoint_id.clone();
        if let Some(local) = self
            .db
            .run(move |conn| super::local_lora::get_local_lora(conn, &id))
            .await?
        {
            return Ok(super::SavedLoraDownload {
                checkpoint_id: local.checkpoint_id,
                url: format!("file://{}", local.storage.key),
                expires_in: 0,
                content_type: local.storage.content_type,
                size_bytes: local.storage.size_bytes,
                sha256: local.storage.sha256,
            });
        }
        super::cloud::CloudOptimizerClient::from_config()?
            .saved_lora_download(&checkpoint_id)
            .await
    }

    pub async fn import_saved_lora_dir(&self, path: String) -> Result<super::SavedLoraCheckpoint> {
        self.db
            .run(move |conn| super::local_lora::import_local_lora_dir(conn, Path::new(&path)))
            .await
    }

    pub async fn infer_saved_lora(&self, request: super::CheckpointInferRequest) -> Result<Value> {
        super::sidecar_training::infer_checkpoint(self, request, |_| {}).await
    }

    pub async fn infer_saved_lora_with_delta<F>(
        &self,
        request: super::CheckpointInferRequest,
        on_delta: F,
    ) -> Result<Value>
    where
        F: FnMut(&str) + Send,
    {
        super::sidecar_training::infer_checkpoint(self, request, on_delta).await
    }

    pub async fn patch_saved_lora(
        &self,
        checkpoint_id: String,
        patch: super::SavedLoraPatchRequest,
    ) -> Result<super::SavedLoraCheckpoint> {
        if self.get_local_lora(checkpoint_id.clone()).await?.is_some() {
            return self
                .db
                .run({
                    let checkpoint_id = checkpoint_id.clone();
                    let patch = patch.clone();
                    move |conn| super::local_lora::patch_local_lora(conn, &checkpoint_id, &patch)
                })
                .await;
        }
        let client = super::cloud::CloudOptimizerClient::from_config()?;
        match client
            .patch_saved_lora_checkpoint(&checkpoint_id, &patch)
            .await
        {
            Ok(row) => {
                let id = checkpoint_id.clone();
                let _ = self
                    .db
                    .run(move |conn| super::local_lora::clear_hosted_overlay(conn, &id))
                    .await;
                Ok(annotate_hosted_lora(row))
            }
            Err(patch_err) => match client.saved_lora_checkpoint(&checkpoint_id).await {
                Ok(base) => {
                    let id = checkpoint_id.clone();
                    let patch = patch.clone();
                    self.db
                        .run(move |conn| {
                            super::local_lora::overlay_hosted_lora(
                                conn,
                                &id,
                                &patch,
                                annotate_hosted_lora(base),
                            )
                        })
                        .await
                }
                Err(_) => {
                    let id = checkpoint_id.clone();
                    let overlay_patch = patch.clone();
                    let _ = self
                        .db
                        .run(move |conn| {
                            super::local_lora::upsert_hosted_overlay(conn, &id, &overlay_patch)
                        })
                        .await;
                    Err(patch_err)
                }
            },
        }
    }

    pub async fn publish_saved_lora(
        &self,
        checkpoint_id: String,
    ) -> Result<super::SavedLoraCheckpoint> {
        let local = self
            .get_local_lora(checkpoint_id.clone())
            .await?
            .ok_or_else(|| anyhow!("only This Mac adapters can be published"))?;
        let adapter = Path::new(&local.storage.key);
        if !adapter.is_dir() {
            bail!("adapter bytes are missing");
        }
        let (archive, sha256) = super::local_lora::zip_adapter_dir(adapter)?;
        let request = json!({
            "name": local.name,
            "description": local.description,
            "provider": "imported",
            "checkpoint_kind": local.checkpoint_kind,
            "visibility": "private",
            "base_model": local.base_model,
            "lora_rank": local.lora_rank,
            "step": local.step,
            "tags": local.tags,
            "metadata": {
                "source_digest": local.checkpoint_id,
                "placement": "this_mac"
            },
            "content_type": "application/zip"
        });
        super::cloud::CloudOptimizerClient::from_config()?
            .publish_saved_lora_archive(&archive, request, &sha256)
            .await
    }

    pub async fn upsert_local_lora_from_event(&self, run_id: String, payload: Value) -> Result<()> {
        let Some(row) =
            super::local_lora::LocalLoraUpsert::from_checkpoint_event(&run_id, &payload)
        else {
            return Ok(());
        };
        self.db
            .run(move |conn| super::local_lora::upsert_local_lora(conn, &row).map(|_| ()))
            .await
    }

    pub async fn get_local_lora(
        &self,
        checkpoint_id: String,
    ) -> Result<Option<super::SavedLoraCheckpoint>> {
        self.db
            .run(move |conn| super::local_lora::get_local_lora(conn, &checkpoint_id))
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
        let algorithm_id = super::kernel::AlgorithmKind::parse_wire(&request.algorithm_id)
            .map_err(|error| anyhow!("{error}"))?
            .wire_id()
            .to_string();
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
        let next = OptimizerRunStatus::parse(next_status)
            .ok_or_else(|| anyhow!("{next_status} is not an OptimizerRunStatus"))?;
        let db = self.db.clone();
        db.run_transaction(move |conn| {
            let mut run = load_run(conn, &optimizer_run_id)?;
            validate_control(&run, &command, next)?;
            run.status = next.as_str().to_string();
            if command == "resume" && run.started_at.is_none() {
                run.started_at = Some(Utc::now().to_rfc3339());
            }
            if command == "cancel" {
                run.finished_at = Some(Utc::now().to_rfc3339());
            }
            upsert_run(conn, &run)?;
            super::experiment_bind::settle_run(conn, &run)?;
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
            let state = super::kernel::persist::load_state(conn, &run_id)?.ok_or_else(|| {
                anyhow!("optimizer run {run_id} has no durable kernel projection")
            })?;
            if state.aggregate_sequence != at_seq {
                bail!(
                    "optimizer run {run_id} projection is at sequence {}, requested {at_seq}",
                    state.aggregate_sequence
                );
            }
            let mut slices = project_from_kernel(&run, &state)?;
            if let Some(only) = only {
                slices.retain(|slice| slice.slice_id == only);
            }
            Ok(slices)
        })
        .await
    }
}

fn project_from_kernel(
    run: &OptimizerRunRecord,
    state: &super::kernel::RunKernelState,
) -> Result<Vec<OptimizerStateSlice>> {
    if run.id != state.run_id {
        bail!(
            "optimizer state identity mismatch: record {} != projection {}",
            run.id,
            state.run_id
        );
    }
    let view = super::kernel::project_view(state);
    let view_json = serde_json::to_value(&view)?;
    let updated_at = state
        .terminal
        .as_ref()
        .map(|terminal| terminal.sealed_at.clone())
        .unwrap_or_else(|| run.created_at.clone());
    let mk = |slice_id: String, data: Value| OptimizerStateSlice {
        schema_version: OPTIMIZER_STATE_SLICE_SCHEMA_VERSION.into(),
        projection_schema_version: super::kernel::RUN_VIEW_SCHEMA_VERSION.into(),
        run_id: state.run_id.clone(),
        algorithm_id: state.algorithm.wire_id().into(),
        slice_id,
        cursor_seq: state.aggregate_sequence,
        updated_at: updated_at.clone(),
        data,
    };
    let result = state
        .projection
        .settle()
        .ok()
        .map(serde_json::to_value)
        .transpose()?
        .unwrap_or(Value::Null);
    Ok(vec![
        mk("run.view.v2".into(), view_json.clone()),
        mk("run.summary".into(), json!({ "view": view_json })),
        mk("run.usage".into(), serde_json::to_value(state.usage())?),
        mk(
            "run.work".into(),
            serde_json::to_value(state.work_summary())?,
        ),
        mk(
            "run.evidence".into(),
            serde_json::to_value(state.evidence_state())?,
        ),
        mk(
            "run.execution".into(),
            json!({ "placement": state.placement }),
        ),
        mk(
            format!("{}.projection", state.algorithm.wire_id()),
            serde_json::to_value(&state.projection)?,
        ),
        mk(format!("{}.result", state.algorithm.wire_id()), result),
    ])
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
        // Manager projections separate optimizer algorithms from eval
        // execution capabilities. Accept the raw handshake spelling as a
        // compatibility input, but never synthesize a claim from host data.
        .get("optimization_algorithms")
        .or_else(|| advertised.get("algorithms"))
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

fn annotate_hosted_lora(mut checkpoint: super::SavedLoraCheckpoint) -> super::SavedLoraCheckpoint {
    checkpoint.placement = "hosted".into();
    let inference = checkpoint.checkpoint_kind == "inference"
        && checkpoint.status == "ready"
        && checkpoint
            .provider_checkpoint_reference
            .as_deref()
            .or(checkpoint.lineage.provider_checkpoint_reference.as_deref())
            .is_some_and(|value| value.starts_with("tinker://"));
    checkpoint.inference_chat_completions = inference;
    checkpoint.inference_responses = inference;
    checkpoint
}

impl OptimizerService {
    async fn project_hosted_loras(
        &self,
        items: Vec<super::SavedLoraCheckpoint>,
    ) -> Vec<super::SavedLoraCheckpoint> {
        let overlays = self
            .db
            .run(|conn| super::local_lora::list_hosted_overlays(conn))
            .await
            .unwrap_or_default();
        items
            .into_iter()
            .map(|item| {
                let mut item = annotate_hosted_lora(item);
                if let Some(overlay) = overlays.get(&item.checkpoint_id) {
                    super::local_lora::apply_hosted_overlay(&mut item, overlay);
                }
                item
            })
            .collect()
    }
}

fn persist_new_run(conn: &Connection, run: &OptimizerRunRecord) -> Result<AppEvent> {
    upsert_run(conn, run)?;
    super::experiment_bind::attach_run(conn, run)?;
    if let Some(session_ref) = run.session_ref.as_deref() {
        insert_relationship(
            conn,
            &OptimizerRelationship {
                from_kind: "optimizer".into(),
                from_id: run.id.clone(),
                edge: "started_from".into(),
                to_kind: "session".into(),
                to_id: session_ref.into(),
                metadata: json!({}),
            },
        )?;
    }
    append_event(
        conn,
        EventAppend {
            event_id: None,
            session_id: run.session_ref.clone(),
            run_id: None,
            source: EventSource::System,
            kind: "optimizer.run.created".into(),
            payload: json!({
                "optimizerRunId": run.id,
                "algorithmId": run.algorithm_id,
                "status": run.status
            }),
            remote_sequence: None,
            command_id: None,
            created_at: None,
        },
    )
}

/// Seal an immutable run request and consume its approved draft in the same
/// transaction that creates the optimizer row.
///
/// The authorization reference names the already-authorized Workshop
/// operation that reached this service boundary. Imports and cloud attachment
/// use their dedicated paths and are recorded as admission-not-required there;
/// local algorithm starts must pass through this approved envelope.
fn persist_admitted_run(
    conn: &Connection,
    run: &OptimizerRunRecord,
    authorization_ref: String,
) -> Result<AppEvent> {
    persist_run_with_admission(conn, run, authorization_ref, true)
}

fn persist_admission_not_required_run(
    conn: &Connection,
    run: &OptimizerRunRecord,
    reason: &str,
) -> Result<AppEvent> {
    persist_run_with_admission(
        conn,
        run,
        format!("workshop:admission-not-required:{reason}"),
        false,
    )
}

fn persist_run_with_admission(
    conn: &Connection,
    run: &OptimizerRunRecord,
    authorization_ref: String,
    approval_required: bool,
) -> Result<AppEvent> {
    let algorithm = super::kernel::AlgorithmKind::parse_wire(&run.algorithm_id)
        .map_err(|error| anyhow!("{error}"))?;
    let placement = super::kernel::bridge::placement_from_run_source(algorithm, &run.source);
    let spec = super::admission::CanonicalJson::new(json!({
        "schemaVersion": "optimizer_authorized_run_start.v1",
        "algorithm": run.algorithm_id,
        "algorithmVersion": run.algorithm_version,
        "objective": run.objective,
        "source": run.source,
        "projectRef": run.project_ref,
        "executionBindings": run.execution_bindings,
        "inputRefs": run.input_refs,
        "summary": run.summary,
    }))
    .map_err(|error| anyhow!("canonicalize authorized run spec: {error}"))?;
    let spec_json = spec.to_canonical_string();
    let spec_digest = spec.digest().as_str().to_string();
    let now = run.created_at.clone();
    let draft_id = format!("draft_{}", run.id);
    let mut draft =
        super::kernel::RunDraft::new(&draft_id, algorithm, &spec_digest, spec_json, &now);
    draft.authorization_ref = Some(authorization_ref);
    let transitions: &[super::kernel::AdmissionState] = if approval_required {
        &[
            super::kernel::AdmissionState::Validating,
            super::kernel::AdmissionState::AwaitingApproval,
            super::kernel::AdmissionState::Approved,
        ]
    } else {
        &[super::kernel::AdmissionState::NotRequired]
    };
    for &next in transitions {
        draft
            .transition(next, &now)
            .map_err(|error| anyhow!("{error}"))?;
    }
    super::kernel::persist::insert_draft(conn, &draft)?;
    let event = persist_new_run(conn, run)?;
    let commit =
        super::kernel::AdmissionCommit::from_approved_draft(&draft, &run.id, placement, &now)
            .map_err(|error| anyhow!("{error}"))?;
    super::kernel::persist::consume_draft(conn, &draft_id, &now)?;
    super::kernel::persist::insert_spec(conn, &commit)?;
    super::kernel::persist::upsert_projection(
        conn,
        &super::kernel::RunKernelState::from_admission(&commit),
    )?;
    Ok(event)
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

fn run_view_context(
    conn: &Connection,
    run: &OptimizerRunRecord,
) -> Result<super::kernel::view::RunViewContext> {
    let mut context = super::kernel::view::RunViewContext::from(run);
    context.artifacts = super::artifacts::list_all(conn, &run.id)?;
    context.effective_contract = super::effective_contract::load(conn, &run.id)?;
    Ok(context)
}

fn upsert_run(conn: &Connection, run: &OptimizerRunRecord) -> Result<()> {
    let status = OptimizerRunStatus::parse(&run.status).ok_or_else(|| {
        anyhow!(
            "refusing optimizer run {} with non-lifecycle status {:?}; algorithm phases and work-item statuses must remain on their owned projections",
            run.id,
            run.status
        )
    })?;
    let mut canonical = run.clone();
    canonical.status = status.as_str().to_string();
    let payload = serde_json::to_string(&canonical)?;
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
            // The status column and payload are both canonicalized by this
            // single writer, so structured reads cannot disagree with SQL.
            status.as_str(),
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
    if let Some(contract) = run.summary.get("effectiveContract") {
        let contract: EffectiveContract =
            serde_json::from_value(contract.clone()).context("decode run effectiveContract")?;
        if contract.optimizer_run_id != run.id {
            bail!(
                "effective contract belongs to {}, refusing to persist it on {}",
                contract.optimizer_run_id,
                run.id
            );
        }
        super::effective_contract::upsert(conn, &contract)?;
    }
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
    frame_store: &ContentStore,
    mut run: OptimizerRunRecord,
    mut events: Vec<OptimizerEventEnvelope>,
    contract: SequenceContract,
) -> Result<(OptimizerRunRecord, Option<AppEvent>)> {
    // Normalize and validate the Optimizers-owned imported/relay slice before
    // even computing the insert plan. Workshop-local event families bypass
    // this gate and remain on the local contract path.
    super::event_contract::normalize_and_validate_imported_eval_events(&mut events)?;
    let durable = durable_event_ids(conn, &run.id, &events)?;
    let plan = plan_batch(&run.id, run.cursor_seq, &durable, &events, contract)
        .with_context(|| format!("validate optimizer event batch for {}", run.id))?;
    let mut appended = 0usize;
    let mut evidence_amendments = Vec::new();
    for (event, verdict) in events.iter_mut().zip(plan) {
        super::experiment_bind::fold_candidate(conn, event)?;
        if verdict == EventVerdict::ConfirmedReplay {
            continue;
        }
        super::frames::persist_event_frame(conn, frame_store, event)?;
        insert_event(conn, event)?;
        // Artifact declarations are indexed in the event transaction. A
        // conflicting identity therefore rolls back both the index and the
        // event that attempted to redefine it.
        super::artifacts::persist_event_artifacts(conn, std::slice::from_ref(event))?;
        // Fold the event's error into the run record as it commits, so a
        // terminal batch seals `error_json` and the manifest's error in the
        // same transaction — they can no longer diverge by one event.
        if let Some(error) = &event.error {
            run.error = Some(error.clone());
        }
        if event.event_type == "optimizer.evidence.amended" {
            evidence_amendments.push(event.clone());
        }
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
    // The append-only event log owns measured usage. Rebuild its accumulator
    // at the same cursor that will be terminal-sealed, while preserving only
    // non-measurement admission metadata (notably the paid-compute receipt).
    // Persist before terminal::seal so the run row and manifest freeze the
    // same numbers in one transaction.
    let mut canonical_usage = OptimizerUsageSummary {
        extra: run.usage.extra.clone(),
        ..OptimizerUsageSummary::default()
    };
    for event in &history {
        if event.event_type == "optimizer.usage.reconciled" {
            apply_authoritative_provider_usage(&mut canonical_usage, event)?;
        } else if let Some(delta) = &event.usage_delta {
            apply_reported_cost(&mut canonical_usage, delta);
            canonical_usage.calls += delta.get("calls").and_then(Value::as_u64).unwrap_or(0);
            canonical_usage.prompt_tokens += delta
                .get("prompt_tokens")
                .or_else(|| delta.get("promptTokens"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            canonical_usage.completion_tokens += delta
                .get("completion_tokens")
                .or_else(|| delta.get("completionTokens"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            canonical_usage.rollouts += delta.get("rollouts").and_then(Value::as_u64).unwrap_or(0);
            canonical_usage.wall_time_ms += delta
                .get("wall_time_ms")
                .or_else(|| delta.get("wallTimeMs"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
        }
    }
    run.usage = canonical_usage;
    update_paid_compute_violation(&mut run);
    upsert_run(conn, &run)?;
    let mut state = persist_kernel_projection(conn, &run, &history)?;
    for amendment in &evidence_amendments {
        persist_evidence_amendment(conn, &state, amendment)?;
    }
    if !evidence_amendments.is_empty() {
        state = super::kernel::persist::load_state(conn, &run.id)?
            .context("evidence amendment projection disappeared after persistence")?;
    }
    run.status = kernel_compatibility_status(&state).into();
    if state.lifecycle != super::kernel::RunLifecycle::Queued && run.started_at.is_none() {
        run.started_at = history.first().map(|event| event.occurred_at.clone());
    }
    run.finished_at = state
        .terminal
        .as_ref()
        .map(|terminal| terminal.sealed_at.clone());
    if state.terminal.as_ref().is_some_and(|terminal| {
        terminal.kind == super::kernel::TerminalKind::Failed
            && terminal.reason == Some(super::kernel::TerminalReason::EvidenceUnusable)
    }) && state.failure_ref.is_none()
    {
        let reason = state
            .terminal
            .as_ref()
            .and_then(|terminal| terminal.evidence.reason.clone())
            .unwrap_or_else(|| "required evaluation evidence is unavailable".into());
        let mut context =
            crate::platform::operations::OperationContext::bootstrap(crate::instance::boot_epoch());
        context.session_id = run.session_ref.clone();
        context.evaluation_id = Some(run.id.clone());
        let failure = crate::platform::failure::FailureRuntime::raise_in_tx(
            conn,
            crate::platform::failure::FailureKind::Evaluation(
                crate::platform::failure::EvaluationFailure::FailedEvidence {
                    run_id: run.id.clone(),
                    reason,
                },
            ),
            context,
            crate::platform::operations::OperationKind::EvaluationExecute,
            crate::platform::operations::OperationPhase::Settle,
            None,
            "optimizer_run_kernel",
        )?;
        state.failure_ref = Some(failure.failure_id.to_string());
        if let Some(terminal) = state.terminal.as_mut() {
            terminal.failure_ref = Some(failure.failure_id.to_string());
        }
        conn.execute(
            "UPDATE optimizer_runs SET terminal_failure_id=?1 WHERE id=?2",
            params![failure.failure_id.as_str(), run.id.as_str()],
        )?;
        super::kernel::persist::upsert_projection(conn, &state)?;
    }
    if let Some(terminal_state) = state.terminal.as_ref() {
        // The racing summary lane is rewritten from the terminal kernel state
        // in the same transaction that seals the manifest, so a stale worker
        // projection (`evalStatus: "running"`) can never survive beside a
        // sealed terminal.
        rewrite_terminal_summary_progress(&mut run, &state);
        upsert_run(conn, &run)?;
        settle_cancellation_receipts(conn, &run.id, terminal_state.final_sequence)?;
        let manifest = json!({
            "schemaVersion": "optimizer_terminal_manifest.v2",
            "optimizerRunId": state.run_id,
            "algorithmId": state.algorithm.wire_id(),
            "terminalCursor": terminal_state.final_sequence,
            "terminal": terminal_state,
            "work": state.work_summary(),
            "usage": state.usage(),
            "evidence": state.evidence_state(),
            "evidenceLedger": state
                .projection
                .eval_evidence_ledger()
                .map(|ledger| serde_json::to_value(ledger).unwrap_or(Value::Null))
                .unwrap_or(Value::Null),
            "projectionRevision": state.projection_revision,
            "error": run.error.clone().unwrap_or(Value::Null),
        });
        let sealed = terminal::seal(conn, &run.id, &manifest)?;
        if let Some(object) = run.summary.as_object_mut() {
            object.insert("terminalManifest".into(), sealed);
        }
    }
    upsert_run(conn, &run)?;
    super::experiment_bind::settle_run(conn, &run)?;
    let projected = project_from_kernel(&run, &state)
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

/// Rewrite `summary.evalStatus` and `summary.progress` from the terminal
/// kernel state. Called only inside the sealing transaction; `run.status` has
/// already been set from `kernel_compatibility_status`.
fn rewrite_terminal_summary_progress(
    run: &mut OptimizerRunRecord,
    state: &super::kernel::RunKernelState,
) {
    let mut summary = run.summary.as_object().cloned().unwrap_or_default();
    summary.insert("evalStatus".into(), json!(run.status));
    let work = state.work_summary();
    let rollouts = state
        .projection
        .work_items()
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let item_state = item
                .terminal
                .map(|terminal| terminal.as_str())
                .unwrap_or_else(|| item.lifecycle.as_str());
            (
                index.to_string(),
                json!({
                    "state": item_state,
                    "workItemId": item.work_item_id,
                    "externalRef": item.external_ref,
                }),
            )
        })
        .collect::<Map<String, Value>>();
    let authoritative = json!({
        "schemaVersion": super::kernel::RUN_VIEW_SCHEMA_VERSION,
        "asOfSequence": state.aggregate_sequence,
        "projectionRevision": state.projection_revision,
        "runState": state.lifecycle.as_str(),
        "rolloutStateCounts": {
            "queued": work.queued,
            "running": work.running,
            "completed": work.succeeded,
            "failed": work.failed,
            "cancelled": work.cancelled,
        },
        "inFlight": work.running,
        "evidence": state.evidence_state(),
        "rollouts": rollouts,
    });
    let mut progress = summary
        .get("progress")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    // Unmeasured counts stay whatever the worker last knew; measured ones are
    // frozen from the kernel. `None` never overwrites with zero.
    if let Some(planned) = work.planned {
        progress.insert("total".into(), json!(planned));
    }
    if let Some(succeeded) = work.succeeded {
        progress.insert("completed".into(), json!(succeeded));
    }
    if let Some(failed) = work.failed {
        progress.insert("failed".into(), json!(failed));
    }
    progress.insert("authoritative".into(), authoritative);
    summary.insert("progress".into(), Value::Object(progress));
    run.summary = Value::Object(summary);
}

/// Turn every open cancellation request for this run into a receipt by
/// stamping the terminal sequence it settled at. Runs inside the sealing
/// transaction; tolerant of the table not existing yet (pre-migration DBs in
/// unit fixtures).
fn settle_cancellation_receipts(
    conn: &Connection,
    run_id: &str,
    terminal_sequence: u64,
) -> Result<()> {
    let result = conn.execute(
        "UPDATE optimizer_cancellation_requests
         SET settled_sequence = ?2, observed_at = COALESCE(observed_at, ?3)
         WHERE run_id = ?1 AND settled_sequence IS NULL",
        params![
            run_id,
            i64::try_from(terminal_sequence).unwrap_or(i64::MAX),
            Utc::now().to_rfc3339(),
        ],
    );
    match result {
        Ok(_) => Ok(()),
        Err(rusqlite::Error::SqliteFailure(_, Some(message)))
            if message.contains("no such table") =>
        {
            Ok(())
        }
        Err(error) => Err(error.into()),
    }
}

fn persist_evidence_amendment(
    conn: &Connection,
    state: &super::kernel::RunKernelState,
    event: &OptimizerEventEnvelope,
) -> Result<()> {
    let terminal = state
        .terminal
        .as_ref()
        .context("evidence amendment requires a sealed terminal")?;
    let terminal_sequence = event
        .delta
        .get("terminalSequence")
        .and_then(Value::as_u64)
        .context("evidence amendment is missing terminalSequence")?;
    let amendment_id = event
        .event_id
        .clone()
        .context("evidence amendment is missing event identity")?;
    let refs = event
        .artifact_refs
        .iter()
        .map(|value| {
            let object = value
                .as_object()
                .context("evidence amendment ref must be an object")?;
            Ok(super::kernel::evidence::EvidenceRef {
                kind: object
                    .get("kind")
                    .and_then(Value::as_str)
                    .context("evidence amendment ref is missing kind")?
                    .to_string(),
                id: object
                    .get("id")
                    .or_else(|| object.get("refId"))
                    .and_then(Value::as_str)
                    .context("evidence amendment ref is missing id")?
                    .to_string(),
                digest: object
                    .get("digest")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let amendment = terminal
        .amend(super::kernel::EvidenceAmendment {
            amendment_id: amendment_id.clone(),
            optimizer_run_id: state.run_id.clone(),
            terminal_sequence,
            recorded_at: event.occurred_at.clone(),
            refs: refs.clone(),
        })
        .map_err(|error| anyhow!("{error}"))?;
    conn.execute(
        "INSERT INTO optimizer_evidence_amendments(
            amendment_id, optimizer_run_id, terminal_sequence, evidence_json, recorded_at
         ) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![
            amendment.amendment_id,
            amendment.optimizer_run_id,
            amendment.terminal_sequence as i64,
            serde_json::to_string(event)?,
            amendment.recorded_at,
        ],
    )?;
    for reference in refs {
        conn.execute(
            "INSERT OR IGNORE INTO optimizer_evidence_refs(
                optimizer_run_id, kind, ref_id, digest, recorded_at
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                state.run_id,
                reference.kind,
                reference.id,
                reference.digest,
                event.occurred_at,
            ],
        )?;
    }
    Ok(())
}

async fn reconcile_via_driver(
    service: &OptimizerService,
    run: OptimizerRunRecord,
) -> Result<OptimizerRunRecord> {
    let algorithm = match super::kernel::AlgorithmKind::parse_wire(&run.algorithm_id) {
        Ok(algorithm) => algorithm,
        Err(_) => {
            if run.source == "local"
                && matches!(run.status.as_str(), "completed" | "failed" | "cancelled")
            {
                return super::recipes::reconcile_persisted(service, &run.id).await;
            }
            return Ok(run);
        }
    };
    let placement = super::kernel::bridge::placement_from_run_source(algorithm, &run.source);
    let driver =
        super::kernel::resolve_driver(algorithm, placement).map_err(|error| anyhow!("{error}"))?;
    match (algorithm, driver) {
        (
            super::kernel::AlgorithmKind::Eval,
            super::kernel::DriverKind::DirectContainerEvaluation
            | super::kernel::DriverKind::LocalPythonProcess,
        ) => super::eval_recipes::reconcile_persisted(service, &run.id).await,
        (super::kernel::AlgorithmKind::Sft, super::kernel::DriverKind::LocalTrainingSidecar) => {
            super::mlx_sft::reconcile(service, &run.id).await
        }
        (
            super::kernel::AlgorithmKind::GoEx,
            super::kernel::DriverKind::HostedOptimizersService,
        ) => super::hosted_gelo::reconcile_persisted(service, &run.id).await,
        (_, super::kernel::DriverKind::LocalPythonProcess)
            if matches!(run.status.as_str(), "completed" | "failed" | "cancelled") =>
        {
            super::recipes::reconcile_persisted(service, &run.id).await
        }
        _ => Ok(run),
    }
}

fn persist_kernel_projection(
    conn: &Connection,
    run: &OptimizerRunRecord,
    events: &[OptimizerEventEnvelope],
) -> Result<super::kernel::RunKernelState> {
    let algorithm = super::kernel::AlgorithmKind::parse_wire(&run.algorithm_id)
        .map_err(|error| anyhow!("{error}"))?;
    let placement = super::kernel::bridge::placement_from_run_source(algorithm, &run.source);
    let spec_digest: String = conn
        .query_row(
            "SELECT spec_digest FROM optimizer_run_specs WHERE optimizer_run_id = ?1",
            [&run.id],
            |row| row.get(0),
        )
        .optional()?
        .filter(|value: &String| !value.trim().is_empty())
        .ok_or_else(|| anyhow!("optimizer run {} is missing its admitted spec", run.id))?;
    let state = super::kernel::bridge::reduce_envelopes(
        &run.id,
        algorithm,
        placement,
        &spec_digest,
        events,
    )
    .map_err(|error| anyhow::anyhow!("kernel reduce failed for {}: {error}", run.id))?;
    super::kernel::persist::upsert_projection(conn, &state)
        .with_context(|| format!("persist kernel projection for {}", run.id))?;
    Ok(state)
}

fn kernel_compatibility_status(state: &super::kernel::RunKernelState) -> &'static str {
    use super::kernel::{RunLifecycle, TerminalKind, TerminalReason};
    match state.lifecycle {
        RunLifecycle::Queued => "queued",
        RunLifecycle::Starting => "starting",
        RunLifecycle::Running => "running",
        RunLifecycle::Paused => "paused",
        RunLifecycle::Cancelling => "cancelling",
        RunLifecycle::Terminal => match state.terminal.as_ref() {
            Some(terminal)
                if terminal.kind == TerminalKind::Failed
                    && terminal.reason == Some(TerminalReason::EvidenceUnusable) =>
            {
                "failed_evidence"
            }
            Some(terminal) => terminal.kind.as_str(),
            None => "failed",
        },
    }
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

/// The one terminal predicate, delegating to [`OptimizerRunStatus`].
///
/// Four predicates used to spell this set four different ways; the enum is now
/// the only place the set is written down.
pub(super) fn is_terminal_status(status: &str) -> bool {
    OptimizerRunStatus::str_is_terminal(status)
}

/// Rewrite any non-terminal local optimizer run that has no live ownership
/// claim to `interrupted`, and seal a terminal manifest in the same
/// transaction. Called from `CoreRuntime::open`, not a spawned bootstrap task.
pub(crate) fn reconcile_stale_local_runs_in_tx(
    conn: &Connection,
    instance_id: &str,
    now: DateTime<Utc>,
) -> Result<Vec<OptimizerRunRecord>> {
    let mut stmt =
        conn.prepare("SELECT payload_json FROM optimizer_runs WHERE source = 'local'")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut payloads = Vec::new();
    for row in rows {
        payloads.push(row?);
    }
    drop(stmt);
    let mut recovered = Vec::new();
    for payload in payloads {
        let mut run: OptimizerRunRecord = serde_json::from_str(&payload)?;
        if is_terminal_status(&run.status) {
            continue;
        }
        if crate::recovery::ownership::optimizer_run_is_live(conn, &run.id, instance_id, now)? {
            continue;
        }
        run.status = "interrupted".into();
        run.finished_at = Some(now.to_rfc3339());
        run.error = Some(json!({
            "code": "unowned",
            "message": "no live optimizer run ownership claim at open",
        }));
        upsert_run(conn, &run)?;
        crate::recovery::ownership::release_optimizer_run(conn, &run.id)?;
        let events = load_events_upto(conn, &run.id, run.cursor_seq)?;
        let manifest = terminal::derive(&run, &events, "interrupted", None);
        let sealed = terminal::seal(conn, &run.id, &manifest)?;
        if let Some(object) = run.summary.as_object_mut() {
            object.insert("terminalManifest".into(), sealed);
        }
        upsert_run(conn, &run)?;
        super::experiment_bind::settle_run(conn, &run)?;
        recovered.push(run);
    }
    Ok(recovered)
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

fn event_carrier(event: &OptimizerEventEnvelope) -> Option<&Map<String, Value>> {
    event
        .delta
        .get("container_event")
        .or_else(|| event.delta.get("containerEvent"))
        .or_else(|| event.raw.get("container_event"))
        .or_else(|| event.raw.get("containerEvent"))
        .and_then(Value::as_object)
}

fn event_string(value: &Value, snake: &str, camel: &str) -> Option<String> {
    value
        .get(snake)
        .or_else(|| value.get(camel))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

/// Shredded query fields for the host event log. The serialized envelope
/// remains the replay authority; these columns are indexes/witnesses derived
/// at the same append boundary, never a second event representation.
fn shredded_event_fields(
    event: &OptimizerEventEnvelope,
) -> (
    Option<String>,
    String,
    Option<i64>,
    Option<String>,
    String,
    Option<String>,
) {
    let carrier = event_carrier(event);
    let carrier_payload = carrier
        .and_then(|value| value.get("payload"))
        .and_then(Value::as_object);
    let rollout_id = carrier
        .and_then(|value| value.get("rollout_id").or_else(|| value.get("rolloutId")))
        .and_then(Value::as_str)
        .or_else(|| {
            event
                .raw
                .get("rollout_id")
                .or_else(|| event.raw.get("rolloutId"))
                .and_then(Value::as_str)
        })
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);
    let kind = carrier
        .and_then(|value| value.get("kind"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&event.event_type)
        .to_string();
    let step = carrier_payload
        .and_then(|value| value.get("step"))
        .or_else(|| carrier.and_then(|value| value.get("step")))
        .or_else(|| event.raw.get("step"))
        .and_then(Value::as_i64);
    let span_id = carrier_payload
        .and_then(|value| value.get("span_id").or_else(|| value.get("spanId")))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .or_else(|| event_string(&event.raw, "span_id", "spanId"));
    let producer_occurred_at = carrier
        .and_then(|value| value.get("occurred_at").or_else(|| value.get("occurredAt")))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(&event.occurred_at)
        .to_string();
    let producer_digest = carrier
        .and_then(|value| value.get("digest"))
        .or_else(|| event.raw.get("digest"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string);
    (
        rollout_id,
        kind,
        step,
        span_id,
        producer_occurred_at,
        producer_digest,
    )
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
    let (rollout_id, kind, step, span_id, producer_occurred_at, producer_digest) =
        shredded_event_fields(event);
    let ingested_at = Utc::now().to_rfc3339();
    let event_id = event
        .event_id
        .clone()
        .unwrap_or_else(|| format!("{}:{}", event.optimizer_run_id, event.sequence_number));
    conn.execute(
        "INSERT INTO optimizer_events(
            event_id, optimizer_run_id, sequence_number, event_type,
            algorithm_id, occurred_at, payload_json, rollout_id, kind, step,
            span_id, producer_occurred_at, ingested_at, ingest_witness,
            producer_digest, payload_cas_digest
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,'host_clock',?14,NULL)",
        params![
            event_id,
            event.optimizer_run_id,
            event.sequence_number as i64,
            event.event_type,
            event.algorithm_id,
            event.occurred_at,
            payload,
            rollout_id,
            kind,
            step,
            span_id,
            producer_occurred_at,
            ingested_at,
            producer_digest,
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

/// `optimizer.usage.reconciled` is a snapshot from Workshop's durable
/// capability ledger, not another provider-call delta. Replacing only the
/// provider-measured fields prevents double-counting container policy spans;
/// rollout and wall-time accounting remain owned by the eval events.
fn apply_authoritative_provider_usage(
    usage: &mut OptimizerUsageSummary,
    event: &OptimizerEventEnvelope,
) -> Result<()> {
    let receipt = event
        .item
        .as_ref()
        .and_then(Value::as_object)
        .context("optimizer.usage.reconciled is missing its typed receipt item")?;
    let schema = receipt
        .get("schemaVersion")
        .and_then(Value::as_str)
        .context("optimizer.usage.reconciled is missing schemaVersion")?;
    if schema != "workshop.provider-usage-receipt.v1" {
        bail!("unsupported provider usage receipt schema `{schema}`");
    }
    let calls = receipt
        .get("calls")
        .and_then(Value::as_u64)
        .context("optimizer.usage.reconciled is missing calls")?;
    let prompt_tokens = receipt
        .get("promptTokens")
        .and_then(Value::as_u64)
        .context("optimizer.usage.reconciled is missing promptTokens")?;
    let completion_tokens = receipt
        .get("completionTokens")
        .and_then(Value::as_u64)
        .context("optimizer.usage.reconciled is missing completionTokens")?;
    let raw_cost = receipt
        .get("costUsd")
        .context("optimizer.usage.reconciled is missing costUsd")?;
    let cost_usd = if raw_cost.is_null() {
        None
    } else {
        Some(
            raw_cost
                .as_f64()
                .filter(|cost| cost.is_finite() && *cost >= 0.0)
                .context("optimizer.usage.reconciled costUsd must be null or non-negative")?,
        )
    };
    if receipt
        .get("receiptDigest")
        .and_then(Value::as_str)
        .filter(|digest| digest.starts_with("sha256:") && digest.len() == 71)
        .is_none()
    {
        bail!("optimizer.usage.reconciled is missing a canonical receiptDigest");
    }

    usage.calls = calls;
    usage.prompt_tokens = prompt_tokens;
    usage.completion_tokens = completion_tokens;
    usage.cost_usd = cost_usd;
    usage.extra.insert(
        "costTelemetryComplete".into(),
        Value::Bool(cost_usd.is_some()),
    );
    usage.extra.insert(
        "providerUsageReceipt".into(),
        Value::Object(receipt.clone()),
    );
    Ok(())
}

