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

