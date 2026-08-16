//! C1-08 subscribe-first helpers for loopback container rollouts.
//!
//! One umbrella: eval driver and visuals IPC poll the **declared**
//! `transports.poll.url` until `stream.subscribed` with `ready: true`, then
//! start. Never construct `/events`. Heartbeats do not count as ready.

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::time::Duration;

pub const STREAM_SUBSCRIBED_KIND: &str = "stream.subscribed";
pub const SUBSCRIBE_READY_TIMEOUT: Duration = Duration::from_secs(2);

/// Bind the stream descriptor echoed by prepare/create-rollout. Never construct
/// `/events` or `/rollouts/{id}/stream` here — those guesses are C1-09 fails.
pub fn declared_stream_descriptor(state: &Value) -> Result<Option<Value>> {
    match state.get("stream") {
        Some(stream) if !stream.is_null() => Ok(Some(stream.clone())),
        _ => Ok(None),
    }
}

/// Poll URL from the declared descriptor only. Missing → fail; never guess `/events`.
pub fn declared_poll_url(stream: &Value) -> Result<String> {
    stream
        .pointer("/transports/poll/url")
        .and_then(Value::as_str)
        .filter(|url| !url.is_empty())
        .map(str::to_string)
        .context("stream descriptor omitted transports.poll.url; refusing to guess /events")
}

pub fn declared_sse_url(stream: &Value) -> Result<String> {
    stream
        .pointer("/transports/sse/url")
        .and_then(Value::as_str)
        .filter(|url| !url.is_empty())
        .map(str::to_string)
        .context("stream descriptor omitted transports.sse.url; refusing to guess /events")
}

pub fn resolve_declared_url(base: &str, declared: &str) -> Result<String> {
    let base_url = reqwest::Url::parse(base).context("invalid container base URL")?;
    Ok(base_url
        .join(declared)
        .context("invalid declared poll URL")?
        .to_string())
}

pub fn poll_event_list(poll: &Value) -> &[Value] {
    if let Some(arr) = poll.as_array() {
        return arr;
    }
    for key in ["events", "items", "envelopes"] {
        if let Some(arr) = poll.get(key).and_then(Value::as_array) {
            return arr;
        }
    }
    &[]
}

fn event_ready_flag(event: &Value) -> bool {
    matches!(event.get("ready").and_then(Value::as_bool), Some(true))
        || matches!(
            event.pointer("/payload/ready").and_then(Value::as_bool),
            Some(true)
        )
}

pub fn is_stream_subscribed_ready(event: &Value) -> bool {
    event.get("kind").and_then(Value::as_str) == Some(STREAM_SUBSCRIBED_KIND)
        && event_ready_flag(event)
}

/// C1-08: `stream.subscribed` with `ready: true` in a poll JSON. Heartbeats do not count.
pub fn poll_has_stream_subscribed(poll: &Value) -> bool {
    poll_event_list(poll).iter().any(is_stream_subscribed_ready)
}

/// Refuse POST /rollouts until the declared poll shows a ready subscribe ACK.
pub fn require_stream_subscribed_before_start(poll: &Value) -> Result<()> {
    if poll_has_stream_subscribed(poll) {
        Ok(())
    } else {
        bail!("C1-08: refusing POST /rollouts; poll did not contain stream.subscribed with ready:true")
    }
}

/// Refuse POST /rollouts until the declared poll shows a ready subscribe ACK.
pub async fn wait_for_stream_subscribed(
    client: &reqwest::Client,
    poll_url: &str,
    timeout: Duration,
) -> Result<Value> {
    let poll_once = async {
        loop {
            let response = client
                .get(poll_url)
                .query(&[("after", "0")])
                .send()
                .await
                .context("poll declared transports.poll.url")?;
            if response.status().as_u16() == 503 {
                bail!("refusing start: declared poll returned 503");
            }
            let poll = response
                .error_for_status()
                .context("poll declared transports.poll.url")?
                .json::<Value>()
                .await?;
            if poll_has_stream_subscribed(&poll) {
                return Ok(poll);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    };
    match tokio::time::timeout(timeout, poll_once).await {
        Ok(result) => result,
        Err(_) => require_stream_subscribed_before_start(&json!({})).map(|()| json!({})),
    }
}

pub fn refuse_auto_transport(telemetry: &Value) -> Result<()> {
    let transport = telemetry
        .get("transport")
        .and_then(Value::as_str)
        .unwrap_or("");
    if transport.eq_ignore_ascii_case("auto") {
        bail!("telemetry.transport=auto is forbidden on visual/authoritative eval runs");
    }
    Ok(())
}

const ISOLATED_POLICY_HARNESS: &str = "isolated_policy_process";

/// Caller names the policy. The host does not pick luna_med or a default harness.
///
/// Isolated code policies omit `config`. Every other harness requires it.
pub fn require_caller_policy_ref(body: &Value) -> Result<Value> {
    // `policyRef` is the documented eval-driver spelling; `policy_ref` is the
    // wire spelling. Accept either — but never invent one.
    let value = body
        .get("policy_ref")
        .or_else(|| body.get("policyRef"))
        .cloned()
        .context("policy_ref { harness, config } is required; the host does not pick a recipe")?;
    if !value.is_object() {
        bail!("policy_ref must be an object");
    }
    let harness = value
        .get("harness")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if harness.is_empty() {
        bail!("policy_ref.harness is required; the host does not pick a recipe");
    }
    let config = value
        .get("config")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if harness == ISOLATED_POLICY_HARNESS {
        return Ok(value);
    }
    if config.is_empty() {
        bail!("policy_ref.config is required; the host does not default luna_med");
    }
    Ok(value)
}

/// Start must name the task instance. Seed 0 is a valid pin, not a missing-field default.
pub fn require_task_instance(body: &Value) -> Result<String> {
    if let Some(id) = body
        .get("task_instance_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|id| !id.is_empty())
    {
        return Ok(id.to_string());
    }
    if let Some(seed) = body.get("seed").and_then(Value::as_i64) {
        return Ok(format!("seed:{seed}"));
    }
    bail!("start requires task_instance_id or seed; the host does not default seed 0")
}

pub fn authoritative_poll_telemetry() -> Value {
    json!({
        "enabled": true,
        "transport": "poll",
        "detail": "standard",
        "frame": {"enabled": false}
    })
}

/// Fill the required normalized rollout telemetry envelope without overriding
/// a caller's explicit transport/detail choices. Agent tools intentionally
/// expose telemetry as optional, so the host owns these protocol defaults.
pub fn normalized_rollout_telemetry(value: Option<&Value>) -> Result<Value> {
    let mut telemetry = match value {
        Some(Value::Object(map)) => map.clone(),
        Some(_) => bail!("telemetry must be an object"),
        None => serde_json::Map::new(),
    };
    telemetry.entry("enabled").or_insert(json!(true));
    telemetry.entry("transport").or_insert(json!("sse"));
    telemetry.entry("retention").or_insert(json!("run"));
    telemetry.entry("detail").or_insert(json!("standard"));
    telemetry
        .entry("frame")
        .or_insert(json!({"enabled": true, "format": "png", "every_n_steps": 1}));
    let normalized = Value::Object(telemetry);
    refuse_auto_transport(&normalized)?;
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_telemetry_fills_protocol_defaults_without_overriding_transport() {
        let telemetry = normalized_rollout_telemetry(Some(&json!({"transport":"poll"})))
            .expect("normalize telemetry");
        assert_eq!(telemetry["enabled"], true);
        assert_eq!(telemetry["transport"], "poll");
        assert_eq!(telemetry["retention"], "run");
    }

    #[test]
    fn declared_poll_url_uses_descriptor_and_never_guesses_events() {
        let stream = json!({
            "id": "stream:r1",
            "transports": { "poll": { "url": "/rollouts/r1/events" } }
        });
        assert_eq!(declared_poll_url(&stream).unwrap(), "/rollouts/r1/events");
        let absolute = resolve_declared_url(
            "http://127.0.0.1:8098",
            &declared_poll_url(&stream).unwrap(),
        )
        .unwrap();
        assert_eq!(absolute, "http://127.0.0.1:8098/rollouts/r1/events");
        assert!(declared_poll_url(&json!({"poll_url": "/events"})).is_err());
        assert!(declared_poll_url(&json!({
            "id": "stream_r1",
            "transports": { "sse": { "url": "/rollouts/r1/stream" } }
        }))
        .is_err());
        assert_eq!(
            declared_sse_url(&json!({
                "id": "stream:r1",
                "transports": { "sse": { "url": "/rollouts/r1/stream" } }
            }))
            .unwrap(),
            "/rollouts/r1/stream"
        );
        assert!(declared_sse_url(&json!({
            "transports": { "poll": { "url": "/rollouts/r1/events" } }
        }))
        .is_err());
    }

    #[test]
    fn poll_has_stream_subscribed_detects_ready_ack() {
        let ready = json!({
            "events": [{
                "kind": "stream.subscribed",
                "control": true,
                "payload": {
                    "ready": true,
                    "stream.id": "stream:r1",
                    "rollout_id": "r1",
                    "next_sequence": 1
                }
            }]
        });
        assert!(poll_has_stream_subscribed(&ready));
        assert!(require_stream_subscribed_before_start(&ready).is_ok());

        let top_level_ready = json!({
            "items": [{ "kind": "stream.subscribed", "ready": true }]
        });
        assert!(poll_has_stream_subscribed(&top_level_ready));
    }

    #[test]
    fn require_stream_subscribed_refuses_start_when_ack_is_missing() {
        let missing = json!({ "events": [{ "kind": "heartbeat" }] });
        assert!(!poll_has_stream_subscribed(&missing));
        let err = require_stream_subscribed_before_start(&missing).unwrap_err();
        assert!(err.to_string().contains("stream.subscribed"));

        let not_ready = json!({
            "events": [{
                "kind": "stream.subscribed",
                "payload": { "ready": false }
            }]
        });
        assert!(require_stream_subscribed_before_start(&not_ready).is_err());
        assert!(require_stream_subscribed_before_start(&json!({})).is_err());
    }

    #[test]
    fn refuse_auto_transport_for_authoritative_runs() {
        assert!(refuse_auto_transport(&json!({"transport":"auto"})).is_err());
        assert!(refuse_auto_transport(&json!({"transport":"sse"})).is_ok());
    }

    #[test]
    fn require_caller_policy_ref_refuses_silent_recipe() {
        assert!(require_caller_policy_ref(&json!({})).is_err());
        assert!(require_caller_policy_ref(&json!({"policy_ref": {"config": "luna_med"}})).is_err());
        assert!(require_caller_policy_ref(&json!({"policy_ref": {"harness": "react"}})).is_err());
        let pin = require_caller_policy_ref(&json!({
            "policy_ref": {"harness": "react", "config": "caller_config"}
        }))
        .unwrap();
        assert_eq!(pin["harness"], "react");
        assert_eq!(pin["config"], "caller_config");
        let isolated = require_caller_policy_ref(&json!({
            "policy_ref": {"harness": "isolated_policy_process"}
        }))
        .unwrap();
        assert_eq!(isolated["harness"], "isolated_policy_process");
        // EVAL_DRIVER.md documents the camelCase spelling; accept it too.
        let camel = require_caller_policy_ref(&json!({
            "policyRef": {"harness": "react", "config": "luna_med"}
        }))
        .unwrap();
        assert_eq!(camel["config"], "luna_med");
    }

    #[test]
    fn require_task_instance_refuses_silent_seed_zero() {
        assert!(require_task_instance(&json!({})).is_err());
        assert_eq!(
            require_task_instance(&json!({"seed": 0})).unwrap(),
            "seed:0"
        );
        assert_eq!(
            require_task_instance(&json!({"task_instance_id": "craftax:test:2001"})).unwrap(),
            "craftax:test:2001"
        );
    }

    #[tokio::test]
    async fn wait_for_stream_subscribed_refuses_poll_503() {
        use crate::ipc::{serve_json, JsonHttpRequest, JsonHttpResponse};
        use hyper::StatusCode;

        async fn unavailable(_request: JsonHttpRequest) -> JsonHttpResponse {
            JsonHttpResponse::error(StatusCode::SERVICE_UNAVAILABLE, "visual poll unavailable")
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let _ = serve_json(listener, unavailable).await;
        });
        let client = crate::http::http_client_builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let err = wait_for_stream_subscribed(
            &client,
            &format!("http://{addr}/rollouts/r1/events"),
            Duration::from_secs(1),
        )
        .await
        .unwrap_err();
        assert!(
            err.to_string().contains("503"),
            "expected 503 refuse, got {err}"
        );
        task.abort();
    }
}
