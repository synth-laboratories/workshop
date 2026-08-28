//! `synth.diagnostic-event.v1` — the one versioned diagnostic contract.
//!
//! Every emitter on every surface (renderer, Tauri backend, MCP adapters,
//! containers, visuals, optimizers, providers) produces this envelope and
//! nothing else. Correlation identifiers are individually optional; an emitter
//! must include every identifier it holds, because the whole point of the
//! system is joining a renderer symptom to the rollout, stream, and trace that
//! produced it.
//!
//! Low-cardinality fields (`severity`, `component`, `event`, `code`,
//! `instance_id`) are the indexed labels. High-cardinality identities stay in
//! the body so the VictoriaLogs stream count cannot explode.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

pub const DIAGNOSTIC_EVENT_SCHEMA: &str = "synth.diagnostic-event.v1";

/// Journal `kind` for a persisted diagnostic. The journal stays the single
/// authoritative event model; diagnostics are one more kind inside it.
pub const JOURNAL_KIND: &str = "diagnostic.event";

/// Bounds. A diagnostic that needs more room than this is a diagnostic that
/// has started carrying a payload, which is what `details` must never become.
pub const MAX_MESSAGE_CHARS: usize = 2_000;
pub const MAX_IDENTIFIER_CHARS: usize = 200;
pub const MAX_DETAILS_BYTES: usize = 8 * 1024;
pub const MAX_DETAILS_KEYS: usize = 32;
pub const MAX_DETAILS_DEPTH: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Debug,
    Info,
    Warn,
    Error,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "debug" => Some(Self::Debug),
            "info" => Some(Self::Info),
            "warn" | "warning" => Some(Self::Warn),
            "error" | "fatal" | "critical" => Some(Self::Error),
            _ => None,
        }
    }

    /// Errors survive queue saturation; everything else is droppable.
    pub fn is_preserved(self) -> bool {
        matches!(self, Self::Error)
    }
}

/// Allow-listed components. This is a closed set on purpose: it is an indexed
/// label, and an open string field is how a local log index acquires an
/// unbounded number of streams.
pub const COMPONENTS: &[&str] = &[
    "renderer",
    "visual-host",
    "visual-registry",
    "visual-ipc",
    "containers",
    "container-stream",
    "mcp",
    "optimizers",
    "optimizer-sidecar",
    "provider",
    "session",
    "storage",
    "laguna",
    "plugins",
    "eval-driver",
    "credential-broker",
    "diagnostics",
    "update",
];

pub fn is_known_component(value: &str) -> bool {
    COMPONENTS.contains(&value)
}

/// Named groups an agent can ask for without knowing the component list.
pub const SCOPES: &[(&str, &[&str])] = &[
    ("renderer", &["renderer", "visual-host"]),
    (
        "visuals",
        &["visual-host", "visual-registry", "visual-ipc", "renderer"],
    ),
    ("containers", &["containers", "container-stream"]),
    ("streams", &["container-stream", "renderer"]),
    ("mcp", &["mcp", "visual-ipc"]),
    ("optimizers", &["optimizers", "optimizer-sidecar"]),
    ("providers", &["provider", "session", "credential-broker"]),
    ("session", &["session", "provider"]),
    ("storage", &["storage"]),
    ("diagnostics", &["diagnostics"]),
];

pub fn scope_components(scope: &str) -> Option<&'static [&'static str]> {
    SCOPES
        .iter()
        .find(|(name, _)| *name == scope)
        .map(|(_, components)| *components)
}

/// Correlation identity. Each field is optional alone; emitters are expected to
/// fill in everything they hold.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Correlation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visual_revision: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollout_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optimizer_run_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
}

/// Every correlation field, as `(wire name, value)`. One list so the query
/// compiler, the explainer, and the indexer cannot drift from the struct.
pub const CORRELATION_FIELDS: &[&str] = &[
    "instance_id",
    "session_id",
    "turn_id",
    "tool_call_id",
    "command_id",
    "visual_id",
    "container_id",
    "rollout_id",
    "stream_id",
    "optimizer_run_id",
    "trace_id",
];

impl Correlation {
    pub fn get(&self, field: &str) -> Option<&str> {
        match field {
            "instance_id" => self.instance_id.as_deref(),
            "session_id" => self.session_id.as_deref(),
            "turn_id" => self.turn_id.as_deref(),
            "tool_call_id" => self.tool_call_id.as_deref(),
            "command_id" => self.command_id.as_deref(),
            "visual_id" => self.visual_id.as_deref(),
            "container_id" => self.container_id.as_deref(),
            "rollout_id" => self.rollout_id.as_deref(),
            "stream_id" => self.stream_id.as_deref(),
            "optimizer_run_id" => self.optimizer_run_id.as_deref(),
            "trace_id" => self.trace_id.as_deref(),
            _ => None,
        }
    }

    pub fn set(&mut self, field: &str, value: Option<String>) {
        match field {
            "instance_id" => self.instance_id = value,
            "session_id" => self.session_id = value,
            "turn_id" => self.turn_id = value,
            "tool_call_id" => self.tool_call_id = value,
            "command_id" => self.command_id = value,
            "visual_id" => self.visual_id = value,
            "container_id" => self.container_id = value,
            "rollout_id" => self.rollout_id = value,
            "stream_id" => self.stream_id = value,
            "optimizer_run_id" => self.optimizer_run_id = value,
            "trace_id" => self.trace_id = value,
            _ => {}
        }
    }

    pub fn present(&self) -> BTreeMap<&'static str, String> {
        CORRELATION_FIELDS
            .iter()
            .filter_map(|field| self.get(field).map(|value| (*field, value.to_owned())))
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.present().is_empty() && self.visual_revision.is_none()
    }
}

/// A validated, redacted diagnostic ready to persist and index.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DiagnosticEvent {
    pub schema: String,
    pub event_id: String,
    pub timestamp: String,
    pub severity: Severity,
    pub component: String,
    pub event: String,
    pub code: String,
    pub message: String,
    pub retryable: bool,
    #[serde(flatten)]
    pub correlation: Correlation,
    #[serde(default)]
    pub details: Map<String, Value>,
}

/// What an emitter supplies. Everything the envelope can derive is derived.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct DiagnosticInput {
    #[serde(default)]
    pub event_id: Option<String>,
    #[serde(default)]
    pub timestamp: Option<String>,
    pub severity: String,
    pub component: String,
    pub event: String,
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub retryable: bool,
    #[serde(default, flatten)]
    pub correlation: Correlation,
    #[serde(default)]
    pub details: Map<String, Value>,
}

impl DiagnosticInput {
    pub fn new(
        severity: Severity,
        component: &str,
        event: &str,
        code: &str,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity: severity.as_str().into(),
            component: component.into(),
            event: event.into(),
            code: code.into(),
            message: message.into(),
            ..Default::default()
        }
    }

    pub fn with_detail(mut self, key: &str, value: Value) -> Self {
        self.details.insert(key.into(), value);
        self
    }

    pub fn with_correlation(mut self, field: &str, value: Option<String>) -> Self {
        self.correlation.set(field, value);
        self
    }

    pub fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }
}

/// Validate, bound, and redact one emitted diagnostic.
///
/// Redaction runs here — before the event reaches the queue, the journal, or
/// the index — so no later stage can be the thing that leaks. Rejection is
/// preferred over silent repair for anything that would corrupt an indexed
/// label; message and details are truncated rather than rejected because
/// losing a real failure to a length bound is worse than a truncated one.
pub fn validate(input: DiagnosticInput) -> Result<DiagnosticEvent> {
    let severity = Severity::parse(input.severity.trim())
        .ok_or_else(|| anyhow::anyhow!("unknown diagnostic severity `{}`", input.severity))?;
    let component = input.component.trim();
    if !is_known_component(component) {
        bail!("unknown diagnostic component `{component}`");
    }
    let event = input.event.trim();
    if !is_dotted_identifier(event) {
        bail!("diagnostic event `{event}` must be a dotted lowercase identifier");
    }
    let code = input.code.trim();
    if !is_snake_identifier(code) {
        bail!("diagnostic code `{code}` must be a snake_case identifier");
    }

    let event_id = input
        .event_id
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty() && value.len() <= MAX_IDENTIFIER_CHARS)
        .unwrap_or_else(|| format!("diag_{}", uuid::Uuid::new_v4()));
    let timestamp = input
        .timestamp
        .and_then(|value| {
            chrono::DateTime::parse_from_rfc3339(value.trim())
                .ok()
                .map(|parsed| parsed.to_utc().to_rfc3339())
        })
        .unwrap_or_else(|| chrono::Utc::now().to_rfc3339());

    let mut correlation = input.correlation;
    for field in CORRELATION_FIELDS {
        let cleaned = correlation
            .get(field)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| truncate_chars(value, MAX_IDENTIFIER_CHARS));
        correlation.set(field, cleaned);
    }

    let message = truncate_chars(
        &super::redact::redact_text(input.message.trim()),
        MAX_MESSAGE_CHARS,
    );
    let details = super::redact::redact_details(input.details);

    Ok(DiagnosticEvent {
        schema: DIAGNOSTIC_EVENT_SCHEMA.into(),
        event_id,
        timestamp,
        severity,
        component: component.to_owned(),
        event: event.to_owned(),
        code: code.to_owned(),
        message,
        retryable: input.retryable,
        correlation,
        details,
    })
}

impl DiagnosticEvent {
    /// Journal payload. `AppEvent.kind` carries [`JOURNAL_KIND`]; the envelope
    /// is the payload verbatim so a journal row round-trips losslessly.
    pub fn to_payload(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| json!({}))
    }

    pub fn from_payload(payload: &Value) -> Option<Self> {
        serde_json::from_value(payload.clone()).ok()
    }

    /// One VictoriaLogs JSON line. `_msg`/`_time` are VL's own field names;
    /// `journal_sequence` is the idempotency key that makes replay after a
    /// restart produce no duplicate logical results.
    pub fn to_index_line(&self, journal_sequence: i64) -> Value {
        let mut line = json!({
            "_msg": self.message,
            "_time": self.timestamp,
            "severity": self.severity.as_str(),
            "component": self.component,
            "event": self.event,
            "code": self.code,
            "retryable": self.retryable,
            "event_id": self.event_id,
            "journal_sequence": journal_sequence.to_string(),
        });
        let object = line.as_object_mut().expect("json object");
        for (field, value) in self.correlation.present() {
            object.insert(field.to_owned(), Value::String(value));
        }
        if let Some(revision) = self.correlation.visual_revision {
            object.insert("visual_revision".into(), json!(revision.to_string()));
        }
        if !self.details.is_empty() {
            object.insert(
                "details".into(),
                Value::String(Value::Object(self.details.clone()).to_string()),
            );
        }
        line
    }
}

fn is_dotted_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
        && !value.starts_with('.')
        && !value.ends_with('.')
}

fn is_snake_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

pub(crate) fn truncate_chars(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_owned();
    }
    let mut out: String = value.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

