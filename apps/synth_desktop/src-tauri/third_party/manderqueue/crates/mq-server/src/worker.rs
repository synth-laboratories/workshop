//! Delivery worker: claim, recheck live grant authority, dispatch, settle.
//!
//! Library form of `mq-server worker` so the loop is testable. A job whose
//! recipient is grant-governed is never sent to the bridge unless the grant is
//! live at dispatch time (revoked, expired, signed-out and membership-revoked
//! grants are dead-lettered without a request). See docs/DELIVERY_SECURITY.md
//! and docs/WORKSHOP_GRANT_CONTRACT.md §6.

use std::time::Duration;

use mq_core::{DeliveryGrant, DeliveryJob, DeliveryStatus, Fabric, Grant, Message};
use serde_json::{json, Value};

use crate::delivery::{delivery_token, matching_bridge_outcome, DELIVERY_PATH};

#[derive(Clone)]
pub struct WorkerConfig {
    bridge_origin: String,
    secret: String,
    pub max_attempts: u32,
}

impl std::fmt::Debug for WorkerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WorkerConfig")
            .field("bridge_origin", &self.bridge_origin)
            .field("max_attempts", &self.max_attempts)
            .finish_non_exhaustive()
    }
}

impl WorkerConfig {
    /// Refuse missing or unsafe delivery wiring before claiming any work.
    pub fn new(bridge: &str, secret: &str, max_attempts: u32) -> Result<Self, String> {
        let url = reqwest::Url::parse(bridge).map_err(|_| "worker requires a valid MQ_BRIDGE_BASE_URL".to_string())?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err("HTTP(S) bridge required".into());
        }
        if url.query().is_some() || url.fragment().is_some() || url.path() != "/" {
            return Err("bridge URL must be an origin".into());
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err("bridge URL must not contain credentials".into());
        }
        delivery_token(b"{}", secret, chrono::Utc::now().timestamp())?;
        Ok(Self {
            bridge_origin: url.origin().ascii_serialization(),
            secret: secret.to_string(),
            max_attempts: max_attempts.max(1),
        })
    }

    pub fn bridge_origin(&self) -> &str {
        &self.bridge_origin
    }
}

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("delivery HTTP client")
}

/// Signed-body envelope. `grant` is present only for grant-governed recipients.
pub fn envelope(job: &DeliveryJob, message: &Message, grant: Option<&Grant>) -> Value {
    let mut body = json!({
        "job_id": job.job_id.0,
        "message_id": job.message_id.0,
        "thread_id": job.thread_id.0,
        "recipient": job.recipient,
        "attempts": job.attempts,
        "message": {
            "seq": message.seq,
            "kind": message.kind,
            "body": message.body,
            "payload": message.payload,
            "sender": message.sender,
            "idempotency_key": message.idempotency_key,
            "correlation_id": message.correlation_id,
            "parent_message_id": message.parent_message_id.map(|m| m.0),
            "causation_id": message.causation_id,
            "created_at": message.created_at,
        }
    });
    if let Some(grant) = grant {
        // Lets the bridge and native acceptance fence on the exact grant authority.
        body["grant"] = json!({
            "grant_id": grant.grant_id,
            "generation": grant.generation,
            "incarnation": grant.incarnation,
        });
    }
    body
}

/// Result of handling one claimed job. `dispatched == false` means no bridge
/// request was made for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobOutcome {
    pub job: DeliveryJob,
    pub dispatched: bool,
    /// Status the worker tried to settle (the settle itself may be refused as stale).
    pub settle: DeliveryStatus,
    pub reason: &'static str,
}

async fn settle(fabric: &Fabric, job: &DeliveryJob, status: DeliveryStatus) {
    if let Err(error) = fabric.settle_delivery_job(job.job_id, job.attempts, status).await {
        eprintln!("settle failed for job {}: {error}", job.job_id.0);
    }
}

fn outcome(job: DeliveryJob, dispatched: bool, settle: DeliveryStatus, reason: &'static str) -> JobOutcome {
    JobOutcome { job, dispatched, settle, reason }
}

/// Handle one already-claimed job end to end.
pub async fn dispatch_job(
    fabric: &Fabric,
    http: &reqwest::Client,
    config: &WorkerConfig,
    job: DeliveryJob,
) -> JobOutcome {
    let message = match fabric.get_message(job.message_id).await {
        Ok(Some(message)) => message,
        Ok(None) | Err(_) => {
            // Retry later; never settle delivered without the message.
            settle(fabric, &job, DeliveryStatus::Pending).await;
            return outcome(job, false, DeliveryStatus::Pending, "message_unavailable");
        }
    };
    // Live grant authority is rechecked immediately before any request.
    let grant = match fabric.delivery_grant(&job, message.seq).await {
        Ok(DeliveryGrant::NotGoverned) => None,
        Ok(DeliveryGrant::Allowed(grant)) => Some(grant),
        Ok(DeliveryGrant::Denied) => {
            settle(fabric, &job, DeliveryStatus::DeadLetter).await;
            return outcome(job, false, DeliveryStatus::DeadLetter, "grant_denied");
        }
        Err(error) => {
            eprintln!("delivery grant check failed for job {}: {error}", job.job_id.0);
            settle(fabric, &job, DeliveryStatus::Pending).await;
            return outcome(job, false, DeliveryStatus::Pending, "grant_check_failed");
        }
    };
    let body = envelope(&job, &message, grant.as_ref());
    let bytes = serde_json::to_vec(&body).expect("JSON envelope");
    let token = delivery_token(&bytes, &config.secret, chrono::Utc::now().timestamp()).expect("delivery signature");
    let url = format!("{}{}", config.bridge_origin, DELIVERY_PATH);
    let bridged = match http
        .post(&url)
        .bearer_auth(token)
        .header("content-type", "application/json")
        .body(bytes)
        .send()
        .await
    {
        Ok(response) if response.status().is_success() => match response.json::<Value>().await {
            Ok(receipt) => matching_bridge_outcome(&receipt, &body),
            Err(error) => {
                eprintln!("invalid bridge receipt: {error}");
                None
            }
        },
        Ok(response) => {
            eprintln!("bridge HTTP {} for job {}", response.status(), job.job_id.0);
            None
        }
        Err(error) => {
            eprintln!("bridge error for job {}: {error}", job.job_id.0);
            None
        }
    };
    let (status, reason) = match bridged {
        Some(status) => (status, "bridge_receipt"),
        None if job.attempts >= config.max_attempts => (DeliveryStatus::DeadLetter, "attempts_exhausted"),
        None => (DeliveryStatus::Pending, "retry"),
    };
    settle(fabric, &job, status).await;
    outcome(job, true, status, reason)
}

/// Claim up to `limit` due jobs and handle each.
pub async fn run_once(
    fabric: &Fabric,
    http: &reqwest::Client,
    config: &WorkerConfig,
    limit: usize,
) -> mq_core::Result<Vec<JobOutcome>> {
    let jobs = fabric.claim_delivery_jobs(limit).await?;
    let mut out = Vec::with_capacity(jobs.len());
    for job in jobs {
        out.push(dispatch_job(fabric, http, config, job).await);
    }
    Ok(out)
}
