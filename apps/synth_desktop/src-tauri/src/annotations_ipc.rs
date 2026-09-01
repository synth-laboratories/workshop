//! Desktop IPC for Trace V5 annotation: `/v1/annotations/{operation}`.
//!
//! Every operation is proxied by identity to the owning container's annotation
//! router (`synth_containers.tracing.annotation.api`). The host adds what the
//! container cannot know: which loopback container owns a trace, whether the
//! session approved a bounded charge, and the single-use reservation that
//! carries that approval. Money never crosses this boundary as a number the
//! agent chose: paid operations are estimated by the container, approved by the
//! host's broker, and forwarded as a signed reservation token.

use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager};

use crate::core_runtime::CoreRuntime;
use crate::error::StructuredFailure;
use crate::limits;
use crate::session::annotation_reservation::{self as reservations, ReservationBinding};
use crate::session::approval::{ApprovalBroker, ApprovalDecision, ApprovalKind, PaidComputeCap};
use crate::session::paid_compute_budget::{micros_from_reported_cost, SettlementOutcome};

/// Mirrors `bin/synth_annotations_mcp.rs::OPERATIONS`; a contract test keeps them equal.
pub const OPERATIONS: &[(&str, bool, bool)] = &[
    ("annotation_list_definitions", true, false),
    ("annotation_estimate", true, false),
    ("annotation_start", false, true),
    ("annotation_get", true, false),
    ("annotation_cancel", false, false),
    ("annotation_list", true, false),
    ("annotation_get_evidence", true, false),
    ("verification_start", false, true),
    ("verification_get", true, false),
    ("annotation_review", false, false),
    ("annotation_consensus", false, false),
    ("annotation_campaign", false, true),
];

const ALLOWED_ARGUMENTS: &[&str] = &[
    "trace_id",
    "job_id",
    "annotation_id",
    "annotator_id",
    "domain",
    "request",
    "reservation_id",
    "session_id",
    "sessionRef",
    "filters",
    "decision",
    "reviewer",
    "rationale",
    "evidence",
    "majority_threshold",
    "run_id",
    "annotators",
    "estimate_only",
    "container_id",
    "containerId",
    "traces",
    "label",
    "repeats",
];

const RESERVATION_TTL_SECONDS: i64 = 900;

fn failure(
    code: &'static str,
    message: impl Into<String>,
    remediation: impl Into<String>,
) -> anyhow::Error {
    anyhow::Error::new(StructuredFailure::new(code, message, remediation))
}

fn string_field(body: &Value, snake: &str, camel: &str) -> Option<String> {
    body.get(snake)
        .or_else(|| body.get(camel))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

pub(crate) fn operation_from_path(path: &str) -> Result<&'static str> {
    let operation = path
        .trim_start_matches("/v1/annotations/")
        .trim_end_matches('/');
    OPERATIONS
        .iter()
        .find(|(name, _, _)| *name == operation)
        .map(|(name, _, _)| *name)
        .ok_or_else(|| {
            failure(
                "annotation_operation_unknown",
                format!("unknown annotation operation `{operation}`"),
                "use an operation listed by annotation_manage",
            )
        })
}

pub(crate) fn validate_arguments(body: &Value) -> Result<()> {
    if let Some(object) = body.as_object() {
        for key in object.keys() {
            if !ALLOWED_ARGUMENTS.contains(&key.as_str()) {
                return Err(failure("annotation_argument_rejected", format!("annotation arguments reject `{key}`"), "pass identities only: trace/job/annotation/container ids, a request, a reservation_id"));
            }
        }
        if let Some(reservation) = object.get("reservation_id") {
            if !reservation.is_string() {
                return Err(failure(
                    "annotation_argument_rejected",
                    "reservation_id must be an opaque string",
                    "pass the reservation id the host returned",
                ));
            }
        }
    }
    Ok(())
}

async fn container_base(core: &CoreRuntime, container_id: &str) -> Result<String> {
    let container = core
        .data()
        .get_container(container_id.to_string())
        .await
        .map_err(|error| failure("annotation_container_unknown", format!("container `{container_id}` is not registered: {error}"), "register the container (workshop.containers.toml or container_list) and pass its immutable id"))?;
    let base = container
        .base_url
        .as_deref()
        .context("container has no base URL")?;
    crate::visuals_ipc::validated_loopback_rollout_base(base)
}

fn client() -> Result<reqwest::Client> {
    Ok(crate::http::http_client_builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(limits::ANNOTATION_IPC_TIMEOUT)
        .build()?)
}

/// Known container error codes keep their identity through the proxy.
fn container_code(code: &str) -> &'static str {
    match code {
        "reservation_required" => "reservation_required",
        "reservation_rejected" => "reservation_rejected",
        "rubric_required" => "rubric_required",
        "definition_unknown" => "definition_unknown",
        "definition_digest_mismatch" => "definition_digest_mismatch",
        "source_trace_unavailable" => "source_trace_unavailable",
        "revision_conflict" => "revision_conflict",
        "evidence_invalid" => "evidence_invalid",
        "unsupported_finding" => "unsupported_finding",
        "job_not_found" => "job_not_found",
        "annotation_not_found" => "annotation_not_found",
        _ => "annotation_container_error",
    }
}

async fn forward(method: &str, url: &str, body: Option<&Value>) -> Result<Value> {
    let client = client()?;
    let request = match method {
        "GET" => client.get(url),
        _ => client.post(url).json(body.unwrap_or(&Value::Null)),
    };
    let response = request.send().await.map_err(|error| {
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
    let code = detail
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("annotation_container_error");
    let message = detail
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or(&text)
        .to_string();
    let mut structured = StructuredFailure::new(
        container_code(code),
        format!("{status}: {message}"),
        "inspect the container error detail",
    );
    structured.details = detail;
    Err(anyhow::Error::new(structured))
}

fn query_string(filters: Option<&Value>) -> String {
    let Some(object) = filters.and_then(Value::as_object) else {
        return String::new();
    };
    let pairs: Vec<String> = object
        .iter()
        .filter_map(|(key, value)| {
            let text = match value {
                Value::String(text) => text.clone(),
                Value::Bool(flag) => flag.to_string(),
                Value::Number(number) => number.to_string(),
                _ => return None,
            };
            Some(format!(
                "{}={}",
                urlencoding_encode(key),
                urlencoding_encode(&text)
            ))
        })
        .collect();
    if pairs.is_empty() {
        String::new()
    } else {
        format!("?{}", pairs.join("&"))
    }
}

fn urlencoding_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn trace_id_of(body: &Value) -> Result<String> {
    string_field(body, "trace_id", "traceId")
        .or_else(|| {
            body.get("request")
                .and_then(|r| r.get("source_trace_id"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .ok_or_else(|| {
            failure(
                "annotation_argument_missing",
                "trace_id required",
                "pass the sealed trace id",
            )
        })
}

fn micros_of(value: Option<&Value>) -> Option<u64> {
    value
        .and_then(Value::as_f64)
        .and_then(micros_from_reported_cost)
}

struct PaidGrant {
    approval_id: String,
    cap_usd_micros: u64,
}

#[derive(Debug)]
pub(crate) struct PaidPreflight {
    pub binding: ReservationBinding,
    pub cap_usd_micros: u64,
    pub model: String,
    /// The container Workshop's own trace index records as the sealer of the
    /// bound trace. Equal to the caller's container id by construction; the
    /// approval card names this one so the human sees who is charging.
    pub container_id: String,
    /// Workshop's local `traces.id` for the bound digest (`tracev5_…`).
    pub trace_row_id: String,
}

/// What Workshop's trace index knows about who sealed a trace.
struct TraceOwner {
    row_id: String,
    source: String,
    container_id: Option<String>,
}

/// Look a sealed trace up by digest in `traces`. `None` when the digest was
/// never imported into this Workshop.
async fn trace_owner(core: &CoreRuntime, trace_digest: &str) -> Result<Option<TraceOwner>> {
    let lookup = crate::trace_ingest::qualified_sha256(trace_digest)
        .unwrap_or_else(|_| trace_digest.to_string());
    core.storage()
        .database()
        .run_read(move |conn| {
            use rusqlite::OptionalExtension;
            Ok(conn
                .query_row(
                    "SELECT id, source, container_id FROM traces WHERE digest=?1",
                    rusqlite::params![lookup],
                    |row| {
                        Ok(TraceOwner {
                            row_id: row.get(0)?,
                            source: row.get(1)?,
                            container_id: row.get(2)?,
                        })
                    },
                )
                .optional()?)
        })
        .await
}

/// Paid annotation charges a human's budget on behalf of one container, so
/// the trace must have a recorded owner and it must be the container asked
/// for. Fails closed with a distinct code for each way that can be untrue.
async fn require_trace_owner(
    core: &CoreRuntime,
    container_id: &str,
    trace_digest: &str,
) -> Result<TraceOwner> {
    let owner = trace_owner(core, trace_digest).await?.ok_or_else(|| {
        failure(
            "annotation_trace_unknown",
            format!("trace `{trace_digest}` is not in Workshop's trace index, so no owning container can be named on the approval card"),
            format!("import the sealed trace from container `{container_id}` first (data_trace_materialize / the traces MCP container import with the container's immutable id), then start the paid job again"),
        )
    })?;
    match owner.container_id.as_deref() {
        None => Err(failure(
            "annotation_trace_unowned",
            format!(
                "trace `{trace_digest}` (Workshop row `{}`, imported via `{}`) has no owning container recorded; it was imported from a bare file, so Workshop cannot name who sealed it on the approval card",
                owner.row_id, owner.source
            ),
            format!("re-import the trace from the container that sealed it (container `{container_id}` and its rollout id) so the owner is recorded; bare file imports cannot fund paid annotation"),
        )),
        Some(recorded) if recorded != container_id => Err(failure(
            "annotation_container_mismatch",
            format!("trace `{trace_digest}` was sealed by container `{recorded}`, not `{container_id}`"),
            format!("pass container_id `{recorded}` (its immutable id from container_list)"),
        )),
        Some(_) => Ok(owner),
    }
}

/// Everything a paid job needs *before* a human is asked: a session, a bounded
/// estimate, a resolved model, and a container Workshop launched (so it holds a
/// reservation secret). Fails closed without touching the approval broker.
pub(crate) async fn paid_preflight(
    core: &CoreRuntime,
    container_id: &str,
    session_id: Option<&str>,
    request: &Value,
    estimate: &Value,
) -> Result<PaidPreflight> {
    let session = session_id.ok_or_else(|| {
        failure(
            "annotation_session_required",
            "paid annotation needs a session",
            "call from an agent session so approval can be routed",
        )
    })?;
    let cap = micros_of(estimate.get("max_cost_usd")).ok_or_else(|| {
        failure(
            "annotation_cap_unbounded",
            "estimate has no max_cost_usd",
            "declare limits.max_cost_usd on the request",
        )
    })?;
    let model = estimate
        .get("resolved_model")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            failure(
                "annotation_cap_unbounded",
                "estimate resolved no model",
                "pass a model or configure a runner default",
            )
        })?;
    let cid = container_id.to_string();
    let has_secret = core
        .storage()
        .database()
        .run_read(move |conn| reservations::load_broker_secret(conn, &cid))
        .await?
        .is_some();
    if !has_secret {
        return Err(failure("reservation_broker_unavailable", format!("container `{container_id}` was not launched by Workshop; no reservation secret exists"), "launch the container through workshop.containers.toml so Workshop can inject the annotation broker secret"));
    }
    let trace_digest = request
        .get("source_trace_digest")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            failure(
                "annotation_binding_incomplete",
                "paid annotation resolved no trace digest",
                "estimate a request bound to a sealed Trace V5 digest",
            )
        })?;
    let annotator_id = request
        .get("annotator_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            failure(
                "annotation_binding_incomplete",
                "paid annotation resolved no annotator",
                "pass a registered annotator id",
            )
        })?;
    let owner = require_trace_owner(core, container_id, trace_digest).await?;
    Ok(PaidPreflight {
        binding: ReservationBinding {
            trace_digest: trace_digest.to_string(),
            annotator_id: annotator_id.to_string(),
            model: model.clone(),
            session_id: session.to_string(),
        },
        cap_usd_micros: cap,
        model,
        container_id: owner.container_id.unwrap_or_else(|| container_id.to_string()),
        trace_row_id: owner.row_id,
    })
}

async fn approve(
    app: &AppHandle,
    session_id: &str,
    operation: &str,
    parameters: Value,
    cap_usd_micros: u64,
    model: Option<&str>,
    binding_digest: &str,
) -> Result<PaidGrant> {
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
        max_cost_usd_micros: Some(cap_usd_micros),
        max_rollouts: None,
    };
    if !requested.is_bounded() {
        return Err(failure(
            "annotation_cap_unbounded",
            "annotation estimate declares no enforceable cost cap",
            "declare limits.max_cost_usd on the request",
        ));
    }
    let (approval_id, decision) = broker
        .authorize_host_outcome(
            app,
            Some(session_id),
            ApprovalKind::PaidCompute {
                operation: format!("annotation.{operation}"),
                parameters,
                estimated_cost_usd_micros: Some(cap_usd_micros),
                requested_cap: requested.clone(),
                requesting_agent: "annotation_manage".to_string(),
                recipe_id: None,
                dataset: None,
                proposer_model: model.map(str::to_owned),
                evaluator_model: None,
                timeout_seconds: None,
                credential_names: vec![],
                preparation_digest: Some(binding_digest.to_string()),
            },
        )
        .await
        .map_err(|error| {
            failure(
                "approval_rejected",
                error.to_string(),
                "ask the user for a bounded approval and retry with a fresh estimate",
            )
        })?;
    let granted = match decision {
        ApprovalDecision::Reject => {
            return Err(failure(
                "approval_rejected",
                "paid annotation was rejected",
                "do not retry without a new approval",
            ))
        }
        ApprovalDecision::ApproveWithCap { cap } => cap
            .max_cost_usd_micros
            .unwrap_or(cap_usd_micros)
            .min(cap_usd_micros),
        ApprovalDecision::Approve { .. } => cap_usd_micros,
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
    Ok(PaidGrant {
        approval_id,
        cap_usd_micros: granted,
    })
}

async fn issue_reservation(
    core: &CoreRuntime,
    container_id: &str,
    session_id: &str,
    grant: &PaidGrant,
    binding: ReservationBinding,
) -> Result<reservations::IssuedReservation> {
    let container_id = container_id.to_string();
    let session_id = session_id.to_string();
    let approval_id = grant.approval_id.clone();
    let cap = grant.cap_usd_micros;
    core.storage()
        .database()
        .run_transaction(move |conn| {
            let secret = reservations::load_broker_secret(conn, &container_id)?
                .ok_or_else(|| failure("reservation_broker_unavailable", format!("container `{container_id}` was not launched by Workshop; no reservation secret exists"), "launch the container through workshop.containers.toml so Workshop can inject the annotation broker secret"))?;
            reservations::issue(conn, &secret, &container_id, &session_id, &approval_id, &binding, cap, "workshop", RESERVATION_TTL_SECONDS)
        })
        .await
}

async fn release_reservation(core: &CoreRuntime, reservation_id: String) {
    let _ = core
        .storage()
        .database()
        .run_transaction(move |conn| reservations::release(conn, &reservation_id))
        .await;
}

async fn settle_if_terminal(core: &CoreRuntime, container_id: &str, payload: &Value) -> Result<()> {
    if payload.get("terminal").and_then(Value::as_bool) != Some(true) {
        return Ok(());
    }
    let Some(job_id) = payload
        .get("job")
        .and_then(|job| job.get("job_id"))
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return Ok(());
    };
    let cost = payload
        .get("job")
        .and_then(|job| job.get("usage"))
        .and_then(|usage| usage.get("cost_usd"))
        .cloned();
    let outcome = match micros_of(cost.as_ref()) {
        Some(micros) => SettlementOutcome::Exact {
            cost_usd_micros: micros,
        },
        None => SettlementOutcome::Unknown,
    };
    let container_id = container_id.to_string();
    core.storage()
        .database()
        .run_transaction(move |conn| {
            if let Some(row) = reservations::by_job(conn, &container_id, &job_id)? {
                reservations::settle(conn, &row.reservation_id, outcome)?;
            }
            Ok(())
        })
        .await
}

async fn apply_reconciliation(
    core: &CoreRuntime,
    container_id: &str,
    report: &Value,
) -> Result<()> {
    let reservation_id = report
        .get("reservation_id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("reconciliation has no reservation_id"))?
        .to_string();
    let job_id = report
        .get("claimed_by_job_id")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("reconciliation has no claimed job"))?
        .to_string();
    let outcome = match report.get("actual_cost_usd_micros").and_then(Value::as_u64) {
        Some(cost_usd_micros) => SettlementOutcome::Exact { cost_usd_micros },
        None => SettlementOutcome::Unknown,
    };
    let expected_container = container_id.to_string();
    core.storage()
        .database()
        .run_transaction(move |conn| {
            let row = reservations::load(conn, &reservation_id)?
                .ok_or_else(|| anyhow!("unknown annotation reservation {reservation_id}"))?;
            anyhow::ensure!(
                row.container_id == expected_container,
                "reservation container mismatch"
            );
            anyhow::ensure!(
                row.job_id.as_deref() == Some(job_id.as_str()),
                "reservation job mismatch"
            );
            reservations::settle(conn, &reservation_id, outcome)?;
            Ok(())
        })
        .await
}

async fn pull_reconciliations(core: &CoreRuntime, container_id: &str, base: &str) -> Result<usize> {
    let payload = forward("GET", &format!("{base}/annotation/reservations"), None).await?;
    let reports = payload
        .get("reconciled")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut applied = 0;
    for report in reports {
        if apply_reconciliation(core, container_id, &report)
            .await
            .is_ok()
        {
            applied += 1;
        }
    }
    Ok(applied)
}

/// Reconciliation is host-owned and continues even when no agent polls a job.
/// The container retains terminal reports, so every pass is idempotent and a
/// Workshop restart resumes from SQLite plus the container's durable outbox.
pub(crate) fn spawn_reconciler(core: Arc<CoreRuntime>) {
    tauri::async_runtime::spawn(async move {
        let mut ticks = tokio::time::interval(std::time::Duration::from_secs(10));
        loop {
            ticks.tick().await;
            let container_ids: Vec<String> = core.storage().database().run_read(|conn| {
                let mut statement = conn.prepare("SELECT DISTINCT container_id FROM annotation_reservations WHERE status = 'forwarded'")?;
                let rows = statement.query_map([], |row| row.get(0))?.collect::<std::result::Result<Vec<String>, _>>()?;
                Ok(rows)
            }).await.unwrap_or_default();
            for container_id in container_ids {
                if let Ok(base) = container_base(&core, &container_id).await {
                    let _ = pull_reconciliations(&core, &container_id, &base).await;
                }
            }
            let _ = pull_campaign_jobs(&core).await;
            let _ = core
                .storage()
                .database()
                .run_transaction(reservations::expire_stale)
                .await;
        }
    });
}

async fn pull_campaign_jobs(core: &CoreRuntime) -> Result<usize> {
    let _ = core
        .storage()
        .database()
        .run_transaction(crate::session::annotation_projection::seed_from_amendments)
        .await;
    let jobs = core
        .storage()
        .database()
        .run_read(crate::session::annotation_projection::list_jobs_needing_reconcile)
        .await
        .unwrap_or_default();
    let mut projected = 0usize;
    for job in jobs {
        let Some(container_id) = job.container_id.clone() else {
            continue;
        };
        let Ok(base) = container_base(core, &container_id).await else {
            continue;
        };
        let url = format!("{base}/annotation-jobs/{}", job.job_id);
        let Ok(payload) = forward("GET", &url, None).await else {
            continue;
        };
        let campaign_id = job.campaign_id.clone();
        let outcome = core
            .storage()
            .database()
            .run_transaction({
                let campaign_id = campaign_id.clone();
                let container_id = container_id.clone();
                move |conn| {
                    crate::session::annotation_projection::apply_job_snapshot(
                        conn,
                        campaign_id.as_deref(),
                        &container_id,
                        &payload,
                    )
                }
            })
            .await;
        let Ok(outcome) = outcome else {
            continue;
        };
        if outcome.already_projected {
            continue;
        }
        if !outcome.terminal || matches!(outcome.state.as_str(), "failed" | "cancelled") {
            let job_id = outcome.job_id.clone();
            let campaign_id = outcome.campaign_id.clone();
            let mark_projected = outcome.terminal;
            let _ = core
                .storage()
                .database()
                .run_transaction(move |conn| {
                    if mark_projected {
                        crate::session::annotation_projection::mark_job_projected(
                            conn, &job_id, None,
                        )?;
                    }
                    if let Some(campaign_id) = campaign_id {
                        crate::session::annotation_projection::refresh_campaign_coverage(
                            conn,
                            &campaign_id,
                        )?;
                    }
                    Ok(())
                })
                .await;
            continue;
        }
        let Some(trace_id) = outcome.trace_id.clone() else {
            let job_id = outcome.job_id.clone();
            let campaign_id = outcome.campaign_id.clone();
            let _ = core
                .storage()
                .database()
                .run_transaction(move |conn| {
                    crate::session::annotation_projection::mark_job_projected(conn, &job_id, None)?;
                    if let Some(campaign_id) = campaign_id {
                        crate::session::annotation_projection::refresh_campaign_coverage(
                            conn,
                            &campaign_id,
                        )?;
                    }
                    Ok(())
                })
                .await;
            continue;
        };
        let Some(campaign_id) = outcome.campaign_id.clone() else {
            continue;
        };
        let annotations = forward(
            "GET",
            &format!("{base}/traces/{trace_id}/annotations"),
            None,
        )
        .await
        .ok();
        let head_payload = forward(
            "GET",
            &format!("{base}/traces/{trace_id}/evidence-head"),
            None,
        )
        .await
        .ok();
        let bundles = if let Some(head_payload) = head_payload {
            Some(json!({
                "bundles": [head_payload.get("head").cloned().unwrap_or(head_payload)]
            }))
        } else {
            forward(
                "GET",
                &format!("{base}/traces/{trace_id}/evidence-bundles"),
                None,
            )
            .await
            .ok()
        };
        let (Some(annotations), Some(bundles)) = (annotations, bundles) else {
            continue;
        };
        let trace_digest = outcome.trace_digest.clone();
        let head = core
            .storage()
            .database()
            .run_transaction(move |conn| {
                crate::session::annotation_projection::project_trace_head(
                    conn,
                    &campaign_id,
                    &trace_id,
                    &trace_digest,
                    &annotations,
                    &bundles,
                )
            })
            .await
            .ok()
            .flatten();
        if let Some(head) = head {
            let _ = open_annotation_workbench(core, &head).await;
            projected += 1;
        } else {
            let job_id = outcome.job_id.clone();
            let campaign_id = outcome.campaign_id.clone();
            let _ = core
                .storage()
                .database()
                .run_transaction(move |conn| {
                    crate::session::annotation_projection::mark_job_projected(conn, &job_id, None)?;
                    if let Some(campaign_id) = campaign_id {
                        crate::session::annotation_projection::refresh_campaign_coverage(
                            conn,
                            &campaign_id,
                        )?;
                    }
                    Ok(())
                })
                .await;
        }
    }
    Ok(projected)
}

async fn open_annotation_workbench(
    core: &CoreRuntime,
    head: &crate::session::annotation_projection::ProjectedHead,
) -> Result<()> {
    let mut trace = match core.data().get_trace(head.trace_digest.clone()).await {
        Ok(trace) => trace,
        Err(_) => core.data().get_trace(head.trace_id.clone()).await?,
    };
    if trace.run_id.is_none() {
        trace.run_id = head.eval_run_id.clone();
    }
    let visual = crate::presentation::ensure_annotation_workbench(
        core,
        crate::presentation::AnnotationWorkbenchRequest {
            trace,
            evidence_digest: head.digest.clone(),
            rubric_digest: head.rubric_digest.clone(),
            campaign_id: head.campaign_id.clone(),
            title: None,
            session_id: head.session_id.clone(),
        },
    )
    .await?;
    let (_shown, event) = core
        .visuals()
        .show(visual.id.clone(), head.session_id.clone())
        .await?;
    if let Ok(event) = serde_json::from_value(event) {
        core.broadcast_committed(Some(event));
    }
    Ok(())
}

async fn project_imported_trace_head(
    core: &CoreRuntime,
    container_id: &str,
    base: &str,
    trace_id: &str,
    session_id: Option<&str>,
    annotations: &Value,
) -> Result<()> {
    let head_payload = forward(
        "GET",
        &format!("{base}/traces/{trace_id}/evidence-head"),
        None,
    )
    .await?;
    let head = head_payload.get("head").unwrap_or(&head_payload);
    let trace_digest = head
        .get("trace_digest")
        .or_else(|| head.get("traceDigest"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .context("sealed evidence head has no trace_digest")?
        .to_string();
    let evidence_digest = head
        .get("bundle_digest")
        .or_else(|| head.get("bundleDigest"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .context("sealed evidence head has no bundle_digest")?;
    let identity = format!("{container_id}\0{trace_id}\0{evidence_digest}");
    let campaign_id = format!("acmp_import_{:x}", Sha256::digest(identity.as_bytes()));
    let campaign_id = campaign_id[.."acmp_import_".len() + 16].to_string();
    let bundles = json!({ "bundles": [head.clone()] });
    let session_id = session_id.map(str::to_owned);
    let projected = core
        .storage()
        .database()
        .run_transaction({
            let campaign_id = campaign_id.clone();
            let container_id = container_id.to_string();
            let trace_id = trace_id.to_string();
            let trace_digest = trace_digest.clone();
            let annotations = annotations.clone();
            move |conn| {
                crate::session::annotation_projection::ensure_import_campaign(
                    conn,
                    &campaign_id,
                    &container_id,
                    &trace_id,
                    session_id.as_deref(),
                )?;
                crate::session::annotation_projection::project_trace_head(
                    conn,
                    &campaign_id,
                    &trace_id,
                    &trace_digest,
                    &annotations,
                    &bundles,
                )
            }
        })
        .await?;
    if let Some(projected) = projected {
        open_annotation_workbench(core, &projected).await?;
    }
    Ok(())
}

pub(crate) async fn dispatch_annotations(
    method: &str,
    path: &str,
    body: Value,
    core: &CoreRuntime,
    app: &AppHandle,
) -> Result<Value> {
    anyhow::ensure!(method == "POST", "annotation IPC accepts POST only");
    let operation = operation_from_path(path)?;
    validate_arguments(&body)?;
    let container_id = string_field(&body, "container_id", "containerId").ok_or_else(|| {
        failure(
            "annotation_argument_missing",
            "container_id required",
            "pass the immutable container id that sealed the trace (container_list)",
        )
    })?;
    let base = container_base(core, &container_id).await?;
    let _ = pull_reconciliations(core, &container_id, &base).await;
    let session_id = string_field(&body, "session_id", "sessionRef");
    if !matches!(
        operation,
        "annotation_start" | "verification_start" | "annotation_campaign"
    ) {
        return dispatch_free(operation, &body, core, &container_id, &base).await;
    }
    match operation {
        "annotation_start" | "verification_start" => {
            let trace_id = trace_id_of(&body)?;
            let mut request = body.get("request").cloned().unwrap_or(json!({}));
            if operation == "verification_start" {
                if let Some(object) = request.as_object_mut() {
                    object.insert("mode".into(), json!("verify"));
                }
            }
            let estimate = forward(
                "POST",
                &format!("{base}/traces/{trace_id}/annotation-estimates"),
                Some(&json!({"request": request})),
            )
            .await?;
            let paid = estimate
                .get("paid")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let cached = estimate
                .get("cached")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let mut forwarded = Map::new();
            forwarded.insert("request".into(), request.clone());
            if let Some(session) = &session_id {
                forwarded.insert("session_id".into(), json!(session));
            }
            let mut issued: Option<reservations::IssuedReservation> = None;
            if paid && !cached {
                let preflight = paid_preflight(
                    core,
                    &container_id,
                    session_id.as_deref(),
                    &request,
                    &estimate,
                )
                .await?;
                let session = preflight.binding.session_id.clone();
                let binding = preflight.binding;
                let grant = approve(app, &session, operation, json!({"traceId": trace_id, "traceRowId": preflight.trace_row_id, "traceDigest": binding.trace_digest, "annotatorId": binding.annotator_id, "containerId": preflight.container_id, "model": preflight.model, "estimate": estimate}), preflight.cap_usd_micros, Some(&preflight.model), &binding.digest()).await?;
                let reservation =
                    issue_reservation(core, &container_id, &session, &grant, binding).await?;
                forwarded.insert("reservation_id".into(), json!(reservation.token));
                issued = Some(reservation);
            }
            let route = if operation == "verification_start" {
                "verification-jobs"
            } else {
                "annotation-jobs"
            };
            match forward(
                "POST",
                &format!("{base}/traces/{trace_id}/{route}"),
                Some(&Value::Object(forwarded)),
            )
            .await
            {
                Ok(payload) => {
                    if let Some(reservation) = issued {
                        if let Some(job_id) = payload
                            .get("job")
                            .and_then(|job| job.get("job_id"))
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                        {
                            let id = reservation.reservation_id.clone();
                            core.storage()
                                .database()
                                .run_transaction(move |conn| {
                                    reservations::mark_forwarded(conn, &id, &job_id)
                                })
                                .await?;
                        } else {
                            release_reservation(core, reservation.reservation_id).await;
                        }
                    }
                    Ok(payload)
                }
                Err(error) => {
                    if let Some(reservation) = issued {
                        release_reservation(core, reservation.reservation_id).await;
                    }
                    Err(error)
                }
            }
        }
        "annotation_campaign" => {
            let mut plan = json!({"traces": body.get("traces").cloned().unwrap_or(Value::Null), "annotators": body.get("annotators").cloned().unwrap_or(json!([])), "label": body.get("label").cloned().unwrap_or(json!("")), "session_id": session_id});
            let estimate_only = body
                .get("estimate_only")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if estimate_only {
                plan["estimate_only"] = json!(true);
                return forward("POST", &format!("{base}/annotation/campaigns"), Some(&plan)).await;
            }
            plan["estimate_only"] = json!(true);
            let estimate =
                forward("POST", &format!("{base}/annotation/campaigns"), Some(&plan)).await?;
            let estimate = estimate.get("estimate").cloned().unwrap_or(estimate);
            let paid_jobs = estimate
                .get("paid_jobs")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            plan["estimate_only"] = json!(false);
            let mut issued: Vec<(String, String)> = Vec::new();
            if !paid_jobs.is_empty() {
                let session = session_id.clone().ok_or_else(|| {
                    failure(
                        "annotation_session_required",
                        "paid campaign needs a session",
                        "call from an agent session",
                    )
                })?;
                let total = micros_of(estimate.get("max_cost_usd")).ok_or_else(|| {
                    failure(
                        "annotation_cap_unbounded",
                        "campaign estimate has no max_cost_usd",
                        "declare limits.max_cost_usd on paid annotators",
                    )
                })?;
                let mut specs = Vec::with_capacity(paid_jobs.len());
                let mut declared_total = 0_u64;
                for job in &paid_jobs {
                    let cap = micros_of(job.get("max_cost_usd")).ok_or_else(|| {
                        failure(
                            "annotation_cap_unbounded",
                            "campaign contains a paid job without max_cost_usd",
                            "declare limits.max_cost_usd on every paid annotator",
                        )
                    })?;
                    let trace_digest = job
                        .get("trace_digest")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .ok_or_else(|| {
                            failure(
                                "annotation_binding_incomplete",
                                "campaign paid job has no trace digest",
                                "re-estimate against sealed Trace V5 inputs",
                            )
                        })?;
                    let annotator_id = job
                        .get("annotator_id")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .ok_or_else(|| {
                            failure(
                                "annotation_binding_incomplete",
                                "campaign paid job has no annotator id",
                                "use registered annotators",
                            )
                        })?;
                    let model = job
                        .get("model")
                        .and_then(Value::as_str)
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .ok_or_else(|| {
                            failure(
                                "annotation_binding_incomplete",
                                "campaign paid job has no resolved model",
                                "configure an annotation runner model",
                            )
                        })?;
                    declared_total = declared_total.checked_add(cap).ok_or_else(|| {
                        failure(
                            "annotation_cap_unbounded",
                            "campaign cost cap overflowed",
                            "reduce the campaign size",
                        )
                    })?;
                    let binding = ReservationBinding {
                        trace_digest: trace_digest.to_string(),
                        annotator_id: annotator_id.to_string(),
                        model: model.to_string(),
                        session_id: session.clone(),
                    };
                    let key = format!(
                        "{}|{}|{}",
                        binding.trace_digest,
                        binding.annotator_id,
                        job.get("repeat_index").and_then(Value::as_u64).unwrap_or(0)
                    );
                    specs.push((key, binding, cap));
                }
                if declared_total != total {
                    return Err(failure("annotation_estimate_inconsistent", format!("campaign total {total} does not equal its paid-job caps {declared_total}"), "refresh the container and estimate the campaign again"));
                }
                let grant = approve(app, &session, operation, json!({"containerId": container_id, "jobs": paid_jobs.len(), "estimate": estimate}), total, None, "campaign").await?;
                if grant.cap_usd_micros < declared_total {
                    let approval_id = grant.approval_id.clone();
                    core.storage()
                        .database()
                        .run_transaction(move |conn| {
                            crate::session::paid_compute_budget::release_reservation(
                                conn,
                                &approval_id,
                            )
                        })
                        .await?;
                    return Err(failure(
                        "annotation_campaign_cap_reduced",
                        "the approved campaign cap is below the sum of its job caps",
                        "approve the full bounded campaign or submit a smaller campaign",
                    ));
                }
                let mut tokens = Map::new();
                for (key, binding, cap) in specs {
                    match issue_reservation(
                        core,
                        &container_id,
                        &session,
                        &PaidGrant {
                            approval_id: grant.approval_id.clone(),
                            cap_usd_micros: cap,
                        },
                        binding,
                    )
                    .await
                    {
                        Ok(reservation) => {
                            issued.push((key.clone(), reservation.reservation_id.clone()));
                            tokens.insert(key, json!(reservation.token));
                        }
                        Err(error) => {
                            for (_, id) in issued.drain(..) {
                                release_reservation(core, id).await;
                            }
                            let approval_id = grant.approval_id.clone();
                            let _ = core
                                .storage()
                                .database()
                                .run_transaction(move |conn| {
                                    crate::session::paid_compute_budget::release_reservation(
                                        conn,
                                        &approval_id,
                                    )
                                })
                                .await;
                            return Err(error);
                        }
                    }
                }
                plan["reservations"] = Value::Object(tokens);
            }
            match forward("POST", &format!("{base}/annotation/campaigns"), Some(&plan)).await {
                Ok(payload) => {
                    // Match by the stable reservation key, never response order: cached or
                    // refused jobs may be omitted or reordered by the container.
                    let bindings: std::collections::HashMap<String, String> = payload
                        .get("job_bindings")
                        .and_then(Value::as_array)
                        .map(|items| {
                            items
                                .iter()
                                .filter_map(|item| {
                                    Some((
                                        item.get("key")?.as_str()?.to_string(),
                                        item.get("job_id")?.as_str()?.to_string(),
                                    ))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    let ids = issued.clone();
                    core.storage()
                        .database()
                        .run_transaction(move |conn| {
                            for (key, id) in &ids {
                                match bindings.get(key) {
                                    Some(job_id) => reservations::mark_forwarded(conn, id, job_id)?,
                                    None => reservations::release(conn, id)?,
                                }
                            }
                            Ok(())
                        })
                        .await?;
                    Ok(payload)
                }
                Err(error) => {
                    for (_, id) in issued {
                        release_reservation(core, id).await;
                    }
                    Err(error)
                }
            }
        }
        other => dispatch_free(other, &body, core, &container_id, &base).await,
    }
}

/// Operations that never spend: proxied by identity, settled on terminal reads.
pub(crate) async fn dispatch_free(
    operation: &str,
    body: &Value,
    core: &CoreRuntime,
    container_id: &str,
    base: &str,
) -> Result<Value> {
    match operation {
        "annotation_list_definitions" => {
            let trace_id = trace_id_of(body)?;
            let query = query_string(body.get("domain").map(|d| json!({"domain": d})).as_ref());
            forward(
                "GET",
                &format!("{base}/traces/{trace_id}/annotation-definitions{query}"),
                None,
            )
            .await
        }
        "annotation_estimate" => {
            let trace_id = trace_id_of(body)?;
            forward(
                "POST",
                &format!("{base}/traces/{trace_id}/annotation-estimates"),
                Some(&json!({"request": body.get("request").cloned().unwrap_or(Value::Null)})),
            )
            .await
        }
        "annotation_get" | "verification_get" => {
            let job_id = string_field(body, "job_id", "jobId").ok_or_else(|| {
                failure(
                    "annotation_argument_missing",
                    "job_id required",
                    "pass the job id",
                )
            })?;
            let payload = forward("GET", &format!("{base}/annotation-jobs/{job_id}"), None).await?;
            settle_if_terminal(core, container_id, &payload).await?;
            Ok(payload)
        }
        "annotation_cancel" => {
            let job_id = string_field(body, "job_id", "jobId").ok_or_else(|| {
                failure(
                    "annotation_argument_missing",
                    "job_id required",
                    "pass the job id",
                )
            })?;
            let payload = forward(
                "POST",
                &format!("{base}/annotation-jobs/{job_id}/cancel"),
                Some(&json!({})),
            )
            .await?;
            settle_if_terminal(core, container_id, &payload).await?;
            Ok(payload)
        }
        "annotation_list" => {
            let trace_id = trace_id_of(body)?;
            let payload = forward(
                "GET",
                &format!(
                    "{base}/traces/{trace_id}/annotations{}",
                    query_string(body.get("filters"))
                ),
                None,
            )
            .await?;
            // A sealed head is useful even when the annotations were created
            // by a container-native post-rollout stage rather than a Workshop
            // campaign. Import it into the same durable projection used by
            // first-class annotation workbenches. The read itself remains
            // successful if the trace has not yet been imported locally.
            if payload
                .get("bundle_digest")
                .or_else(|| payload.get("bundleDigest"))
                .and_then(Value::as_str)
                .is_some()
            {
                let session_id = string_field(body, "session_id", "sessionRef");
                let _ = project_imported_trace_head(
                    core,
                    container_id,
                    base,
                    &trace_id,
                    session_id.as_deref(),
                    &payload,
                )
                .await;
            }
            Ok(payload)
        }
        "annotation_get_evidence" => {
            let annotation_id =
                string_field(body, "annotation_id", "annotationId").ok_or_else(|| {
                    failure(
                        "annotation_argument_missing",
                        "annotation_id required",
                        "pass the annotation id",
                    )
                })?;
            forward("GET", &format!("{base}/annotations/{annotation_id}"), None).await
        }
        "annotation_review" => {
            let annotation_id =
                string_field(body, "annotation_id", "annotationId").ok_or_else(|| {
                    failure(
                        "annotation_argument_missing",
                        "annotation_id required",
                        "pass the annotation id",
                    )
                })?;
            forward("POST", &format!("{base}/annotations/{annotation_id}/reviews"), Some(&json!({"decision": body.get("decision"), "reviewer": body.get("reviewer").cloned().unwrap_or(json!("workshop")), "rationale": body.get("rationale").cloned().unwrap_or(json!("")), "evidence": body.get("evidence").cloned().unwrap_or(json!([]))}))).await
        }
        "annotation_consensus" => {
            let trace_id = trace_id_of(body)?;
            forward("POST", &format!("{base}/traces/{trace_id}/annotation-consensus"), Some(&json!({"annotator_id": body.get("annotator_id"), "majority_threshold": body.get("majority_threshold").cloned().unwrap_or(json!(0.5))}))).await
        }
        other => Err(anyhow!("unhandled annotation operation {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Minimal loopback container: health/info for registration plus the annotation router shape.
    async fn fake_container(
        paid: bool,
    ) -> (String, Arc<AtomicUsize>, Arc<std::sync::Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let jobs = Arc::new(AtomicUsize::new(0));
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let (jobs_c, seen_c) = (jobs.clone(), seen.clone());
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    return;
                };
                let jobs = jobs_c.clone();
                let seen = seen_c.clone();
                tokio::spawn(async move {
                    let mut raw = Vec::new();
                    let mut buf = [0u8; 4096];
                    let text = loop {
                        let n = stream.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        raw.extend_from_slice(&buf[..n]);
                        let text = String::from_utf8_lossy(&raw).to_string();
                        if let Some((head, body)) = text.split_once("\r\n\r\n") {
                            let expected = head
                                .lines()
                                .find_map(|l| {
                                    l.to_ascii_lowercase()
                                        .strip_prefix("content-length:")
                                        .map(|v| v.trim().parse::<usize>().unwrap_or(0))
                                })
                                .unwrap_or(0);
                            if body.len() >= expected {
                                break text;
                            }
                        }
                    };
                    let line = text.lines().next().unwrap_or_default().to_string();
                    let mut parts = line.split_whitespace();
                    let (method, path) = (
                        parts.next().unwrap_or_default().to_string(),
                        parts.next().unwrap_or_default().to_string(),
                    );
                    seen.lock().unwrap().push(format!("{method} {path}"));
                    let (status, payload) = match (method.as_str(), path.as_str()) {
                        ("GET", "/health") => (200, json!({"ok": true, "status": "ok"})),
                        ("GET", "/info") => (200, json!({"capabilities": {}})),
                        (_, p) if p.ends_with("/annotation-estimates") => (
                            200,
                            json!({"idempotency_key": "k", "cached": false, "paid": paid, "max_cost_usd": 0.25, "resolved_model": "gpt-5.6-luna", "runner_kind": "codex_app_server"}),
                        ),
                        (_, p) if p.ends_with("/annotation-jobs") => {
                            jobs.fetch_add(1, Ordering::SeqCst);
                            (
                                202,
                                json!({"job": {"job_id": "ajob_1", "state": "prepared"}, "accepted": true}),
                            )
                        }
                        (_, p) if p.starts_with("/annotation-jobs/") => (
                            200,
                            json!({"job": {"job_id": "ajob_1", "state": "sealed", "usage": {"cost_usd": 0.004}}, "terminal": true}),
                        ),
                        (_, p) if p.contains("/annotations?") || p.ends_with("/annotations") => {
                            (200, json!({"count": 0, "annotations": []}))
                        }
                        _ => (
                            404,
                            json!({"detail": {"code": "job_not_found", "message": "nope"}}),
                        ),
                    };
                    let body = payload.to_string();
                    let response = format!("HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                    let _ = stream.write_all(response.as_bytes()).await;
                });
            }
        });
        (format!("http://{addr}"), jobs, seen)
    }

    async fn registered(core: &CoreRuntime, base: &str) -> String {
        let value = crate::visuals_ipc::dispatch(
            "POST",
            "/v1/containers",
            json!({"baseUrl": base, "name": "fake", "location": "local"}),
            core,
        )
        .await
        .expect("register");
        value["container"]["id"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn free_operations_proxy_by_identity_and_settle_terminal_jobs() {
        let (base, jobs, seen) = fake_container(false).await;
        let dir = tempfile::tempdir().unwrap();
        let core = CoreRuntime::open(dir.path()).unwrap();
        let container_id = registered(&core, &base).await;
        let listed = dispatch_free(
            "annotation_list",
            &json!({"trace_id": "trace_x", "filters": {"label": "belief.contradicted"}}),
            &core,
            &container_id,
            &base,
        )
        .await
        .unwrap();
        assert_eq!(listed["count"], 0);
        assert!(
            seen.lock()
                .unwrap()
                .iter()
                .any(|line| line == "GET /traces/trace_x/annotations?label=belief.contradicted"),
            "{:?}",
            seen.lock().unwrap()
        );
        // a forwarded reservation settles when the job is read back terminal
        let cid = container_id.clone();
        core.storage()
            .database()
            .run_transaction(move |conn| {
                reservations::store_broker_secret(conn, &cid, "s")?;
                let issued = reservations::issue(
                    conn,
                    "s",
                    &cid,
                    "sess",
                    "approval-x",
                    &ReservationBinding {
                        trace_digest: "sha256:aa".into(),
                        annotator_id: "a".into(),
                        model: "m".into(),
                        session_id: "sess".into(),
                    },
                    250_000,
                    "t",
                    600,
                )?;
                reservations::mark_forwarded(conn, &issued.reservation_id, "ajob_1")
            })
            .await
            .unwrap();
        let job = dispatch_free(
            "annotation_get",
            &json!({"job_id": "ajob_1"}),
            &core,
            &container_id,
            &base,
        )
        .await
        .unwrap();
        assert_eq!(job["terminal"], true);
        let cid = container_id.clone();
        let settled = core
            .storage()
            .database()
            .run_read(move |conn| Ok(reservations::by_job(conn, &cid, "ajob_1")?.is_none()))
            .await
            .unwrap();
        assert!(
            settled,
            "forwarded reservation must be settled after a terminal read"
        );
        assert_eq!(jobs.load(Ordering::SeqCst), 0);
        let missing = dispatch_free(
            "annotation_get_evidence",
            &json!({"annotation_id": "ann_nope"}),
            &core,
            &container_id,
            &base,
        )
        .await
        .unwrap_err();
        assert_eq!(
            missing.downcast_ref::<StructuredFailure>().unwrap().code,
            "job_not_found"
        );
    }

    #[tokio::test]
    async fn paid_preflight_refuses_without_session_cap_or_launch_secret() {
        let dir = tempfile::tempdir().unwrap();
        let core = CoreRuntime::open(dir.path()).unwrap();
        let digest = format!("sha256:{}", "a".repeat(64));
        let request = json!({"source_trace_digest": digest, "annotator_id": "craftax.belief"});
        let estimate = json!({"paid": true, "cached": false, "max_cost_usd": 0.25, "resolved_model": "gpt-5.6-luna"});
        let err = paid_preflight(&core, "ctr", None, &request, &estimate)
            .await
            .unwrap_err();
        assert_eq!(
            err.downcast_ref::<StructuredFailure>().unwrap().code,
            "annotation_session_required"
        );
        let err = paid_preflight(
            &core,
            "ctr",
            Some("sess"),
            &request,
            &json!({"paid": true, "resolved_model": "m"}),
        )
        .await
        .unwrap_err();
        assert_eq!(
            err.downcast_ref::<StructuredFailure>().unwrap().code,
            "annotation_cap_unbounded"
        );
        let err = paid_preflight(&core, "ctr", Some("sess"), &request, &estimate)
            .await
            .unwrap_err();
        assert_eq!(
            err.downcast_ref::<StructuredFailure>().unwrap().code,
            "reservation_broker_unavailable"
        );
        core.storage()
            .database()
            .run_transaction(|conn| reservations::store_broker_secret(conn, "ctr", "s"))
            .await
            .unwrap();
        // A launched container is not enough: the card must be able to name
        // who sealed the trace, which only Workshop's own trace index knows.
        let err = paid_preflight(&core, "ctr", Some("sess"), &request, &estimate)
            .await
            .unwrap_err();
        assert_eq!(
            err.downcast_ref::<StructuredFailure>().unwrap().code,
            "annotation_trace_unknown"
        );
        let seeded = digest.clone();
        core.storage()
            .database()
            .run_transaction(move |conn| {
                for id in ["ctr", "other"] {
                    conn.execute("INSERT INTO containers(id,name,location,status,health_json,metadata_json,created_at,updated_at) VALUES(?1,'Local','local','ready','{}','{}','2026-01-01','2026-01-01')", rusqlite::params![id])?;
                }
                conn.execute(
                    "INSERT INTO traces(id,digest,title,source,metrics_json,metadata_json,created_at) VALUES('tracev5_aaaa',?1,'Trace','import','[]','{}','2026-01-03')",
                    rusqlite::params![seeded],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        // Bare file import: present, but nobody sealed it as far as Workshop knows.
        let err = paid_preflight(&core, "ctr", Some("sess"), &request, &estimate)
            .await
            .unwrap_err();
        let failure = err.downcast_ref::<StructuredFailure>().unwrap();
        assert_eq!(failure.code, "annotation_trace_unowned");
        assert!(
            failure.message.contains("no owning container"),
            "{}",
            failure.message
        );
        assert!(failure.message.contains("tracev5_aaaa"), "{}", failure.message);
        let set_owner = |owner: &'static str| {
            let digest = digest.clone();
            let core = &core;
            async move {
                core.storage()
                    .database()
                    .run_transaction(move |conn| {
                        conn.execute(
                            "UPDATE traces SET container_id=?1 WHERE digest=?2",
                            rusqlite::params![owner, digest],
                        )?;
                        Ok(())
                    })
                    .await
                    .unwrap();
            }
        };
        set_owner("other").await;
        let err = paid_preflight(&core, "ctr", Some("sess"), &request, &estimate)
            .await
            .unwrap_err();
        assert_eq!(
            err.downcast_ref::<StructuredFailure>().unwrap().code,
            "annotation_container_mismatch"
        );
        set_owner("ctr").await;
        let ok = paid_preflight(&core, "ctr", Some("sess"), &request, &estimate)
            .await
            .unwrap();
        assert_eq!(ok.cap_usd_micros, 250_000);
        assert_eq!(ok.binding.session_id, "sess");
        assert_eq!(ok.container_id, "ctr");
        assert_eq!(ok.trace_row_id, "tracev5_aaaa");
    }

    #[test]
    fn unknown_container_is_refused_before_any_hop() {
        let dir = tempfile::tempdir().unwrap();
        let core = CoreRuntime::open(dir.path()).unwrap();
        let rt = tokio::runtime::Runtime::new().unwrap();
        let err = rt
            .block_on(container_base(&core, "ctr_missing"))
            .unwrap_err();
        assert_eq!(
            err.downcast_ref::<StructuredFailure>().unwrap().code,
            "annotation_container_unknown"
        );
    }

    #[test]
    fn operations_match_the_shim_catalog() {
        let shim = include_str!("bin/synth_annotations_mcp.rs");
        for (name, _, _) in OPERATIONS {
            assert!(
                shim.contains(&format!("(\"{name}\",")),
                "{name} missing from shim"
            );
        }
        let catalogued = shim
            .lines()
            .map(str::trim)
            .filter(|line| {
                line.starts_with("(\"annotation_") || line.starts_with("(\"verification_")
            })
            .filter(|line| line.contains(", true, ") || line.contains(", false, "))
            .count();
        assert_eq!(
            catalogued,
            OPERATIONS.len(),
            "shim OPERATIONS table drifted from the host"
        );
    }

    #[test]
    fn arguments_and_operations_are_validated_before_any_hop() {
        assert!(operation_from_path("/v1/annotations/delete").is_err());
        assert_eq!(
            operation_from_path("/v1/annotations/annotation_get").unwrap(),
            "annotation_get"
        );
        assert!(validate_arguments(&json!({"trace_id": "t", "path": "/etc/passwd"})).is_err());
        assert!(
            validate_arguments(&json!({"trace_id": "t", "reservation_id": {"cap": 1}})).is_err()
        );
        assert!(validate_arguments(
            &json!({"trace_id": "t", "container_id": "c", "reservation_id": "rsv"})
        )
        .is_ok());
        assert_eq!(
            query_string(Some(&json!({"label": "a b", "include_superseded": true}))),
            "?include_superseded=true&label=a%20b"
        );
    }
}
