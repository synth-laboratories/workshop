//! C1-08 subscribe-first helpers for loopback container rollouts.
//!
//! One umbrella: eval driver and visuals IPC poll the **declared**
//! `transports.poll.url` until `stream.subscribed` with `ready: true`, then
//! start. Never construct `/events`. Heartbeats do not count as ready.

use crate::diagnostics::{codes, Correlation, DiagnosticInput, DiagnosticsService, Severity};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;

pub const STREAM_SUBSCRIBED_KIND: &str = "stream.subscribed";
/// Ten concurrent Banking77 prepares can queue poll reads behind one another
/// while each stream file already contains `stream.subscribed`. A 2s host
/// deadline then fails closed with C1-08 even though the ACK exists. Give the
/// matching stream time to be observed without POSTing first.
pub const SUBSCRIBE_READY_TIMEOUT: Duration = Duration::from_secs(10);
const SUBSCRIBE_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Correlated diagnostic emitter for one rollout's stream.
///
/// The helpers below are pure functions over a client and a URL; the identities
/// that make their failures worth recording (container, rollout, stream,
/// visual, session) live with the caller. This carries both, so a subscribe
/// timeout is recorded as *this* rollout's timeout rather than an anonymous one.
///
/// [`StreamDiagnostics::none`] keeps every existing call site and test honest:
/// an uninstrumented caller emits nothing rather than reaching for a global.
#[derive(Clone, Default)]
pub struct StreamDiagnostics {
    service: Option<Arc<DiagnosticsService>>,
    correlation: Correlation,
}

impl StreamDiagnostics {
    pub fn new(service: Option<Arc<DiagnosticsService>>, correlation: Correlation) -> Self {
        Self {
            service,
            correlation,
        }
    }

    /// For tests and for callers that hold no runtime.
    pub fn none() -> Self {
        Self::default()
    }

    fn emit(
        &self,
        severity: Severity,
        event: &str,
        code: &str,
        message: impl Into<String>,
        retryable: bool,
        details: Value,
    ) {
        let Some(service) = self.service.as_ref() else {
            return;
        };
        let mut input = DiagnosticInput::new(severity, "container-stream", event, code, message)
            .retryable(retryable);
        input.correlation = self.correlation.clone();
        if let Some(object) = details.as_object() {
            input.details = object.clone();
        }
        service.emit(input);
    }

    /// The stream reached subscribed. Recorded at `info` because it is the
    /// transition that tells an investigator the gap started *after* here.
    pub fn subscribed(&self, waited: Duration) {
        self.emit(
            Severity::Info,
            "stream.subscribed",
            "stream_subscribed",
            "declared stream reached subscribed",
            false,
            json!({ "waited_ms": waited.as_millis() as u64 }),
        );
    }

    pub fn subscribe_timeout(&self, timeout: Duration) {
        self.subscribe_timeout_details(
            timeout,
            json!({ "timeout_ms": timeout.as_millis() as u64 }),
        );
    }

    fn subscribe_timeout_details(&self, timeout: Duration, details: Value) {
        self.emit(
            Severity::Error,
            "stream.subscribe.timeout",
            codes::STREAM_SUBSCRIBE_TIMEOUT,
            format!(
                "declared stream never reached subscribed within {}ms",
                timeout.as_millis()
            ),
            true,
            details,
        );
    }

    pub fn rollout_id(&self) -> &str {
        self.correlation.rollout_id.as_deref().unwrap_or("unknown")
    }

    /// The declared poll authority refused. The rollout is not started, so this
    /// is the cause of every downstream emptiness, not a symptom of it.
    pub fn poll_unavailable(&self, status: u16) {
        self.emit(
            Severity::Error,
            "stream.poll.unavailable",
            codes::STREAM_INTERRUPTED,
            format!("declared poll authority returned {status}"),
            true,
            json!({ "status": status }),
        );
    }

    /// Narrow an emitter to one rollout. The scripted-rollout helper mints its
    /// own ids, so the caller cannot supply them up front.
    pub fn with_rollout(mut self, rollout_id: &str) -> Self {
        self.correlation.rollout_id = Some(rollout_id.to_owned());
        if self.correlation.stream_id.is_none() {
            self.correlation.stream_id = Some(rollout_id.to_owned());
        }
        self
    }

    pub fn interrupted(&self, message: impl Into<String>, retryable: bool) {
        self.emit(
            Severity::Error,
            "stream.interrupted",
            codes::STREAM_INTERRUPTED,
            message,
            retryable,
            json!({}),
        );
    }
}

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

pub fn poll_event_kinds(poll: &Value) -> Vec<String> {
    poll_event_list(poll)
        .iter()
        .map(|event| {
            event
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string()
        })
        .collect()
}

fn redact_poll_url(url: &str) -> String {
    let Ok(mut parsed) = reqwest::Url::parse(url) else {
        return "unparseable-poll-url".to_string();
    };
    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);
    parsed.to_string()
}

struct PollTrace {
    attempts: u32,
    last_status: Option<u16>,
    last_kinds: Vec<String>,
}

impl PollTrace {
    fn snapshot(trace: &std::sync::Mutex<Self>) -> Self {
        trace
            .lock()
            .map(|guard| Self {
                attempts: guard.attempts,
                last_status: guard.last_status,
                last_kinds: guard.last_kinds.clone(),
            })
            .unwrap_or(Self {
                attempts: 0,
                last_status: None,
                last_kinds: Vec::new(),
            })
    }

    fn c1_08_message(
        &self,
        diagnostics: &StreamDiagnostics,
        poll_url: &str,
        timeout: Duration,
        elapsed: Duration,
    ) -> String {
        format!(
            "C1-08: refusing POST /rollouts; poll did not contain stream.subscribed with ready:true \
             (rollout_id={}, poll_url={}, http_status={}, event_kinds={}, elapsed_ms={}, timeout_ms={}, attempts={})",
            diagnostics.rollout_id(),
            redact_poll_url(poll_url),
            self.last_status
                .map(|status| status.to_string())
                .unwrap_or_else(|| "none".to_string()),
            if self.last_kinds.is_empty() {
                "none".to_string()
            } else {
                self.last_kinds.join(",")
            },
            elapsed.as_millis(),
            timeout.as_millis(),
            self.attempts
        )
    }
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
    diagnostics: &StreamDiagnostics,
) -> Result<Value> {
    let started = std::time::Instant::now();
    let trace = std::sync::Mutex::new(PollTrace {
        attempts: 0,
        last_status: None,
        last_kinds: Vec::new(),
    });
    let poll_once = async {
        loop {
            let response = client
                .get(poll_url)
                .query(&[("after", "0")])
                .send()
                .await
                .with_context(|| {
                    format!(
                        "poll declared transports.poll.url ({})",
                        redact_poll_url(poll_url)
                    )
                })?;
            let status = response.status().as_u16();
            if let Ok(mut guard) = trace.lock() {
                guard.attempts = guard.attempts.saturating_add(1);
                guard.last_status = Some(status);
            }
            if status == 503 {
                diagnostics.poll_unavailable(503);
                bail!("refusing start: declared poll returned 503");
            }
            let poll = response
                .error_for_status()
                .with_context(|| {
                    format!(
                        "poll declared transports.poll.url ({})",
                        redact_poll_url(poll_url)
                    )
                })?
                .json::<Value>()
                .await?;
            let kinds = poll_event_kinds(&poll);
            if let Ok(mut guard) = trace.lock() {
                guard.last_kinds = kinds;
            }
            if poll_has_stream_subscribed(&poll) {
                return Ok(poll);
            }
            tokio::time::sleep(SUBSCRIBE_POLL_INTERVAL).await;
        }
    };
    match tokio::time::timeout(timeout, poll_once).await {
        Ok(Ok(poll)) => {
            diagnostics.subscribed(started.elapsed());
            Ok(poll)
        }
        Ok(Err(error)) => Err(error),
        Err(_) => {
            let snapshot = PollTrace::snapshot(&trace);
            let elapsed = started.elapsed();
            diagnostics.subscribe_timeout_details(
                timeout,
                json!({
                    "timeout_ms": timeout.as_millis() as u64,
                    "elapsed_ms": elapsed.as_millis() as u64,
                    "attempts": snapshot.attempts,
                    "http_status": snapshot.last_status,
                    "event_kinds": snapshot.last_kinds,
                    "poll_url": redact_poll_url(poll_url),
                    "rollout_id": diagnostics.rollout_id(),
                }),
            );
            bail!(snapshot.c1_08_message(diagnostics, poll_url, timeout, elapsed))
        }
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
            &StreamDiagnostics::none(),
        )
        .await
        .unwrap_err();
        assert!(
            err.to_string().contains("503"),
            "expected 503 refuse, got {err}"
        );
        task.abort();
    }

    #[tokio::test]
    async fn subscribe_timeout_names_rollout_poll_url_status_kinds_and_attempts() {
        use crate::ipc::{serve_json, JsonHttpRequest, JsonHttpResponse};

        async fn heartbeat(_request: JsonHttpRequest) -> JsonHttpResponse {
            JsonHttpResponse::ok(json!({"events": [{"kind": "heartbeat"}]}))
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let _ = serve_json(listener, heartbeat).await;
        });
        let client = crate::http::http_client_builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let poll_url = format!("http://{addr}/rollouts/roll_timeout/events");
        let err = wait_for_stream_subscribed(
            &client,
            &poll_url,
            Duration::from_millis(250),
            &StreamDiagnostics::none().with_rollout("roll_timeout"),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("C1-08"), "{err}");
        assert!(err.contains("rollout_id=roll_timeout"), "{err}");
        assert!(err.contains(&format!("poll_url={poll_url}")), "{err}");
        assert!(err.contains("http_status=200"), "{err}");
        assert!(err.contains("event_kinds=heartbeat"), "{err}");
        assert!(err.contains("attempts="), "{err}");
        assert!(err.contains("elapsed_ms="), "{err}");
        task.abort();
    }

    #[tokio::test]
    async fn ten_independent_prepared_streams_reach_post_exactly_once() {
        use crate::ipc::{serve_json, JsonHttpRequest, JsonHttpResponse};
        use hyper::StatusCode;
        use std::collections::HashMap;
        use std::sync::Arc;
        use std::time::Instant;
        use tokio::sync::Mutex;

        #[derive(Default)]
        struct Lane {
            ack_delivered: u32,
            posts: u32,
            posts_before_ack: u32,
        }

        struct World {
            ready_at: Instant,
            lanes: Mutex<HashMap<String, Lane>>,
        }

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let world = Arc::new(World {
            ready_at: Instant::now() + Duration::from_millis(2_200),
            lanes: Mutex::new(HashMap::new()),
        });
        let handler_world = Arc::clone(&world);
        let task = tokio::spawn(async move {
            let _ = serve_json(listener, move |request: JsonHttpRequest| {
                let world = Arc::clone(&handler_world);
                async move {
                    let path = request.path.split('?').next().unwrap_or(request.path.as_str());
                    if request.method == hyper::Method::POST && path == "/rollouts/prepare" {
                        let Some(rollout_id) = request
                            .body
                            .get("rollout_id")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                        else {
                            return JsonHttpResponse::error(
                                StatusCode::BAD_REQUEST,
                                "prepare requires rollout_id",
                            );
                        };
                        world
                            .lanes
                            .lock()
                            .await
                            .entry(rollout_id.clone())
                            .or_default();
                        return JsonHttpResponse::ok(json!({
                            "rollout_id": rollout_id,
                            "stream": {
                                "id": format!("stream:{rollout_id}"),
                                "transports": {
                                    "poll": { "url": format!("/rollouts/{rollout_id}/events") }
                                }
                            }
                        }));
                    }
                    if request.method == hyper::Method::GET
                        && path.starts_with("/rollouts/")
                        && path.ends_with("/events")
                    {
                        let rollout_id = path
                            .trim_start_matches("/rollouts/")
                            .trim_end_matches("/events")
                            .to_string();
                        let events = if Instant::now() >= world.ready_at {
                            let mut lanes = world.lanes.lock().await;
                            if let Some(lane) = lanes.get_mut(&rollout_id) {
                                lane.ack_delivered = lane.ack_delivered.saturating_add(1);
                            }
                            json!([{
                                "kind": "stream.subscribed",
                                "control": true,
                                "ready": true,
                                "payload": {
                                    "ready": true,
                                    "stream.id": format!("stream:{rollout_id}"),
                                    "rollout_id": rollout_id,
                                    "next_sequence": 1
                                }
                            }])
                        } else {
                            json!([{"kind": "heartbeat"}])
                        };
                        return JsonHttpResponse::ok(json!({ "events": events }));
                    }
                    if request.method == hyper::Method::POST && path == "/rollouts" {
                        let Some(rollout_id) = request
                            .body
                            .get("rollout_id")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                        else {
                            return JsonHttpResponse::error(
                                StatusCode::BAD_REQUEST,
                                "start requires rollout_id",
                            );
                        };
                        let mut lanes = world.lanes.lock().await;
                        let lane = lanes.entry(rollout_id.clone()).or_default();
                        if lane.ack_delivered == 0 {
                            lane.posts_before_ack = lane.posts_before_ack.saturating_add(1);
                            return JsonHttpResponse::error(
                                StatusCode::CONFLICT,
                                "C1-08: refusing POST /rollouts; poll did not contain stream.subscribed with ready:true",
                            );
                        }
                        lane.posts = lane.posts.saturating_add(1);
                        return JsonHttpResponse::ok(json!({
                            "rollout_id": rollout_id,
                            "status": "started"
                        }));
                    }
                    JsonHttpResponse::error(StatusCode::NOT_FOUND, "unknown route")
                }
            })
            .await;
        });

        let client = crate::http::http_client_builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap();
        let base = format!("http://{addr}");
        let mut tasks = tokio::task::JoinSet::new();
        for index in 0..10 {
            let client = client.clone();
            let base = base.clone();
            tasks.spawn(async move {
                let rollout_id = format!("roll_eval_{index}");
                let prepared = client
                    .post(format!("{base}/rollouts/prepare"))
                    .json(&json!({ "rollout_id": rollout_id }))
                    .send()
                    .await
                    .unwrap()
                    .error_for_status()
                    .unwrap()
                    .json::<Value>()
                    .await
                    .unwrap();
                let stream = declared_stream_descriptor(&prepared)
                    .unwrap()
                    .expect("prepare omitted stream");
                let poll_url =
                    resolve_declared_url(&base, &declared_poll_url(&stream).unwrap()).unwrap();
                wait_for_stream_subscribed(
                    &client,
                    &poll_url,
                    SUBSCRIBE_READY_TIMEOUT,
                    &StreamDiagnostics::none().with_rollout(&rollout_id),
                )
                .await
                .unwrap();
                let started = client
                    .post(format!("{base}/rollouts"))
                    .json(&json!({ "rollout_id": rollout_id }))
                    .send()
                    .await
                    .unwrap();
                assert!(
                    started.status().is_success(),
                    "POST /rollouts failed for {rollout_id}: {}",
                    started.status()
                );
                rollout_id
            });
        }

        let mut started = Vec::new();
        while let Some(result) = tasks.join_next().await {
            started.push(result.expect("join handshake task"));
        }
        started.sort();
        assert_eq!(
            started,
            (0..10)
                .map(|index| format!("roll_eval_{index}"))
                .collect::<Vec<_>>()
        );

        let premature = client
            .post(format!("{base}/rollouts/prepare"))
            .json(&json!({ "rollout_id": "roll_eval_premature" }))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
        drop(premature);
        let refused = client
            .post(format!("{base}/rollouts"))
            .json(&json!({ "rollout_id": "roll_eval_premature" }))
            .send()
            .await
            .unwrap();
        assert_eq!(refused.status().as_u16(), 409);

        let lanes = world.lanes.lock().await;
        assert_eq!(lanes.len(), 11);
        for index in 0..10 {
            let lane = lanes.get(&format!("roll_eval_{index}")).expect("lane");
            assert_eq!(lane.posts, 1, "stream {index} must POST exactly once");
            assert_eq!(lane.posts_before_ack, 0);
            assert!(lane.ack_delivered >= 1);
        }
        let premature_lane = lanes.get("roll_eval_premature").expect("premature lane");
        assert_eq!(premature_lane.posts, 0);
        assert_eq!(premature_lane.posts_before_ack, 1);
        task.abort();
    }
}
