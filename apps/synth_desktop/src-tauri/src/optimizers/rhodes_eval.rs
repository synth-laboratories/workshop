//! Read-only Rhodes evaluation mirror. Backend acceptance owns execution;
//! Workshop owns its local delivery cursor and projection, never a second job.
use super::{cloud::CloudOptimizerClient, events::OptimizerEventDraft};
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RhodesEvalAttachRequest {
    pub pool_id: String,
    pub rollout_id: String,
    #[serde(default)]
    pub open_visual: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct RhodesEventPage {
    pub rollout_id: String,
    pub events: Vec<Value>,
    pub next_sequence: u64,
    pub has_more: bool,
    pub status: String,
    pub cleanup_pending: Option<bool>,
    pub publication_pending: Option<bool>,
    pub inference_pending: Option<bool>,
}

pub(super) fn coordinate(value: &str) -> Result<&str> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        bail!("invalid Rhodes resource coordinate");
    }
    Ok(value)
}

pub(super) fn mirror_id(endpoint: &str, pool: &str, rollout: &str) -> String {
    let identity = serde_json::to_vec(&(endpoint, pool, rollout)).expect("string tuple JSON");
    format!("rhodes_{:x}", Sha256::digest(identity))
}

impl RhodesEventPage {
    pub(super) fn validate(&self, request: &RhodesEvalAttachRequest, after: u64) -> Result<()> {
        if self.rollout_id != request.rollout_id || self.events.len() > 200 {
            bail!("Rhodes replay identity or page bound mismatch");
        }
        let mut cursor = after;
        for event in &self.events {
            cursor = cursor.checked_add(1).context("Rhodes sequence overflow")?;
            if event.get("sequence").and_then(Value::as_u64) != Some(cursor)
                || event.get("rollout_id").and_then(Value::as_str)
                    != Some(request.rollout_id.as_str())
                || event.get("pool_id").and_then(Value::as_str) != Some(request.pool_id.as_str())
                || event.get("created_at").and_then(Value::as_str).is_none()
                || event.get("event_type").and_then(Value::as_str).is_none()
            {
                bail!("Rhodes replay gap or event identity mismatch");
            }
        }
        if self.next_sequence != cursor || (self.has_more && self.events.is_empty()) {
            bail!("Rhodes replay cursor did not match committed events");
        }
        if !matches!(
            self.status.as_str(),
            "queued" | "running" | "completed" | "failed" | "cancelled"
        ) {
            bail!("unknown Rhodes lifecycle status");
        }
        Ok(())
    }

    pub(super) fn drained(&self) -> bool {
        !self.has_more
            && self.cleanup_pending == Some(false)
            && self.publication_pending != Some(true)
            && self.inference_pending != Some(true)
            && matches!(self.status.as_str(), "completed" | "failed" | "cancelled")
    }
}

/// Preserve requested and server-admitted limits as separate observations.
/// Missing legacy evidence is null; zero is a real accepted ceiling.
pub(super) fn result_snapshot(remote: &Value) -> Value {
    json!({
        "score": remote.get("score"),
        "summary": remote.get("summary"),
        "limits": remote.get("limits"),
        "acceptedExecutionLimits": remote.pointer("/metadata/accepted_execution_limits"),
        "requiredLimitCapabilities": remote.pointer("/metadata/required_limit_capabilities"),
        "executionLimitCapabilities": remote.pointer("/metadata/execution_limit_capabilities"),
        "executionDeadlineAt": remote.get("execution_deadline_at"),
        "usage": remote.get("usage"),
        "artifacts": remote.get("artifacts"),
        "traceCorrelationId": remote.get("trace_correlation_id"),
        "resultPublication": remote.pointer("/metadata/result_publication"),
        "nativeEvidence": remote.pointer("/metadata/native_evidence"),
    })
}

/// Project only named operational facts; replay remains the evidence authority.
#[path = "rhodes_observations.rs"]
mod observations;
pub(super) use observations::critical_observations;

pub(super) fn draft(raw: Value) -> Result<OptimizerEventDraft> {
    let kind = raw
        .get("event_type")
        .and_then(Value::as_str)
        .context("Rhodes event kind")?;
    // Backend lifecycle is retained here as source evidence. The replay page
    // supplies exactly one local lifecycle receipt after this batch is ingested.
    let projection = "eval.rhodes.event";
    let timestamp = raw
        .get("created_at")
        .and_then(Value::as_str)
        .context("Rhodes timestamp")?
        .to_owned();
    // Preserve source identity/order in raw. The local store allocates its own
    // delivery sequence atomically with the source cursor, not from this number.
    Ok(OptimizerEventDraft::new(projection, "eval")
        .occurred_at(timestamp)
        .item(
            json!({"kind":"rollout", "id":raw.get("rollout_id"), "status":raw.get("status"),
            "reward":raw.get("score"), "label":kind}),
        )
        .raw(raw))
}

impl CloudOptimizerClient {
    pub(super) fn rhodes_endpoint(&self) -> &str {
        &self.base_url
    }

    pub(super) async fn rhodes_run(&self, request: &RhodesEvalAttachRequest) -> Result<Value> {
        self.rhodes_json(&format!(
            "/v1/pools/{}/rollouts/{}",
            coordinate(&request.pool_id)?,
            coordinate(&request.rollout_id)?
        ))
        .await
    }

    pub(super) async fn rhodes_page(
        &self,
        request: &RhodesEvalAttachRequest,
        after: u64,
    ) -> Result<RhodesEventPage> {
        let raw = self
            .rhodes_json(&format!(
                "/v1/pools/{}/rollouts/{}/events?after_sequence={after}&limit=200&format=json",
                coordinate(&request.pool_id)?,
                coordinate(&request.rollout_id)?
            ))
            .await?;
        let page: RhodesEventPage =
            serde_json::from_value(raw).context("decode Rhodes replay page")?;
        page.validate(request, after)?;
        Ok(page)
    }

    async fn rhodes_json(&self, path: &str) -> Result<Value> {
        let mut response = self
            .client
            .get(format!("{}{path}", self.base_url))
            .bearer_auth(&self.api_key)
            .send()
            .await
            .context("read Rhodes evaluation")?;
        if !response.status().is_success() {
            bail!("Rhodes evaluation HTTP {}", response.status());
        }
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .context("read Rhodes response chunk")?
        {
            if body.len().saturating_add(chunk.len()) > 4 * 1024 * 1024 {
                bail!("Rhodes response exceeds 4 MiB");
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body).context("decode Rhodes response")
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn admitted_limits_preserve_zero_and_server_authority() {
        let snapshot = super::result_snapshot(&serde_json::json!({
            "limits": {"timeout_s": 300},
            "accepted_execution_limits": {"timeout_s": 999},
            "metadata": {"accepted_execution_limits": {"timeout_s": 0},
                         "required_limit_capabilities": []},
            "execution_deadline_at": "2026-09-11T07:00:00Z"
        }));
        assert_eq!(snapshot["limits"]["timeout_s"], 300);
        assert_eq!(snapshot["acceptedExecutionLimits"]["timeout_s"], 0);
        assert_eq!(snapshot["requiredLimitCapabilities"], serde_json::json!([]));
        assert_eq!(snapshot["executionDeadlineAt"], "2026-09-11T07:00:00Z");
        let legacy = super::result_snapshot(&serde_json::json!({"limits": {"timeout_s": 30}}));
        assert!(legacy["acceptedExecutionLimits"].is_null());
        assert!(legacy["requiredLimitCapabilities"].is_null());
    }

    use super::*;
    fn request() -> RhodesEvalAttachRequest {
        RhodesEvalAttachRequest {
            pool_id: "pool".into(),
            rollout_id: "rollout".into(),
            open_visual: false,
        }
    }
    fn page() -> RhodesEventPage {
        RhodesEventPage {
            rollout_id: "rollout".into(),
            events: vec![
                json!({"sequence":1,"pool_id":"pool","rollout_id":"rollout","created_at":"2026-09-10T00:00:00Z","event_type":"rollout.failed"}),
            ],
            next_sequence: 1,
            has_more: false,
            status: "failed".into(),
            cleanup_pending: Some(true),
            publication_pending: None,
            inference_pending: None,
        }
    }
    #[test]
    fn cleanup_is_independent_of_scientific_terminal() {
        let mut p = page();
        p.validate(&request(), 0).unwrap();
        assert!(!p.drained());
        p.cleanup_pending = Some(false);
        assert!(p.drained());
        p.publication_pending = Some(true);
        assert!(!p.drained());
        p.publication_pending = Some(false);
        assert!(p.drained());
        p.inference_pending = Some(true);
        assert!(!p.drained());
        p.inference_pending = Some(false);
        assert!(p.drained());
        p.cleanup_pending = None;
        assert!(!p.drained());
    }
    #[test]
    fn replay_refuses_gaps_and_wrong_owner() {
        let mut p = page();
        p.events[0]["sequence"] = json!(2);
        assert!(p.validate(&request(), 0).is_err());
        let mut p = page();
        p.events[0]["pool_id"] = json!("other");
        assert!(p.validate(&request(), 0).is_err());
    }
    #[test]
    fn missing_reward_stays_null_and_source_is_preserved() {
        let raw = page().events.remove(0);
        let d = draft(raw.clone()).unwrap();
        assert_eq!(d.raw, raw);
        assert!(d.item.unwrap()["reward"].is_null());
        assert_eq!(d.event_type, "eval.rhodes.event");
    }
    #[tokio::test]
    async fn mirror_commits_local_delivery_and_source_cursor_atomically() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            for _ in 0..9 {
                let (mut connection, _) = listener.accept().unwrap();
                connection
                    .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                    .unwrap();
                let mut bytes = [0u8; 8192];
                let size = connection.read(&mut bytes).unwrap();
                let request = String::from_utf8_lossy(&bytes[..size]);
                let body = if request.contains("/events?") {
                    let events = if request.contains("after_sequence=0&") {
                        vec![
                            json!({"sequence":1,"pool_id":"pool","rollout_id":"rollout","created_at":"2026-09-10T00:00:00Z","event_type":"rollout.failed","status":"failed"}),
                        ]
                    } else if request.contains("after_sequence=1&") {
                        vec![
                            json!({"sequence":2,"pool_id":"pool","rollout_id":"rollout","created_at":"2026-09-10T00:00:01Z","event_type":"rollout.cleanup_confirmed","status":"failed"}),
                        ]
                    } else {
                        vec![]
                    };
                    let first = request.contains("after_sequence=0&");
                    json!({"rollout_id":"rollout","events":events,"next_sequence":if first {1} else {2},"has_more":false,"status":"failed","cleanup_pending":first})
                } else {
                    json!({"pool_id":"pool","rollout_id":"rollout","status":"failed","score":null})
                };
                let body = body.to_string();
                write!(connection,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",body.len(),body).unwrap();
            }
        });
        let (service, _directory, _events) = super::super::service::tests::service().await;
        let client = CloudOptimizerClient::new(format!("http://{address}"), "fixture-only");
        let id = mirror_id(client.rhodes_endpoint(), "pool", "rollout");
        let first = service
            .reconcile_rhodes_page(&client, &request(), &id)
            .await
            .unwrap()
            .0;
        assert_eq!(first.summary["rhodes"]["sourceSequence"], 1);
        assert!(
            first.cursor_seq > 1,
            "local observation is a separate delivery event"
        );
        assert_eq!(first.summary["rhodes"]["drained"], false);
        assert!(first.summary["rhodes"]["resultSnapshot"]["score"].is_null());
        let second = service
            .reconcile_rhodes_page(&client, &request(), &id)
            .await
            .unwrap()
            .0;
        assert_eq!(second.summary["rhodes"]["sourceSequence"], 2);
        assert_eq!(second.summary["rhodes"]["drained"], true);
        assert!(
            second.cursor_seq > first.cursor_seq,
            "cleanup amendment must be durable"
        );
        let third = service
            .reconcile_rhodes_page(&client, &request(), &id)
            .await
            .unwrap()
            .0;
        assert_eq!(
            third.cursor_seq, second.cursor_seq,
            "empty replay must not mint events"
        );
        assert_eq!(second.status, "failed");
        server.join().unwrap();
    }
    #[tokio::test]
    #[ignore = "requires an authorized existing local Rhodes fixture; no compute submission"]
    async fn live_rhodes_fixture_mirrors_into_workshop_store() {
        let endpoint = std::env::var("EI_RHODES_FIXTURE_URL").unwrap();
        let key = std::env::var("EI_RHODES_FIXTURE_KEY").unwrap();
        let request = RhodesEvalAttachRequest {
            pool_id: std::env::var("EI_RHODES_FIXTURE_POOL").unwrap(),
            rollout_id: std::env::var("EI_RHODES_FIXTURE_ROLLOUT").unwrap(),
            open_visual: false,
        };
        assert!(
            endpoint.starts_with("http://127.0.0.1:"),
            "local qualification only"
        );
        let client = CloudOptimizerClient::new(endpoint, key);
        let id = mirror_id(
            client.rhodes_endpoint(),
            &request.pool_id,
            &request.rollout_id,
        );
        let (service, _directory, _events) = super::super::service::tests::service().await;
        let mut run = service
            .reconcile_rhodes_page(&client, &request, &id)
            .await
            .unwrap()
            .0;
        for _ in 0..10 {
            if run.summary["rhodes"]["drained"] == json!(true) {
                break;
            }
            run = service
                .reconcile_rhodes_page(&client, &request, &id)
                .await
                .unwrap()
                .0;
        }
        assert_eq!(run.summary["rhodes"]["drained"], true);
        assert!(run.cursor_seq > 0);
        let cursor = run.cursor_seq;
        let replay = service
            .reconcile_rhodes_page(&client, &request, &id)
            .await
            .unwrap()
            .0;
        assert_eq!(replay.cursor_seq, cursor);
        assert_eq!(replay.summary["rhodes"]["cleanupPending"], false);
        if let Ok(expected) = std::env::var("EI_RHODES_FIXTURE_REFUSAL_CODE") {
            assert_eq!(replay.summary["rhodes"]["lastLimitRefusal"]["error_code"], expected);
            assert_eq!(replay.summary["rhodes"]["inferencePending"], false);
        }
    }
}
