//! Optional post-rollout annotation stage for evaluation runs ("lane B").
//!
//! Off unless the workspace recipe declares an `[annotation]` table. The stage
//! runs strictly after the run's terminal manifest is sealed, so nothing it
//! does can move objective reward or the run outcome. It submits one
//! annotation campaign — the run's fully sealed Trace V5 refs × the declared
//! annotators × repeats — to the owning container's annotation router
//! (`POST /annotation/campaigns`) and records the container's job ids on the
//! run as an append-only evidence amendment (`kind = "annotation_job"`,
//! `digest = trace digest`), where the UI and skills already read evidence.
//!
//! Money: deterministic annotators are free and need nothing. Paid annotators
//! are estimated by the container, approved through the installed
//! [`PaidApprover`] (the Desktop approval card; `ApprovalKind::PaidCompute`)
//! *before* any reservation exists, then carried as one single-use signed
//! reservation per paid job minted by
//! [`crate::session::annotation_reservation`]. If the paid lane cannot be
//! prepared (no session, no cap, no approver, rejected), the campaign is still
//! submitted without reservations and the container records those jobs as
//! refused — reported, never fatal.
//!
//! Every failure ends in an amendment on the run; none propagates to the eval
//! worker. The eval completed; the annotation lane may not have.
//!
//! ```toml
//! [annotation]
//! annotators = ["craftax.deterministic", { id = "craftax.belief", repeats = 2 }]
//! repeats = 1              # default for annotators that do not say
//! max_cost_usd = 0.80      # hard ceiling on the paid lane; omit to allow free annotators only
//! label = "post_rollout"
//! [annotation.throughput]  # advisory per-class caps forwarded to the container scheduler
//! deterministic = 4
//! paid = 2
//! ```

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, RwLock};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::events::OptimizerEventDraft;
use super::OptimizerService;
use crate::error::StructuredFailure;
use crate::session::annotation_reservation::{self as reservations, ReservationBinding};
use crate::session::paid_compute_budget::{self, micros_from_reported_cost};

const EVAL_ALGORITHM_ID: &str = "eval";
pub(crate) const EVENT_SOURCE: &str = "annotation_stage";
pub(crate) const JOB_REF_KIND: &str = "annotation_job";
pub(crate) const CAMPAIGN_REF_KIND: &str = "annotation_campaign";
const RESERVATION_TTL_SECONDS: i64 = 900;
const MAX_REPEATS: u32 = 5;
const MAX_ANNOTATORS: usize = 16;
const THROUGHPUT_CLASSES: &[&str] = &["deterministic", "paid"];
const SPEC_KEYS: &[&str] = &[
    "enabled",
    "annotators",
    "repeats",
    "max_cost_usd",
    "label",
    "throughput",
];
const ANNOTATOR_KEYS: &[&str] = &[
    "id",
    "annotator_id",
    "repeats",
    "model",
    "rubric_id",
    "reasoning_effort",
    "runner_kind",
];

fn failure(
    code: &'static str,
    message: impl Into<String>,
    remediation: impl Into<String>,
) -> anyhow::Error {
    anyhow::Error::new(StructuredFailure::new(code, message, remediation))
}

// ---------------------------------------------------------------------------
// Spec
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct AnnotatorSpec {
    pub annotator_id: String,
    pub repeats: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rubric_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_kind: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct AnnotationStageSpec {
    pub annotators: Vec<AnnotatorSpec>,
    /// Advisory per-class concurrency caps (`deterministic`, `paid`), forwarded
    /// to the container scheduler verbatim.
    pub throughput: BTreeMap<String, u32>,
    /// Hard ceiling on the paid lane. `None` means paid annotators are refused.
    pub max_cost_usd: Option<f64>,
    pub label: String,
}

impl AnnotationStageSpec {
    /// Parse the `[annotation]` table of a workspace recipe. Every bound is
    /// checked here, where it is read, so a bad recipe is refused at load time.
    /// `enabled = false` keeps the block but turns the stage off.
    pub(crate) fn parse(recipe_id: &str, table: &toml::value::Table) -> Result<Option<Self>> {
        for key in table.keys() {
            if !SPEC_KEYS.contains(&key.as_str()) {
                bail!("recipe `{recipe_id}` annotation.{key} is not an admitted option");
            }
        }
        match table.get("enabled") {
            None | Some(toml::Value::Boolean(true)) => {}
            Some(toml::Value::Boolean(false)) => return Ok(None),
            Some(_) => bail!("recipe `{recipe_id}` annotation.enabled must be a boolean"),
        }
        let default_repeats = match table.get("repeats") {
            None => 1,
            Some(value) => repeats_of(recipe_id, "annotation.repeats", value)?,
        };
        let declared = table
            .get("annotators")
            .and_then(toml::Value::as_array)
            .filter(|items| !items.is_empty())
            .ok_or_else(|| {
                anyhow!("recipe `{recipe_id}` annotation.annotators must list at least one annotator")
            })?;
        if declared.len() > MAX_ANNOTATORS {
            bail!("recipe `{recipe_id}` annotation.annotators lists more than {MAX_ANNOTATORS} annotators");
        }
        let mut annotators = Vec::with_capacity(declared.len());
        let mut seen = BTreeSet::new();
        for item in declared {
            let spec = match item {
                toml::Value::String(id) => AnnotatorSpec {
                    annotator_id: id.trim().to_string(),
                    repeats: default_repeats,
                    model: None,
                    rubric_id: None,
                    reasoning_effort: None,
                    runner_kind: None,
                },
                toml::Value::Table(entry) => {
                    for key in entry.keys() {
                        if !ANNOTATOR_KEYS.contains(&key.as_str()) {
                            bail!("recipe `{recipe_id}` annotation.annotators.{key} is not an admitted option");
                        }
                    }
                    let id = entry
                        .get("id")
                        .or_else(|| entry.get("annotator_id"))
                        .and_then(toml::Value::as_str)
                        .map(str::trim)
                        .filter(|id| !id.is_empty())
                        .ok_or_else(|| {
                            anyhow!("recipe `{recipe_id}` annotation.annotators entries need an id")
                        })?;
                    let repeats = match entry.get("repeats") {
                        None => default_repeats,
                        Some(value) => repeats_of(recipe_id, "annotation.annotators.repeats", value)?,
                    };
                    AnnotatorSpec {
                        annotator_id: id.to_string(),
                        repeats,
                        model: optional_string(recipe_id, entry, "model")?,
                        rubric_id: optional_string(recipe_id, entry, "rubric_id")?,
                        reasoning_effort: optional_string(recipe_id, entry, "reasoning_effort")?,
                        runner_kind: optional_string(recipe_id, entry, "runner_kind")?,
                    }
                }
                _ => bail!("recipe `{recipe_id}` annotation.annotators entries must be ids or tables"),
            };
            if spec.annotator_id.is_empty() {
                bail!("recipe `{recipe_id}` annotation.annotators contains an empty id");
            }
            if !seen.insert(spec.annotator_id.clone()) {
                bail!(
                    "recipe `{recipe_id}` annotation.annotators lists `{}` twice",
                    spec.annotator_id
                );
            }
            annotators.push(spec);
        }
        let mut throughput = BTreeMap::new();
        if let Some(value) = table.get("throughput") {
            let caps = value.as_table().ok_or_else(|| {
                anyhow!("recipe `{recipe_id}` annotation.throughput must be a table")
            })?;
            for (class, cap) in caps {
                if !THROUGHPUT_CLASSES.contains(&class.as_str()) {
                    bail!(
                        "recipe `{recipe_id}` annotation.throughput.{class} is not an annotator class (expected one of {})",
                        THROUGHPUT_CLASSES.join(", ")
                    );
                }
                let cap = cap
                    .as_integer()
                    .filter(|cap| (1..=64).contains(cap))
                    .ok_or_else(|| {
                        anyhow!("recipe `{recipe_id}` annotation.throughput.{class} must be 1..=64")
                    })?;
                throughput.insert(class.clone(), cap as u32);
            }
        }
        let max_cost_usd = match table.get("max_cost_usd") {
            None => None,
            Some(value) => {
                let cost = value
                    .as_float()
                    .or_else(|| value.as_integer().map(|v| v as f64))
                    .filter(|cost| cost.is_finite() && *cost > 0.0)
                    .ok_or_else(|| {
                        anyhow!("recipe `{recipe_id}` annotation.max_cost_usd must be a positive finite number")
                    })?;
                Some(cost)
            }
        };
        let label = match table.get("label") {
            None => "post_rollout".to_string(),
            Some(value) => value
                .as_str()
                .map(str::trim)
                .filter(|label| !label.is_empty())
                .ok_or_else(|| anyhow!("recipe `{recipe_id}` annotation.label must be a non-empty string"))?
                .to_string(),
        };
        Ok(Some(Self {
            annotators,
            throughput,
            max_cost_usd,
            label,
        }))
    }

    /// The container's `AnnotatorPlan` shape.
    pub(crate) fn campaign_annotators(&self) -> Vec<Value> {
        self.annotators
            .iter()
            .map(|annotator| {
                let mut plan = json!({
                    "annotator_id": annotator.annotator_id,
                    "repeats": annotator.repeats,
                });
                if let Some(model) = &annotator.model {
                    plan["model"] = json!(model);
                }
                if let Some(rubric_id) = &annotator.rubric_id {
                    plan["rubric_id"] = json!(rubric_id);
                }
                if let Some(effort) = &annotator.reasoning_effort {
                    plan["reasoning_effort"] = json!(effort);
                }
                if let Some(runner_kind) = &annotator.runner_kind {
                    plan["runner_kind"] = json!(runner_kind);
                }
                plan
            })
            .collect()
    }

    pub(crate) fn max_cost_usd_micros(&self) -> Option<u64> {
        self.max_cost_usd.and_then(micros_from_reported_cost)
    }
}

fn repeats_of(recipe_id: &str, field: &str, value: &toml::Value) -> Result<u32> {
    value
        .as_integer()
        .filter(|repeats| (1..=i64::from(MAX_REPEATS)).contains(repeats))
        .map(|repeats| repeats as u32)
        .ok_or_else(|| anyhow!("recipe `{recipe_id}` {field} must be 1..={MAX_REPEATS}"))
}

fn optional_string(
    recipe_id: &str,
    entry: &toml::value::Table,
    field: &str,
) -> Result<Option<String>> {
    match entry.get(field) {
        None => Ok(None),
        Some(value) => value
            .as_str()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| Some(value.to_string()))
            .ok_or_else(|| {
                anyhow!("recipe `{recipe_id}` annotation.annotators.{field} must be a non-empty string")
            }),
    }
}

// ---------------------------------------------------------------------------
// Inputs: the run's sealed traces
// ---------------------------------------------------------------------------

/// `{kind: "trace_v5", id, digest}` refs for every fully sealed trace in the
/// worker's terminal records — the shape the container's `plan_from_refs`
/// consumes. Partial traces are not annotatable evidence and are skipped.
pub(crate) fn sealed_trace_refs(records: &[Value]) -> Vec<Value> {
    let mut refs = Vec::new();
    let mut seen = BTreeSet::new();
    for record in records {
        if record.get("evidenceState").and_then(Value::as_str) == Some("sealed_partial") {
            continue;
        }
        let Some(traces) = record
            .pointer("/sealedTrace/traces")
            .and_then(Value::as_array)
        else {
            continue;
        };
        for trace in traces {
            // The imported Workshop trace id is host-local. The annotation
            // router still owns the producer bundle, so address that bundle
            // by its original trace id while preserving the verified digest.
            let (Some(id), Some(digest)) = (
                trace
                    .get("producerTraceId")
                    .or_else(|| trace.get("traceId"))
                    .and_then(Value::as_str),
                trace
                    .get("digest")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|digest| !digest.is_empty()),
            ) else {
                continue;
            };
            if seen.insert(digest.to_string()) {
                refs.push(json!({"kind": "trace_v5", "id": id, "digest": digest}));
            }
        }
    }
    refs
}

// ---------------------------------------------------------------------------
// Paid approval hook
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub(crate) struct PaidApprovalRequest {
    pub run_id: String,
    pub session_id: String,
    pub container_id: String,
    pub cap_usd_micros: u64,
    pub paid_jobs: usize,
    pub estimate: Value,
    /// Binds the approval to exactly this plan (trace digests × annotators × repeats).
    pub binding_digest: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PaidApprovalGrant {
    pub approval_id: String,
    pub cap_usd_micros: u64,
}

pub(crate) type PaidApprovalFuture = Pin<Box<dyn Future<Output = Result<PaidApprovalGrant>> + Send>>;
pub(crate) type PaidApprover = Arc<dyn Fn(PaidApprovalRequest) -> PaidApprovalFuture + Send + Sync>;

static PAID_APPROVER: RwLock<Option<PaidApprover>> = RwLock::new(None);

/// Box a closure as a [`PaidApprover`], giving the closure its expected
/// signature so `Box::pin(async …)` coerces at the return site.
pub(crate) fn paid_approver<F>(approve: F) -> PaidApprover
where
    F: Fn(PaidApprovalRequest) -> PaidApprovalFuture + Send + Sync + 'static,
{
    Arc::new(approve)
}

/// Register the process-wide approver the eval worker consults. Installed
/// once by the composition root; tests hand [`execute`] their own instead.
pub(crate) fn install_paid_approver(approver: PaidApprover) {
    if let Ok(mut slot) = PAID_APPROVER.write() {
        *slot = Some(approver);
    }
}

pub(crate) fn installed_paid_approver() -> Option<PaidApprover> {
    PAID_APPROVER.read().ok().and_then(|slot| slot.clone())
}

/// The Desktop approver: routes through the session's `ApprovalBroker` as an
/// `ApprovalKind::PaidCompute` host outcome, exactly like a paid annotation
/// started from the annotations IPC.
#[allow(dead_code)] // wired by the composition root (see the lib.rs hook patch)
pub(crate) fn install_desktop_paid_approver(app: tauri::AppHandle) {
    use crate::session::approval::{ApprovalBroker, ApprovalDecision, ApprovalKind, PaidComputeCap};
    use tauri::Manager;
    let approver = paid_approver(move |request: PaidApprovalRequest| {
        let app = app.clone();
        Box::pin(async move {
            let broker = app
                .try_state::<Arc<ApprovalBroker>>()
                .map(|state| state.inner().clone())
                .ok_or_else(|| {
                    failure(
                        "approval_broker_unavailable",
                        "approval broker unavailable",
                        "open a Workshop session",
                    )
                })?;
            let requested = PaidComputeCap {
                max_cost_usd_micros: Some(request.cap_usd_micros),
                max_rollouts: None,
            };
            if !requested.is_bounded() {
                return Err(failure(
                    "annotation_cap_unbounded",
                    "annotation estimate declares no enforceable cost cap",
                    "declare annotation.max_cost_usd on the recipe",
                ));
            }
            let (approval_id, decision) = broker
                .authorize_host_outcome(
                    &app,
                    Some(request.session_id.as_str()),
                    ApprovalKind::PaidCompute {
                        operation: "annotation.post_rollout_campaign".to_string(),
                        parameters: json!({
                            "runId": request.run_id,
                            "containerId": request.container_id,
                            "jobs": request.paid_jobs,
                            "estimate": request.estimate,
                        }),
                        estimated_cost_usd_micros: Some(request.cap_usd_micros),
                        requested_cap: requested.clone(),
                        recipe_id: None,
                        dataset: None,
                        proposer_model: None,
                        evaluator_model: None,
                        timeout_seconds: None,
                        credential_names: vec![],
                        requesting_agent: "annotation_stage".to_string(),
                        preparation_digest: Some(request.binding_digest.clone()),
                    },
                )
                .await
                .map_err(|error| {
                    failure(
                        "approval_rejected",
                        error.to_string(),
                        "ask the user for a bounded approval and rerun the annotation campaign",
                    )
                })?;
            let cap = request.cap_usd_micros;
            let granted = match decision {
                ApprovalDecision::Reject => {
                    return Err(failure(
                        "approval_rejected",
                        "post-rollout annotation was rejected",
                        "do not retry without a new approval",
                    ))
                }
                ApprovalDecision::ApproveWithCap { cap: granted } => {
                    granted.max_cost_usd_micros.unwrap_or(cap).min(cap)
                }
                ApprovalDecision::Approve { .. } => cap,
                ApprovalDecision::Credential { .. } => {
                    return Err(failure(
                        "approval_rejected",
                        "unexpected credential decision for paid compute",
                        "retry",
                    ))
                }
            };
            if granted == 0 {
                return Err(failure(
                    "approval_rejected",
                    "granted cap is zero",
                    "ask for a bounded approval",
                ));
            }
            Ok(PaidApprovalGrant {
                approval_id,
                cap_usd_micros: granted,
            })
        })
    });
    install_paid_approver(approver);
}

// ---------------------------------------------------------------------------
// Container client
// ---------------------------------------------------------------------------

struct CampaignClient {
    base: String,
    client: reqwest::Client,
}

impl CampaignClient {
    fn new(base_url: &str) -> Result<Self> {
        let base = crate::visuals_ipc::validated_loopback_rollout_base(base_url)?;
        let client = crate::http::http_client_builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(crate::limits::ANNOTATION_IPC_TIMEOUT)
            .build()?;
        Ok(Self { base, client })
    }

    async fn post_campaign(&self, plan: &Value) -> Result<Value> {
        let url = format!("{}/annotation/campaigns", self.base);
        let response = self
            .client
            .post(&url)
            .json(plan)
            .send()
            .await
            .map_err(|error| {
                failure(
                    "annotation_container_unreachable",
                    format!("{url}: {error}"),
                    "check the container's /health and that it mounts the annotation router",
                )
            })?;
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        let payload: Value = serde_json::from_str(&text).unwrap_or_else(|_| json!({"raw": text}));
        if status.is_success() {
            return Ok(payload);
        }
        let detail = payload.get("detail").cloned().unwrap_or(payload.clone());
        let message = detail
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or(&text)
            .to_string();
        let mut structured = StructuredFailure::new(
            "annotation_container_error",
            format!("{status}: {message}"),
            "inspect the container error detail",
        );
        structured.details = detail;
        Err(anyhow::Error::new(structured))
    }

    /// The container may expose a promoted domain digest for the producer
    /// trace. Workshop's imported trace keeps the sealed source digest, so use
    /// the container-owned ref when it identifies the same producer bundle.
    async fn trace_refs(&self) -> Option<Vec<Value>> {
        let response = self
            .client
            .get(format!("{}/annotation/traces", self.base))
            .send()
            .await
            .ok()?;
        if !response.status().is_success() {
            return None;
        }
        response
            .json::<Value>()
            .await
            .ok()?
            .get("traces")?
            .as_array()
            .cloned()
    }
}

// ---------------------------------------------------------------------------
// Stage execution
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct RecordedJob {
    pub job_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotator_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat_index: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reservation_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StageReport {
    /// `submitted` | `skipped` | `failed`
    pub status: String,
    pub container_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub campaign_id: Option<String>,
    pub traces: usize,
    pub jobs: Vec<RecordedJob>,
    pub cache_hits: u64,
    pub enqueued: u64,
    pub refused: Vec<Value>,
    pub paid_jobs: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cost_usd: Option<f64>,
    pub notes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl StageReport {
    fn new(status: &str, container_id: &str) -> Self {
        Self {
            status: status.to_string(),
            container_id: container_id.to_string(),
            campaign_id: None,
            traces: 0,
            jobs: Vec::new(),
            cache_hits: 0,
            enqueued: 0,
            refused: Vec::new(),
            paid_jobs: 0,
            approval_id: None,
            max_cost_usd: None,
            notes: Vec::new(),
            error: None,
        }
    }

    fn failed(container_id: &str, error: &anyhow::Error) -> Self {
        let mut report = Self::new("failed", container_id);
        report.error = Some(format!("{error:#}"));
        report
    }
}

/// The worker's hook: run the stage after the seal and record whatever
/// happened. Never returns an error — the eval is already terminal and its
/// outcome must not depend on this lane.
pub(crate) async fn run_after_terminal(
    service: &OptimizerService,
    run_id: &str,
    spec: &AnnotationStageSpec,
    container_id: &str,
    container_base_url: &str,
    records: &[Value],
) -> StageReport {
    let approver = installed_paid_approver();
    let report = match execute(
        service,
        run_id,
        spec,
        container_id,
        container_base_url,
        records,
        approver,
    )
    .await
    {
        Ok(report) => report,
        Err(error) => StageReport::failed(container_id, &error),
    };
    // Recording can only fail if the run is not sealed (a worker bug) or the
    // database is gone; either way there is nowhere left to report to.
    let _ = record(service, run_id, spec, &report).await;
    report
}

/// Estimate, (optionally) approve and reserve, submit. Returns the report of
/// what the container accepted; `Err` only when nothing was submitted.
pub(crate) async fn execute(
    service: &OptimizerService,
    run_id: &str,
    spec: &AnnotationStageSpec,
    container_id: &str,
    container_base_url: &str,
    records: &[Value],
    approver: Option<PaidApprover>,
) -> Result<StageReport> {
    let mut traces = sealed_trace_refs(records);
    if traces.is_empty() {
        let mut report = StageReport::new("skipped", container_id);
        report
            .notes
            .push("no fully sealed Trace V5 bundles to annotate".into());
        return Ok(report);
    }
    let client = CampaignClient::new(container_base_url)?;
    if let Some(owner_refs) = client.trace_refs().await {
        for trace in &mut traces {
            let Some(id) = trace.get("id").and_then(Value::as_str) else {
                continue;
            };
            let Some(owner) = owner_refs
                .iter()
                .find(|candidate| candidate.get("id").and_then(Value::as_str) == Some(id))
            else {
                continue;
            };
            if let Some(digest) = owner
                .get("digest")
                .and_then(Value::as_str)
                .filter(|digest| !digest.trim().is_empty())
            {
                trace["digest"] = json!(digest);
            }
        }
    }
    let session_id = service.get(run_id.to_string()).await?.session_ref;
    let mut plan = json!({
        "traces": traces,
        "annotators": spec.campaign_annotators(),
        "label": spec.label,
        "session_id": session_id,
        "throughput": spec.throughput,
        "estimate_only": true,
    });
    let estimate = client.post_campaign(&plan).await?;
    let estimate = estimate.get("estimate").cloned().unwrap_or(estimate);
    let paid_jobs = estimate
        .get("paid_jobs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut report = StageReport::new("submitted", container_id);
    report.traces = traces.len();
    report.paid_jobs = paid_jobs.len();
    report.max_cost_usd = spec.max_cost_usd;
    if let Some(notes) = estimate.get("notes").and_then(Value::as_array) {
        report
            .notes
            .extend(notes.iter().filter_map(Value::as_str).map(str::to_owned));
    }
    plan["estimate_only"] = json!(false);

    // Lane B money: one approval for the whole campaign, one signed
    // reservation per paid job. A paid lane that cannot be prepared is a note,
    // not a failure — the free annotators still run and the container marks
    // the paid jobs `reservation_required`.
    let mut paid_lane: Option<PaidLane> = None;
    if !paid_jobs.is_empty() {
        match prepare_paid_lane(
            service,
            run_id,
            spec,
            container_id,
            session_id.as_deref(),
            &estimate,
            &paid_jobs,
            approver,
        )
        .await
        {
            Ok(lane) => {
                report.approval_id = Some(lane.approval_id.clone());
                plan["reservations"] = Value::Object(lane.tokens.clone());
                paid_lane = Some(lane);
            }
            Err(error) => report.notes.push(format!("paid lane skipped: {error:#}")),
        }
    }

    let payload = match client.post_campaign(&plan).await {
        Ok(payload) => payload,
        Err(error) => {
            if let Some(lane) = paid_lane.take() {
                release_paid_lane(service, &lane).await;
            }
            return Err(error);
        }
    };

    // Match by the stable reservation key, never response order: cached or
    // refused jobs may be omitted or reordered by the container.
    let bindings: HashMap<String, (String, Option<String>)> = payload
        .get("job_bindings")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some((
                        item.get("key")?.as_str()?.to_string(),
                        (
                            item.get("job_id")?.as_str()?.to_string(),
                            item.get("reservation_id")
                                .and_then(Value::as_str)
                                .map(str::to_owned),
                        ),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    if let Some(lane) = paid_lane.take() {
        let forwarded: Vec<(String, Option<String>)> = lane
            .issued
            .iter()
            .map(|(key, reservation_id)| {
                (
                    reservation_id.clone(),
                    bindings.get(key).map(|(job_id, _)| job_id.clone()),
                )
            })
            .collect();
        service
            .database()
            .run_transaction(move |conn| {
                for (reservation_id, job_id) in &forwarded {
                    match job_id {
                        Some(job_id) => reservations::mark_forwarded(conn, reservation_id, job_id)?,
                        None => reservations::release(conn, reservation_id)?,
                    }
                }
                Ok(())
            })
            .await?;
    }

    report.campaign_id = payload
        .get("campaign_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    report.cache_hits = payload.get("cache_hits").and_then(Value::as_u64).unwrap_or(0);
    report.enqueued = payload.get("enqueued").and_then(Value::as_u64).unwrap_or(0);
    report.refused = payload
        .get("refused")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let job_ids: Vec<String> = payload
        .get("jobs")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    report.jobs = bind_jobs(&traces, spec, &report.refused, &job_ids, &bindings);
    Ok(report)
}

struct PaidLane {
    approval_id: String,
    /// `(reservation key, reservation id)` in plan order.
    issued: Vec<(String, String)>,
    tokens: Map<String, Value>,
}

#[allow(clippy::too_many_arguments)]
async fn prepare_paid_lane(
    service: &OptimizerService,
    run_id: &str,
    spec: &AnnotationStageSpec,
    container_id: &str,
    session_id: Option<&str>,
    estimate: &Value,
    paid_jobs: &[Value],
    approver: Option<PaidApprover>,
) -> Result<PaidLane> {
    let session = session_id.filter(|s| !s.trim().is_empty()).ok_or_else(|| {
        failure(
            "annotation_session_required",
            "paid annotators need the run to belong to a session",
            "start the eval from an agent session so approval can be routed",
        )
    })?;
    let ceiling = spec.max_cost_usd_micros().ok_or_else(|| {
        failure(
            "annotation_cap_unbounded",
            "the recipe declares paid annotators without annotation.max_cost_usd",
            "declare annotation.max_cost_usd on the recipe",
        )
    })?;
    let approver = approver.ok_or_else(|| {
        failure(
            "approval_broker_unavailable",
            "no paid-annotation approver is installed in this process",
            "run from the Desktop, which installs the approval card",
        )
    })?;
    let total = estimate
        .get("max_cost_usd")
        .and_then(Value::as_f64)
        .and_then(micros_from_reported_cost)
        .ok_or_else(|| {
            failure(
                "annotation_cap_unbounded",
                "campaign estimate has no max_cost_usd",
                "declare limits.max_cost_usd on paid annotators",
            )
        })?;
    let cid = container_id.to_string();
    let has_secret = service
        .database()
        .run_read(move |conn| reservations::load_broker_secret(conn, &cid))
        .await?
        .is_some();
    if !has_secret {
        return Err(failure(
            "reservation_broker_unavailable",
            format!("container `{container_id}` was not launched by Workshop; no reservation secret exists"),
            "launch the container through workshop.containers.toml so Workshop can inject the annotation broker secret",
        ));
    }
    let mut specs: Vec<(String, ReservationBinding, u64)> = Vec::with_capacity(paid_jobs.len());
    let mut declared_total = 0_u64;
    for job in paid_jobs {
        let cap = job
            .get("max_cost_usd")
            .and_then(Value::as_f64)
            .and_then(micros_from_reported_cost)
            .ok_or_else(|| {
                failure(
                    "annotation_cap_unbounded",
                    "campaign contains a paid job without max_cost_usd",
                    "declare limits.max_cost_usd on every paid annotator",
                )
            })?;
        let field = |name: &str, what: &str| -> Result<String> {
            job.get(name)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
                .ok_or_else(|| {
                    failure(
                        "annotation_binding_incomplete",
                        format!("campaign paid job has no {what}"),
                        "re-estimate against sealed Trace V5 inputs and registered annotators",
                    )
                })
        };
        let binding = ReservationBinding {
            trace_digest: field("trace_digest", "trace digest")?,
            annotator_id: field("annotator_id", "annotator id")?,
            model: field("model", "resolved model")?,
            session_id: session.to_string(),
        };
        declared_total = declared_total.checked_add(cap).ok_or_else(|| {
            failure(
                "annotation_cap_unbounded",
                "campaign cost cap overflowed",
                "reduce the campaign size",
            )
        })?;
        let key = format!(
            "{}|{}|{}",
            binding.trace_digest,
            binding.annotator_id,
            job.get("repeat_index").and_then(Value::as_u64).unwrap_or(0)
        );
        specs.push((key, binding, cap));
    }
    if declared_total != total {
        return Err(failure(
            "annotation_estimate_inconsistent",
            format!("campaign total {total} does not equal its paid-job caps {declared_total}"),
            "refresh the container and estimate the campaign again",
        ));
    }
    if total > ceiling {
        return Err(failure(
            "annotation_cost_ceiling_exceeded",
            format!(
                "campaign estimate {:.4} USD exceeds annotation.max_cost_usd {:.4}",
                total as f64 / 1_000_000.0,
                ceiling as f64 / 1_000_000.0
            ),
            "raise annotation.max_cost_usd, drop annotators, or annotate fewer rollouts",
        ));
    }
    let binding_digest = super::admission::digest_of(&json!({
        "runId": run_id,
        "containerId": container_id,
        "keys": specs.iter().map(|(key, _, _)| key.as_str()).collect::<Vec<_>>(),
        "capUsdMicros": total,
    }))
    .map_err(|error| anyhow!("digest annotation plan: {error}"))?
    .to_string();
    let grant = approver(PaidApprovalRequest {
        run_id: run_id.to_string(),
        session_id: session.to_string(),
        container_id: container_id.to_string(),
        cap_usd_micros: total,
        paid_jobs: paid_jobs.len(),
        estimate: estimate.clone(),
        binding_digest,
    })
    .await?;
    if grant.cap_usd_micros < declared_total {
        release_budget(service, &grant.approval_id).await;
        return Err(failure(
            "annotation_campaign_cap_reduced",
            "the approved campaign cap is below the sum of its job caps",
            "approve the full bounded campaign or submit a smaller campaign",
        ));
    }
    // All reservations are minted in one transaction: either every paid job
    // carries a token or none does and the approval's budget hold is released.
    let container = container_id.to_string();
    let session_owned = session.to_string();
    let approval_id = grant.approval_id.clone();
    let minted = service
        .database()
        .run_transaction(move |conn| {
            let secret = reservations::load_broker_secret(conn, &container)?
                .context("annotation broker secret vanished")?;
            let mut issued = Vec::with_capacity(specs.len());
            let mut tokens = Map::new();
            for (key, binding, cap) in specs {
                let reservation = reservations::issue(
                    conn,
                    &secret,
                    &container,
                    &session_owned,
                    &approval_id,
                    &binding,
                    cap,
                    "workshop",
                    RESERVATION_TTL_SECONDS,
                )?;
                tokens.insert(key.clone(), json!(reservation.token));
                issued.push((key, reservation.reservation_id));
            }
            Ok((issued, tokens))
        })
        .await;
    match minted {
        Ok((issued, tokens)) => Ok(PaidLane {
            approval_id: grant.approval_id,
            issued,
            tokens,
        }),
        Err(error) => {
            release_budget(service, &grant.approval_id).await;
            Err(error)
        }
    }
}

async fn release_budget(service: &OptimizerService, approval_id: &str) {
    let approval_id = approval_id.to_string();
    let _ = service
        .database()
        .run_transaction(move |conn| paid_compute_budget::release_reservation(conn, &approval_id))
        .await;
}

/// The container never saw the campaign: give every reservation back.
async fn release_paid_lane(service: &OptimizerService, lane: &PaidLane) {
    let ids: Vec<String> = lane.issued.iter().map(|(_, id)| id.clone()).collect();
    let _ = service
        .database()
        .run_transaction(move |conn| {
            for id in &ids {
                reservations::release(conn, id)?;
            }
            Ok(())
        })
        .await;
}

/// Map returned job ids back to traces. The container submits in plan order
/// (trace → annotator → repeat) and reports what it refused by key, so the
/// accepted sequence is reconstructible; when the counts disagree only the
/// reservation-bound jobs keep a trace binding.
fn bind_jobs(
    traces: &[Value],
    spec: &AnnotationStageSpec,
    refused: &[Value],
    job_ids: &[String],
    bindings: &HashMap<String, (String, Option<String>)>,
) -> Vec<RecordedJob> {
    let refused_keys: BTreeSet<String> = refused
        .iter()
        .filter_map(|item| {
            Some(format!(
                "{}|{}|{}",
                item.get("trace_digest")?.as_str()?,
                item.get("annotator_id")?.as_str()?,
                item.get("repeat_index").and_then(Value::as_u64).unwrap_or(0)
            ))
        })
        .collect();
    let mut expected = Vec::new();
    for trace in traces {
        let (Some(trace_id), Some(digest)) = (
            trace.get("id").and_then(Value::as_str),
            trace.get("digest").and_then(Value::as_str),
        ) else {
            continue;
        };
        for annotator in &spec.annotators {
            for repeat in 0..u64::from(annotator.repeats.max(1)) {
                let key = format!("{digest}|{}|{repeat}", annotator.annotator_id);
                if refused_keys.contains(&key) {
                    continue;
                }
                expected.push((key, trace_id.to_string(), digest.to_string(), annotator.annotator_id.clone(), repeat));
            }
        }
    }
    let by_job: HashMap<&str, (&str, Option<&String>)> = bindings
        .iter()
        .map(|(key, (job_id, reservation))| (job_id.as_str(), (key.as_str(), reservation.as_ref())))
        .collect();
    let by_key: HashMap<&str, &(String, String, String, String, u64)> = expected
        .iter()
        .map(|entry| (entry.0.as_str(), entry))
        .collect();
    let aligned = expected.len() == job_ids.len();
    job_ids
        .iter()
        .enumerate()
        .map(|(index, job_id)| {
            let bound = by_job.get(job_id.as_str());
            let entry = bound
                .and_then(|(key, _)| by_key.get(key).copied())
                .or_else(|| if aligned { expected.get(index) } else { None });
            RecordedJob {
                job_id: job_id.clone(),
                trace_id: entry.map(|e| e.1.clone()),
                trace_digest: entry.map(|e| e.2.clone()),
                annotator_id: entry.map(|e| e.3.clone()),
                repeat_index: entry.map(|e| e.4),
                reservation_id: bound.and_then(|(_, reservation)| reservation.cloned()),
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Recording on the run
// ---------------------------------------------------------------------------

/// Append the stage outcome as an evidence amendment linked to the sealed
/// terminal: `annotation_job` refs keyed by trace digest plus the report in the
/// delta. Post-terminal, append-only, idempotent per campaign.
pub(crate) async fn record(
    service: &OptimizerService,
    run_id: &str,
    spec: &AnnotationStageSpec,
    report: &StageReport,
) -> Result<()> {
    let owned = run_id.to_string();
    let terminal_sequence = service
        .database()
        .run_read(move |conn| {
            let state = super::kernel::persist::load_state(conn, &owned)?
                .context("evaluation run has no saved kernel projection")?;
            Ok(state
                .terminal
                .as_ref()
                .map(|terminal| terminal.final_sequence))
        })
        .await?
        .context("the annotation stage may record only after a sealed terminal state")?;
    let mut refs = Vec::with_capacity(report.jobs.len() + 1);
    if let Some(campaign_id) = &report.campaign_id {
        refs.push(json!({"kind": CAMPAIGN_REF_KIND, "id": campaign_id, "digest": Value::Null}));
    }
    for job in &report.jobs {
        refs.push(json!({
            "kind": JOB_REF_KIND,
            "id": job.job_id,
            "digest": job.trace_digest,
        }));
    }
    let key = report
        .campaign_id
        .clone()
        .unwrap_or_else(|| report.status.clone());
    let draft = OptimizerEventDraft::new("optimizer.evidence.amended", EVAL_ALGORITHM_ID)
        .idempotency_key(format!("eval:annotation-stage:{terminal_sequence}:{key}"))
        .delta(Map::from_iter([
            ("terminalSequence".into(), json!(terminal_sequence)),
            ("annotationStage".into(), serde_json::to_value(report)?),
        ]))
        .artifact_refs(refs)
        .raw(json!({ "source": EVENT_SOURCE, "spec": spec }));
    service
        .append_event_payloads(run_id.to_string(), vec![draft])
        .await?;
    if let Ok(stage) = serde_json::to_value(report) {
        let eval_run_id = run_id.to_string();
        let label = spec.label.clone();
        let _ = service
            .database()
            .run_transaction(move |conn| {
                crate::session::annotation_projection::seed_from_stage_payload(
                    conn,
                    &eval_run_id,
                    Some(&label),
                    &stage,
                )
            })
            .await;
    }
    Ok(())
}

/// Annotation job ids recorded on a run, with the trace digest each annotates
/// (`None` when the container did not bind the job to a trace).
pub(crate) fn recorded_jobs(
    conn: &rusqlite::Connection,
    run_id: &str,
) -> Result<Vec<(String, Option<String>)>> {
    let mut statement = conn.prepare(
        "SELECT ref_id, digest FROM optimizer_evidence_refs
         WHERE optimizer_run_id = ?1 AND kind = ?2
         ORDER BY recorded_at, ref_id",
    )?;
    let rows = statement
        .query_map(rusqlite::params![run_id, JOB_REF_KIND], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::{serve_json, JsonHttpRequest, JsonHttpResponse};
    use crate::optimizers::models::{OptimizerCapabilities, OptimizerCreateRequest};
    use crate::optimizers::service::tests::service;
    use std::sync::Mutex;

    fn table(text: &str) -> toml::value::Table {
        let value: toml::Value = toml::from_str(text).unwrap();
        value
            .get("annotation")
            .and_then(toml::Value::as_table)
            .cloned()
            .unwrap()
    }

    #[test]
    fn parses_the_recipe_block_with_defaults_and_overrides() {
        let spec = AnnotationStageSpec::parse(
            "r",
            &table(
                r#"
                [annotation]
                annotators = ["craftax.deterministic", { id = "craftax.belief", repeats = 2, model = "gpt-5.6-luna" }]
                repeats = 1
                max_cost_usd = 0.8
                [annotation.throughput]
                deterministic = 4
                paid = 2
                "#,
            ),
        )
        .unwrap()
        .unwrap();
        assert_eq!(spec.annotators.len(), 2);
        assert_eq!(spec.annotators[0].annotator_id, "craftax.deterministic");
        assert_eq!(spec.annotators[0].repeats, 1);
        assert_eq!(spec.annotators[1].repeats, 2);
        assert_eq!(spec.annotators[1].model.as_deref(), Some("gpt-5.6-luna"));
        assert_eq!(spec.throughput["paid"], 2);
        assert_eq!(spec.max_cost_usd, Some(0.8));
        assert_eq!(spec.max_cost_usd_micros(), Some(800_000));
        assert_eq!(spec.label, "post_rollout");
        assert_eq!(
            spec.campaign_annotators()[1],
            json!({"annotator_id": "craftax.belief", "repeats": 2, "model": "gpt-5.6-luna"})
        );
        // round-trips through the recorded raw payload
        let echoed: AnnotationStageSpec =
            serde_json::from_value(serde_json::to_value(&spec).unwrap()).unwrap();
        assert_eq!(echoed, spec);
    }

    #[test]
    fn refuses_bad_blocks_where_they_are_read() {
        let cases = [
            ("[annotation]\nannotators = []", "at least one annotator"),
            ("[annotation]\nannotators = [\"a\", \"a\"]", "twice"),
            ("[annotation]\nannotators = [\"a\"]\nrepeats = 0", "1..=5"),
            ("[annotation]\nannotators = [\"a\"]\nmax_cost_usd = -1", "positive finite"),
            ("[annotation]\nannotators = [\"a\"]\nthroughput = { gpu = 2 }", "annotator class"),
            ("[annotation]\nannotators = [\"a\"]\nmystery = 1", "not an admitted option"),
            ("[annotation]\nannotators = [{ repeats = 1 }]", "need an id"),
        ];
        for (text, expected) in cases {
            let error = AnnotationStageSpec::parse("r", &table(text)).unwrap_err();
            assert!(error.to_string().contains(expected), "{text}: {error}");
        }
        assert!(AnnotationStageSpec::parse(
            "r",
            &table("[annotation]\nenabled = false\nannotators = [\"a\"]")
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn sealed_trace_refs_skip_partial_and_duplicate_traces() {
        let records = vec![
            json!({"sealedTrace": {"traces": [{"traceId": "host-t1", "producerTraceId": "t1", "digest": "sha256:aaa"}]}}),
            json!({"evidenceState": "sealed_partial", "sealedTrace": {"traces": [{"traceId": "t2", "digest": "sha256:bbb"}]}}),
            json!({"sealedTrace": {"traces": [{"traceId": "host-t1", "producerTraceId": "t1", "digest": "sha256:aaa"}]}}),
            json!({"reward": 1.0}),
        ];
        assert_eq!(
            sealed_trace_refs(&records),
            vec![json!({"kind": "trace_v5", "id": "t1", "digest": "sha256:aaa"})]
        );
    }

    /// The container's annotation router, reduced to the campaign endpoint.
    struct FakeContainer {
        base: String,
        requests: Arc<Mutex<Vec<Value>>>,
        _task: tokio::task::JoinHandle<()>,
    }

    async fn fake_container(paid_jobs: Vec<Value>, refuse_without_reservation: bool) -> FakeContainer {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let seen = requests.clone();
        let task = tokio::spawn(async move {
            let _ = serve_json(listener, move |request: JsonHttpRequest| {
                let seen = seen.clone();
                let paid_jobs = paid_jobs.clone();
                async move {
                    if request.path != "/annotation/campaigns" {
                        return JsonHttpResponse::error(hyper::StatusCode::NOT_FOUND, "nope");
                    }
                    seen.lock().unwrap().push(request.body.clone());
                    let body = &request.body;
                    let traces = body["traces"].as_array().cloned().unwrap_or_default();
                    let annotators = body["annotators"].as_array().cloned().unwrap_or_default();
                    if body["estimate_only"].as_bool() == Some(true) {
                        let total: f64 = paid_jobs
                            .iter()
                            .filter_map(|job| job["max_cost_usd"].as_f64())
                            .sum();
                        return JsonHttpResponse::ok(json!({"estimate": {
                            "job_count": traces.len() * annotators.len(),
                            "max_cost_usd": total,
                            "paid_jobs": paid_jobs,
                            "notes": ["fake estimate"],
                        }}));
                    }
                    let reservations = body["reservations"].as_object().cloned().unwrap_or_default();
                    let mut jobs = Vec::new();
                    let mut bindings = Vec::new();
                    let mut refused = Vec::new();
                    let mut n = 0;
                    for trace in &traces {
                        for annotator in &annotators {
                            let repeats = annotator["repeats"].as_u64().unwrap_or(1).max(1);
                            for repeat in 0..repeats {
                                let key = format!(
                                    "{}|{}|{repeat}",
                                    trace["digest"].as_str().unwrap(),
                                    annotator["annotator_id"].as_str().unwrap()
                                );
                                let is_paid = paid_jobs.iter().any(|job| {
                                    job["trace_digest"] == trace["digest"]
                                        && job["annotator_id"] == annotator["annotator_id"]
                                        && job["repeat_index"].as_u64().unwrap_or(0) == repeat
                                });
                                if is_paid && refuse_without_reservation && !reservations.contains_key(&key) {
                                    refused.push(json!({
                                        "annotator_id": annotator["annotator_id"],
                                        "trace_digest": trace["digest"],
                                        "repeat_index": repeat,
                                        "reason": "reservation_required",
                                    }));
                                    continue;
                                }
                                n += 1;
                                let job_id = format!("ajob_{n}");
                                if reservations.contains_key(&key) {
                                    bindings.push(json!({"key": key, "job_id": job_id, "reservation_id": format!("container_rsv_{n}")}));
                                }
                                jobs.push(json!(job_id));
                            }
                        }
                    }
                    JsonHttpResponse::ok(json!({
                        "campaign_id": "acmp_1",
                        "jobs": jobs,
                        "job_bindings": bindings,
                        "cache_hits": 0,
                        "enqueued": jobs.len(),
                        "refused": refused,
                    }))
                }
            })
            .await;
        });
        FakeContainer {
            base,
            requests,
            _task: task,
        }
    }

    async fn sealed_eval_run(svc: &OptimizerService, id: &str, session: Option<&str>) -> String {
        let (run, _) = svc
            .create(OptimizerCreateRequest {
                algorithm_id: "eval".into(),
                algorithm_version: Some("1".into()),
                objective: Some("annotation stage".into()),
                source: Some("local".into()),
                project_ref: None,
                session_ref: session.map(str::to_owned),
                id: Some(id.into()),
                execution_bindings: None,
                input_refs: None,
                capabilities: Some(OptimizerCapabilities::for_algorithm("eval")),
                summary: Some(json!({ "recipeId": "eval.annotated.v1" })),
                open_visual: Some(false),
                seed_fixture: None,
                cloud_config: None,
                local_path: None,
            })
            .await
            .unwrap();
        svc.append_event_payloads(
            run.id.clone(),
            vec![OptimizerEventDraft::new("optimizer.run.started", "eval")
                .raw(json!({"source": "test"}))],
        )
        .await
        .unwrap();
        svc.settle_run(
            run.id.clone(),
            crate::optimizers::kernel::SettleCause::Completed,
            None,
        )
        .await
        .unwrap();
        run.id
    }

    fn records() -> Vec<Value> {
        vec![
            json!({"rolloutId": "r1", "sealedTrace": {"traces": [{"traceId": "t1", "digest": "sha256:1111"}]}}),
            json!({"rolloutId": "r2", "sealedTrace": {"traces": [{"traceId": "t2", "digest": "sha256:2222"}]}}),
        ]
    }

    fn parse_spec(text: &str) -> AnnotationStageSpec {
        AnnotationStageSpec::parse("r", &table(text)).unwrap().unwrap()
    }

    #[tokio::test]
    async fn free_campaign_is_submitted_after_the_seal_and_recorded_per_trace() {
        let (svc, _dir, _) = service().await;
        let container = fake_container(vec![], false).await;
        let run_id = sealed_eval_run(&svc, "annot_free", None).await;
        let status_before = svc.get(run_id.clone()).await.unwrap().status;
        let spec = parse_spec("[annotation]\nannotators = [\"craftax.deterministic\", { id = \"generic.reasoning\", repeats = 2 }]");
        let report = run_after_terminal(&svc, &run_id, &spec, "ctr_1", &container.base, &records()).await;
        assert_eq!(report.status, "submitted", "{report:?}");
        assert_eq!(report.campaign_id.as_deref(), Some("acmp_1"));
        assert_eq!(report.jobs.len(), 6);
        assert_eq!(report.paid_jobs, 0);
        assert_eq!(report.notes, vec!["fake estimate".to_string()]);
        // plan order: trace -> annotator -> repeat
        assert_eq!(report.jobs[0].trace_digest.as_deref(), Some("sha256:1111"));
        assert_eq!(report.jobs[0].annotator_id.as_deref(), Some("craftax.deterministic"));
        assert_eq!(report.jobs[2].repeat_index, Some(1));
        assert_eq!(report.jobs[3].trace_digest.as_deref(), Some("sha256:2222"));
        let sent = container.requests.lock().unwrap().clone();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0]["estimate_only"], json!(true));
        assert_eq!(sent[1]["estimate_only"], json!(false));
        assert!(sent[1].get("reservations").is_none());
        assert_eq!(sent[1]["traces"][1]["digest"], json!("sha256:2222"));
        assert_eq!(sent[1]["label"], json!("post_rollout"));
        // recorded on the run as evidence the UI and skills already read
        let jobs = svc
            .database()
            .with_conn(|conn| recorded_jobs(conn, &run_id))
            .unwrap();
        assert_eq!(jobs.len(), 6);
        assert!(jobs.iter().any(|(id, digest)| id == "ajob_4" && digest.as_deref() == Some("sha256:2222")));
        let seeded = svc
            .database()
            .with_conn(|conn| {
                let campaign: String = conn.query_row(
                    "SELECT status FROM annotation_campaigns WHERE campaign_id='acmp_1'",
                    [],
                    |row| row.get(0),
                )?;
                let n: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM annotation_jobs WHERE campaign_id='acmp_1'",
                    [],
                    |row| row.get(0),
                )?;
                Ok((campaign, n))
            })
            .unwrap();
        assert_eq!(seeded.0, "submitted");
        assert_eq!(seeded.1, 6);
        let evidence = svc
            .get_state(run_id.clone(), "run.evidence".into(), None)
            .await
            .unwrap();
        let kinds: Vec<&str> = evidence.data["refs"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|r| r["kind"].as_str())
            .collect();
        assert!(kinds.contains(&JOB_REF_KIND) && kinds.contains(&CAMPAIGN_REF_KIND), "{kinds:?}");
        let events = svc.events_after(run_id.clone(), 0, Some(100)).await.unwrap();
        let amendment = events
            .iter()
            .find(|event| event.event_type == "optimizer.evidence.amended")
            .expect("amendment");
        assert_eq!(amendment.delta["annotationStage"]["status"], json!("submitted"));
        // the terminal manifest is untouched
        // the seal and the run outcome are exactly what they were before lane B
        assert!(svc.terminal_manifest(run_id.clone()).await.unwrap().is_some(), "still sealed");
        assert_eq!(svc.get(run_id).await.unwrap().status, status_before);
    }

    #[tokio::test]
    async fn paid_jobs_are_approved_once_then_reserved_per_job_and_forwarded() {
        let (svc, _dir, _) = service().await;
        let paid_jobs = vec![
            json!({"trace_id": "t1", "trace_digest": "sha256:1111", "annotator_id": "craftax.belief", "repeat_index": 0, "model": "gpt-5.6-luna", "max_cost_usd": 0.25}),
            json!({"trace_id": "t2", "trace_digest": "sha256:2222", "annotator_id": "craftax.belief", "repeat_index": 0, "model": "gpt-5.6-luna", "max_cost_usd": 0.25}),
        ];
        let container = fake_container(paid_jobs, true).await;
        let run_id = sealed_eval_run(&svc, "annot_paid", Some("sess_1")).await;
        svc.database()
            .with_conn(|conn| reservations::store_broker_secret(conn, "ctr_1", "secret"))
            .unwrap();
        let approvals = Arc::new(Mutex::new(Vec::new()));
        let seen = approvals.clone();
        let approver = paid_approver(move |request: PaidApprovalRequest| {
            let seen = seen.clone();
            Box::pin(async move {
                seen.lock().unwrap().push(request.clone());
                Ok(PaidApprovalGrant {
                    approval_id: "apr_1".into(),
                    cap_usd_micros: request.cap_usd_micros,
                })
            })
        });
        let spec = parse_spec("[annotation]\nannotators = [\"craftax.deterministic\", \"craftax.belief\"]\nmax_cost_usd = 0.8");
        let report = execute(&svc, &run_id, &spec, "ctr_1", &container.base, &records(), Some(approver))
            .await
            .unwrap();
        assert_eq!(report.status, "submitted");
        assert_eq!(report.paid_jobs, 2);
        assert_eq!(report.approval_id.as_deref(), Some("apr_1"));
        assert_eq!(report.jobs.len(), 4, "{:?}", report.jobs);
        assert!(report.refused.is_empty());
        // one approval for the whole campaign, bound to the plan
        let approvals = approvals.lock().unwrap();
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].cap_usd_micros, 500_000);
        assert_eq!(approvals[0].paid_jobs, 2);
        assert!(approvals[0].binding_digest.starts_with("sha256:"));
        // one signed reservation per paid job, keyed the way the container looks them up
        let sent = container.requests.lock().unwrap().clone();
        let tokens = sent[1]["reservations"].as_object().unwrap();
        assert_eq!(tokens.len(), 2);
        assert!(tokens.contains_key("sha256:1111|craftax.belief|0"));
        assert!(tokens.contains_key("sha256:2222|craftax.belief|0"));
        // forwarded with the container's job id so settlement can find it
        let paid = report
            .jobs
            .iter()
            .filter(|job| job.annotator_id.as_deref() == Some("craftax.belief"))
            .collect::<Vec<_>>();
        assert_eq!(paid.len(), 2);
        for job in paid {
            let row = svc
                .database()
                .with_conn(|conn| reservations::by_job(conn, "ctr_1", &job.job_id))
                .unwrap()
                .expect("forwarded reservation");
            assert_eq!(row.status, "forwarded");
            assert_eq!(row.approval_id, "apr_1");
            assert_eq!(row.reserved_usd_micros, 250_000);
        }
    }

    #[tokio::test]
    async fn paid_lane_is_skipped_not_fatal_when_it_cannot_be_prepared() {
        let (svc, _dir, _) = service().await;
        let paid_jobs = vec![json!({"trace_id": "t1", "trace_digest": "sha256:1111", "annotator_id": "craftax.belief", "repeat_index": 0, "model": "m", "max_cost_usd": 0.25})];
        let container = fake_container(paid_jobs, true).await;
        // no session, no max_cost_usd, no approver: the free annotator still runs
        let run_id = sealed_eval_run(&svc, "annot_paid_skipped", None).await;
        let spec = parse_spec("[annotation]\nannotators = [\"craftax.deterministic\", \"craftax.belief\"]");
        let report = run_after_terminal(&svc, &run_id, &spec, "ctr_1", &container.base, &records()).await;
        assert_eq!(report.status, "submitted");
        assert!(report.notes.iter().any(|note| note.contains("paid lane skipped") && note.contains("session")), "{:?}", report.notes);
        assert_eq!(report.refused.len(), 1);
        assert_eq!(report.refused[0]["reason"], json!("reservation_required"));
        assert_eq!(report.jobs.len(), 3);
        assert!(report.jobs.iter().all(|job| job.trace_digest.is_some()));
        let sent = container.requests.lock().unwrap().clone();
        assert!(sent[1].get("reservations").is_none());

        // a ceiling below the estimate never reaches the approver
        let run_id = sealed_eval_run(&svc, "annot_ceiling", Some("sess_2")).await;
        svc.database()
            .with_conn(|conn| reservations::store_broker_secret(conn, "ctr_1", "secret"))
            .unwrap();
        let asked = Arc::new(Mutex::new(0usize));
        let counter = asked.clone();
        let approver = paid_approver(move |_| {
            *counter.lock().unwrap() += 1;
            Box::pin(async { Err(anyhow!("must not be asked")) })
        });
        let spec = parse_spec("[annotation]\nannotators = [\"craftax.belief\"]\nmax_cost_usd = 0.1");
        let report = execute(&svc, &run_id, &spec, "ctr_1", &container.base, &records(), Some(approver))
            .await
            .unwrap();
        assert_eq!(*asked.lock().unwrap(), 0);
        assert!(report.notes.iter().any(|note| note.contains("annotation.max_cost_usd")), "{:?}", report.notes);
        assert_eq!(report.approval_id, None);
    }

    #[tokio::test]
    async fn an_unreachable_container_is_recorded_as_a_failed_stage_not_an_error() {
        let (svc, _dir, _) = service().await;
        let run_id = sealed_eval_run(&svc, "annot_unreachable", None).await;
        let status_before = svc.get(run_id.clone()).await.unwrap().status;
        let spec = parse_spec("[annotation]\nannotators = [\"craftax.deterministic\"]");
        let report = run_after_terminal(&svc, &run_id, &spec, "ctr_1", "http://127.0.0.1:9", &records()).await;
        assert_eq!(report.status, "failed");
        assert!(report.error.as_deref().unwrap_or_default().contains("annotation/campaigns"), "{:?}", report.error);
        let events = svc.events_after(run_id.clone(), 0, Some(100)).await.unwrap();
        let amendment = events
            .iter()
            .find(|event| event.event_type == "optimizer.evidence.amended")
            .expect("failure is still recorded");
        assert_eq!(amendment.delta["annotationStage"]["status"], json!("failed"));
        assert!(amendment.artifact_refs.is_empty());
        // the seal and the run outcome are exactly what they were before lane B
        assert!(svc.terminal_manifest(run_id.clone()).await.unwrap().is_some(), "still sealed");
        assert_eq!(svc.get(run_id).await.unwrap().status, status_before);
    }

    #[tokio::test]
    async fn no_sealed_traces_means_skipped_without_touching_the_container() {
        let (svc, _dir, _) = service().await;
        let run_id = sealed_eval_run(&svc, "annot_skip", None).await;
        let spec = parse_spec("[annotation]\nannotators = [\"craftax.deterministic\"]");
        let report = run_after_terminal(&svc, &run_id, &spec, "ctr_1", "http://127.0.0.1:9", &[json!({"reward": 1.0})]).await;
        assert_eq!(report.status, "skipped");
        assert!(report.jobs.is_empty());
    }
}
