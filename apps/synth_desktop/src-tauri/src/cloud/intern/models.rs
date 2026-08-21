use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const EVENT_SCHEMA: &str = "smr.intern-runtime-event.v1";
pub const RECEIPT_SCHEMA: &str = "smr.intern-runtime-command-receipt.v1";

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeBinding {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub factory_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionMode {
    Fast,
    Standard,
    Deep,
}

impl Default for ExecutionMode {
    fn default() -> Self {
        Self::Standard
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SyncCreateRequest {
    #[serde(default)]
    pub objective: String,
    pub idempotency_key: String,
    #[serde(default)]
    pub binding: RuntimeBinding,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    #[serde(default)]
    pub execution_mode: ExecutionMode,
    #[serde(default = "yes")]
    pub require_operator_approval: bool,
}

fn yes() -> bool {
    true
}

impl SyncCreateRequest {
    pub fn desktop(
        objective: impl Into<String>,
        idempotency_key: impl Into<String>,
        binding: RuntimeBinding,
    ) -> Self {
        Self {
            objective: objective.into(),
            idempotency_key: idempotency_key.into(),
            binding,
            metadata: Map::new(),
            execution_mode: ExecutionMode::Standard,
            require_operator_approval: true,
        }
    }

    pub fn validate(&self) -> Result<()> {
        require_text("objective", &self.objective)?;
        require_text("idempotency_key", &self.idempotency_key)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyncCommandKind {
    OperatorMessage,
    Intervene,
    AnswerInteraction,
    Pause,
    Resume,
    Close,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SyncCommandRequest {
    pub command_id: String,
    pub idempotency_key: String,
    pub expected_generation: u64,
    pub command_kind: SyncCommandKind,
    #[serde(default)]
    pub payload: Map<String, Value>,
    #[serde(default)]
    pub execution_mode: ExecutionMode,
    #[serde(default = "sync_mode")]
    pub mode: String,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
}

fn sync_mode() -> String {
    "sync".to_owned()
}

impl SyncCommandRequest {
    pub fn operator_message(
        command_id: impl Into<String>,
        idempotency_key: impl Into<String>,
        expected_generation: u64,
        body: impl Into<String>,
    ) -> Self {
        let command_id = command_id.into();
        let mut payload = Map::new();
        payload.insert("body".into(), Value::String(body.into()));
        payload.insert("context".into(), Value::Object(Map::new()));
        payload.insert("turn_id".into(), Value::String(command_id.clone()));
        Self {
            command_id,
            idempotency_key: idempotency_key.into(),
            expected_generation,
            command_kind: SyncCommandKind::OperatorMessage,
            payload,
            execution_mode: ExecutionMode::Standard,
            mode: sync_mode(),
            evidence_refs: Vec::new(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        require_text("command_id", &self.command_id)?;
        require_text("idempotency_key", &self.idempotency_key)?;
        if self.mode != "sync" {
            bail!("sync command mode must be sync");
        }
        match self.command_kind {
            SyncCommandKind::OperatorMessage | SyncCommandKind::Intervene => {
                require_payload_text(&self.payload, &["body"])?
            }
            SyncCommandKind::AnswerInteraction => {
                require_payload_text(&self.payload, &["interaction_id"])?;
                require_payload_text(&self.payload, &["body", "answer"])?;
            }
            SyncCommandKind::Pause => {
                require_payload_text(&self.payload, &["reason", "rationale"])?
            }
            SyncCommandKind::Close => {
                require_payload_text(&self.payload, &["outcome", "status"])?;
                require_payload_text(&self.payload, &["reason", "rationale"])?;
            }
            SyncCommandKind::Resume => {}
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AsyncBudget {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum_cost_cents: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum_daily_cost_cents: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum_monthly_cost_cents: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum_cycles: Option<u64>,
    #[serde(default = "one")]
    pub maximum_concurrent_runs: u64,
}

fn one() -> u64 {
    1
}

impl Default for AsyncBudget {
    fn default() -> Self {
        Self {
            maximum_cost_cents: None,
            maximum_daily_cost_cents: None,
            maximum_monthly_cost_cents: None,
            maximum_cycles: None,
            maximum_concurrent_runs: 1,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AsyncEnsureRequest {
    #[serde(default)]
    pub objective: String,
    pub idempotency_key: String,
    #[serde(default)]
    pub binding: RuntimeBinding,
    #[serde(default)]
    pub budget: AsyncBudget,
    #[serde(default)]
    pub metadata: Map<String, Value>,
    #[serde(default)]
    pub factory_ready_wait_seconds: u8,
}

impl AsyncEnsureRequest {
    pub fn desktop(
        objective: impl Into<String>,
        idempotency_key: impl Into<String>,
        binding: RuntimeBinding,
    ) -> Self {
        Self {
            objective: objective.into(),
            idempotency_key: idempotency_key.into(),
            binding,
            budget: AsyncBudget {
                maximum_concurrent_runs: 1,
                ..Default::default()
            },
            metadata: Map::new(),
            factory_ready_wait_seconds: 0,
        }
    }

    pub fn validate(&self) -> Result<()> {
        require_text("objective", &self.objective)?;
        require_text("idempotency_key", &self.idempotency_key)?;
        if self.factory_ready_wait_seconds > 60 {
            bail!("factory_ready_wait_seconds must be <= 60");
        }
        if self.budget.maximum_concurrent_runs == 0 {
            bail!("maximum_concurrent_runs must be >= 1");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AsyncCommandKind {
    Pause,
    Resume,
    Cancel,
    ProvideInput,
    AnswerInteraction,
    Message,
    Intervene,
    RedirectObjective,
    RequestCheckpoint,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AsyncCommandRequest {
    pub command_id: String,
    pub idempotency_key: String,
    pub expected_generation: u64,
    pub command_kind: AsyncCommandKind,
    #[serde(default)]
    pub payload: Map<String, Value>,
}

impl AsyncCommandRequest {
    pub fn message(
        command_id: impl Into<String>,
        idempotency_key: impl Into<String>,
        expected_generation: u64,
        body: impl Into<String>,
        context: Map<String, Value>,
    ) -> Self {
        let mut payload = Map::new();
        payload.insert("body".into(), Value::String(body.into()));
        payload.insert("context".into(), Value::Object(context));
        Self {
            command_id: command_id.into(),
            idempotency_key: idempotency_key.into(),
            expected_generation,
            command_kind: AsyncCommandKind::Message,
            payload,
        }
    }

    pub fn validate(&self) -> Result<()> {
        require_text("command_id", &self.command_id)?;
        require_text("idempotency_key", &self.idempotency_key)?;
        match self.command_kind {
            AsyncCommandKind::Pause | AsyncCommandKind::Cancel => {
                require_payload_text(&self.payload, &["reason"])?
            }
            AsyncCommandKind::ProvideInput | AsyncCommandKind::AnswerInteraction => {
                require_payload_text(&self.payload, &["interaction_id"])?;
                require_payload_text(&self.payload, &["body"])?;
            }
            AsyncCommandKind::Message
            | AsyncCommandKind::Intervene
            | AsyncCommandKind::RedirectObjective => {
                require_payload_text(&self.payload, &["body"])?
            }
            AsyncCommandKind::RequestCheckpoint => {
                if self
                    .payload
                    .get("body")
                    .and_then(Value::as_str)
                    .is_some_and(|v| !v.trim().is_empty())
                {
                    bail!("request_checkpoint does not accept body");
                }
            }
            AsyncCommandKind::Resume => {}
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeKind {
    Sync,
    Async,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
pub struct CommandReceipt {
    pub schema_version: String,
    pub command_id: String,
    pub runtime_kind: RuntimeKind,
    pub runtime_id: String,
    pub status: String,
    #[specta(type = specta_typescript::Number)]
    pub previous_generation: u64,
    #[specta(type = specta_typescript::Number)]
    pub state_generation: u64,
    pub decision_code: String,
    pub created_at: String,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub actuation: Option<Value>,
    #[serde(default)]
    pub duplicate: bool,
}

impl CommandReceipt {
    pub fn validate_for(&self, command_id: &str) -> Result<()> {
        if self.schema_version != RECEIPT_SCHEMA {
            bail!("unsupported receipt schema");
        }
        if self.command_id != command_id {
            bail!("command receipt identity drifted");
        }
        if self.state_generation < self.previous_generation {
            bail!("receipt generation regressed");
        }
        self.local_terminal_status()?;
        Ok(())
    }

    /// Map the provider's admission/decision receipt onto the desktop's
    /// durable terminal receipt states. HTTP 202 only means the provider
    /// returned a receipt; it does not make refused/conflicting commands
    /// successful local work.
    pub fn local_terminal_status(&self) -> Result<&'static str> {
        match self.status.as_str() {
            "received" | "delivered" | "applied" | "noop" => Ok("completed"),
            "refused" | "superseded" | "conflict" => Ok("rejected"),
            status => bail!("unsupported command receipt status: {status}"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct InternEvent {
    pub schema_version: String,
    pub event_id: String,
    pub runtime_kind: RuntimeKind,
    pub runtime_id: String,
    pub sequence: u64,
    pub previous_state_generation: u64,
    pub state_generation: u64,
    pub event_kind: String,
    pub command_id: String,
    pub payload: Value,
    pub created_at: String,
}

impl InternEvent {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != EVENT_SCHEMA {
            bail!("unsupported Intern event schema");
        }
        if self.sequence == 0 {
            bail!("Intern event sequence must be >= 1");
        }
        if self.state_generation == 0 || self.state_generation < self.previous_state_generation {
            bail!("Intern event generation is invalid");
        }
        require_text("event_id", &self.event_id)?;
        require_text("runtime_id", &self.runtime_id)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum EventListResponse {
    Bare(Vec<InternEvent>),
    Events { events: Vec<InternEvent> },
    Items { items: Vec<InternEvent> },
    Data { data: Vec<InternEvent> },
}

impl EventListResponse {
    pub fn into_events(self) -> Vec<InternEvent> {
        match self {
            Self::Bare(v) => v,
            Self::Events { events } => events,
            Self::Items { items } => items,
            Self::Data { data } => data,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct RuntimeProjection {
    #[serde(default)]
    pub sync_session_id: Option<String>,
    #[serde(default)]
    pub async_runtime_id: Option<String>,
    #[serde(default)]
    pub async_assignment_id: Option<String>,
    pub status: String,
    pub state_generation: u64,
    #[serde(default)]
    pub last_event_sequence: u64,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl RuntimeProjection {
    pub fn runtime_id(&self) -> Option<&str> {
        self.sync_session_id
            .as_deref()
            .or(self.async_runtime_id.as_deref())
    }

    pub fn validate_async_identity(&self) -> Result<()> {
        if let (Some(runtime), Some(assignment)) =
            (&self.async_runtime_id, &self.async_assignment_id)
        {
            if runtime != assignment {
                bail!("Async Intern compatibility identity drifted");
            }
        }
        Ok(())
    }
}

fn require_text(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{name} is required");
    }
    Ok(())
}

fn require_payload_text(payload: &Map<String, Value>, names: &[&str]) -> Result<()> {
    if names.iter().any(|name| {
        payload
            .get(*name)
            .and_then(Value::as_str)
            .is_some_and(|v| !v.trim().is_empty())
    }) {
        return Ok(());
    }
    bail!("{} requires {}", names[0], names.join(" or "))
}
