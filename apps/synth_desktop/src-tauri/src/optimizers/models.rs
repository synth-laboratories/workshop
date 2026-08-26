use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const OPTIMIZER_RUN_SCHEMA_VERSION: &str = "optimizer_run.v1";
pub const OPTIMIZER_EVENT_SCHEMA_VERSION: &str = "optimizer_event.v1";
pub const OPTIMIZER_STATE_SLICE_SCHEMA_VERSION: &str = "optimizer_state_slice.v1";

/// The one status vocabulary for `optimizer_runs`.
///
/// Before this type there were four disagreeing terminal predicates
/// (`service::is_terminal_status`, `validate_control`, the milestone wait, and
/// `recipes::reconcile_persisted`) over an untyped `String` column with a
/// fifteen-word vocabulary. A run that read as finished to one of them read as
/// live to another, which is how a settled run kept a spinner turning.
///
/// The variants are exactly the spellings a producer writes today — the local
/// recipe workers, the training-lifecycle projection, and the Synth Cloud
/// mirror. [`OptimizerRunStatus::parse`] additionally accepts the legacy
/// aliases those producers used to emit so a database written by an older
/// build still reads; every alias normalizes onto one canonical spelling, and
/// migration 28 rewrites the stored column so the trigger domain holds. The
/// aliases live in `parse` and not in `#[serde(alias)]` on purpose: they are a
/// read-compatibility detail of this build, not part of the contract the
/// renderer is handed, and putting them on the variants would export them as
/// legal values in the generated TypeScript union.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum OptimizerRunStatus {
    /// Admitted, not yet started.
    Queued,
    /// Config admission is running (training lane).
    Validating,
    /// Compute is being acquired (training lane).
    Provisioning,
    /// The worker process is coming up.
    Starting,
    /// A prepared run held until its viewer attaches.
    WaitingForViewer,
    Running,
    Paused,
    /// A cancel was accepted and is being carried out.
    Cancelling,
    /// The training environment stopped answering; compute may still be live.
    EnvUnreachable,
    /// Finished, but its evidence lane did not settle cleanly.
    Degraded,
    Completed,
    Failed,
    /// Terminal with unusable evidence — distinct from `Failed` because the
    /// work ran; only the receipt is missing.
    FailedEvidence,
    Cancelled,
    /// The owner process died without sealing; recovery rewrote the row.
    Interrupted,
    InfrastructureLost,
    /// Stopped because a spend or step ceiling was reached.
    CapReached,
}

impl OptimizerRunStatus {
    /// Every canonical spelling, in lifecycle order. The migration-28 trigger
    /// domain and the generated TypeScript union are both this list.
    pub const ALL: &'static [Self] = &[
        Self::Queued,
        Self::Validating,
        Self::Provisioning,
        Self::Starting,
        Self::WaitingForViewer,
        Self::Running,
        Self::Paused,
        Self::Cancelling,
        Self::EnvUnreachable,
        Self::Degraded,
        Self::Completed,
        Self::Failed,
        Self::FailedEvidence,
        Self::Cancelled,
        Self::Interrupted,
        Self::InfrastructureLost,
        Self::CapReached,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Validating => "validating",
            Self::Provisioning => "provisioning",
            Self::Starting => "starting",
            Self::WaitingForViewer => "waiting_for_viewer",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Cancelling => "cancelling",
            Self::EnvUnreachable => "env_unreachable",
            Self::Degraded => "degraded",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::FailedEvidence => "failed_evidence",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
            Self::InfrastructureLost => "infrastructure_lost",
            Self::CapReached => "cap_reached",
        }
    }

    /// Canonical spelling, or `None` for a word no producer has ever written.
    ///
    /// Legacy aliases resolve here rather than at each call site: that is the
    /// whole point of the type. Unknown input is `None` — never `Running`,
    /// which is how an unrecognised terminal word used to keep a card live.
    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "queued" | "created" => Self::Queued,
            "validating" => Self::Validating,
            "provisioning" => Self::Provisioning,
            "starting" => Self::Starting,
            "waiting_for_viewer" => Self::WaitingForViewer,
            "running" => Self::Running,
            "paused" => Self::Paused,
            "cancelling" => Self::Cancelling,
            "env_unreachable" => Self::EnvUnreachable,
            "degraded" => Self::Degraded,
            "completed" | "succeeded" => Self::Completed,
            "failed" | "error" | "done" | "stopped" | "aborted" => Self::Failed,
            "failed_evidence" => Self::FailedEvidence,
            "cancelled" | "canceled" => Self::Cancelled,
            "interrupted" => Self::Interrupted,
            "infrastructure_lost" => Self::InfrastructureLost,
            "cap_reached" => Self::CapReached,
            _ => return None,
        })
    }

    /// The canonical spelling for a stored word, leaving anything unrecognised
    /// untouched so a write can still be inspected rather than silently
    /// relabelled. The migration-28 trigger is what refuses the unrecognised.
    pub fn canonical(value: &str) -> &str {
        match Self::parse(value) {
            Some(status) => status.as_str(),
            None => value,
        }
    }

    /// The single terminal predicate. Compute has stopped and the record will
    /// not move again on its own.
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed
                | Self::Failed
                | Self::FailedEvidence
                | Self::Cancelled
                | Self::Interrupted
                | Self::Degraded
                | Self::InfrastructureLost
                | Self::CapReached
        )
    }

    /// Terminal check straight off a stored word. An unrecognised word is not
    /// terminal: the run record is the terminal authority, and a word this
    /// build does not know is not that authority saying "finished".
    pub fn str_is_terminal(value: &str) -> bool {
        Self::parse(value).is_some_and(Self::is_terminal)
    }

    /// Whether `next` is a legal successor. Terminal is terminal: nothing
    /// leaves it, so a late event cannot resurrect a settled run.
    pub fn can_transition_to(self, next: Self) -> bool {
        if self == next {
            return true;
        }
        if self.is_terminal() {
            return false;
        }
        match next {
            // Only a running optimizer pauses.
            Self::Paused => self == Self::Running,
            // A queued run may start; a cancelling one may not go back to work.
            Self::Running => self != Self::Cancelling,
            _ => true,
        }
    }
}

/// Status vocabulary owned by training jobs, separate from optimizer run state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainingJobStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

impl TrainingJobStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "succeeded" | "completed" => Self::Succeeded,
            "failed" => Self::Failed,
            "cancelled" | "canceled" => Self::Cancelled,
            "interrupted" => Self::Interrupted,
            _ => return None,
        })
    }

    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Interrupted
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SavedLoraStorage {
    pub backend: String,
    pub bucket: String,
    pub key: String,
    pub version: Option<String>,
    pub etag: Option<String>,
    pub sha256: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub size_bytes: Option<u64>,
    pub content_type: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SavedLoraLineage {
    pub optimizer_algorithm: Option<String>,
    pub run_id: Option<String>,
    pub attempt_id: Option<String>,
    pub source_checkpoint_id: Option<String>,
    pub provider_checkpoint_reference: Option<String>,
}

fn default_hosted_placement() -> String {
    "hosted".into()
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SavedLoraCheckpoint {
    pub schema_version: String,
    pub checkpoint_id: String,
    pub org_id: String,
    pub owner_user_id: Option<String>,
    pub visibility: String,
    pub name: String,
    pub description: String,
    pub provider: String,
    pub checkpoint_kind: String,
    pub provider_checkpoint_reference: Option<String>,
    pub run_id: Option<String>,
    pub attempt_id: Option<String>,
    pub source_checkpoint_id: Option<String>,
    pub optimizer_algorithm: Option<String>,
    pub base_model: String,
    pub lora_rank: Option<i32>,
    #[specta(type = specta_typescript::Number)]
    pub step: Option<u64>,
    pub status: String,
    pub storage: SavedLoraStorage,
    #[serde(default)]
    pub lineage: SavedLoraLineage,
    #[serde(default = "default_hosted_placement")]
    pub placement: String,
    #[serde(default)]
    pub inference_chat_completions: bool,
    #[serde(default)]
    pub inference_responses: bool,
    pub tags: Vec<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub metadata: Value,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub archived_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SavedLoraCheckpointPage {
    pub schema_version: String,
    pub items: Vec<SavedLoraCheckpoint>,
    #[specta(type = specta_typescript::Number)]
    pub total: u64,
    #[specta(type = specta_typescript::Number)]
    pub limit: u64,
    #[specta(type = specta_typescript::Number)]
    pub offset: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SavedLoraRunIdentity {
    pub run_id: String,
    pub attempt_id: Option<String>,
    pub optimizer_algorithm: String,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SavedLoraRunCounts {
    #[specta(type = specta_typescript::Number)]
    pub total: u64,
    #[specta(type = specta_typescript::Number)]
    pub inference: u64,
    #[specta(type = specta_typescript::Number)]
    pub training: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SavedLoraRunPage {
    pub schema_version: String,
    pub run: SavedLoraRunIdentity,
    pub items: Vec<SavedLoraCheckpoint>,
    pub counts: SavedLoraRunCounts,
    #[specta(type = specta_typescript::Number)]
    pub total: u64,
    #[specta(type = specta_typescript::Number)]
    pub limit: u64,
    #[specta(type = specta_typescript::Number)]
    pub offset: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerRunOutputIdentity {
    pub run_id: String,
    pub attempt_id: Option<String>,
    pub optimizer_algorithm: String,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerRunOutputArtifact {
    pub artifact_id: String,
    pub run_id: String,
    pub artifact_name: String,
    pub content_type: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub size_bytes: u64,
    pub sha256: Option<String>,
    pub storage_backend: String,
    pub uri: String,
    pub download_path: String,
    #[specta(type = specta_typescript::Unknown)]
    pub metadata: Value,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerRunOutputCounts {
    #[specta(type = specta_typescript::Number)]
    pub artifacts: u64,
    #[specta(type = specta_typescript::Number)]
    pub model_checkpoints: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerRunOutputs {
    pub schema_version: String,
    pub run: OptimizerRunOutputIdentity,
    #[specta(type = specta_typescript::Unknown)]
    pub result: Option<Value>,
    pub artifacts: Vec<OptimizerRunOutputArtifact>,
    pub model_checkpoints: Vec<SavedLoraCheckpoint>,
    pub counts: OptimizerRunOutputCounts,
}

#[derive(Clone, Debug, Default, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SavedLoraCheckpointQuery {
    pub search: Option<String>,
    pub scope: Option<String>,
    pub placement: Option<String>,
    pub provider: Option<String>,
    pub checkpoint_kind: Option<String>,
    pub base_model: Option<String>,
    pub run_id: Option<String>,
    pub attempt_id: Option<String>,
    pub source_checkpoint_id: Option<String>,
    pub optimizer_algorithm: Option<String>,
    pub status: Option<String>,
    pub tags: Option<Vec<String>>,
    #[specta(type = specta_typescript::Number)]
    pub limit: Option<u64>,
    #[specta(type = specta_typescript::Number)]
    pub offset: Option<u64>,
}

#[derive(Clone, Debug, Default, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SavedLoraPatchRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub tags: Option<Vec<String>>,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointInferRequest {
    pub checkpoint_id: String,
    pub family: String,
    #[specta(type = specta_typescript::Unknown)]
    pub body: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SavedLoraDownload {
    pub checkpoint_id: String,
    pub url: String,
    #[specta(type = specta_typescript::Number)]
    pub expires_in: u64,
    pub content_type: String,
    #[specta(type = specta_typescript::Number)]
    pub size_bytes: Option<u64>,
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HostedTrainingModel {
    pub model_id: String,
    pub label: String,
    pub provider: String,
    pub provider_revision: String,
    pub architecture: String,
    #[specta(type = specta_typescript::Number)]
    pub max_context_length: u64,
    #[specta(type = specta_typescript::Unknown)]
    pub rank: Value,
    #[specta(type = specta_typescript::Unknown)]
    pub algorithms: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HostedTrainingModelCatalog {
    pub schema_version: String,
    pub catalog_revision: String,
    pub live_preflight_required: bool,
    pub models: Vec<HostedTrainingModel>,
    #[specta(type = specta_typescript::Number)]
    pub total: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerCapabilities {
    #[serde(default)]
    pub cancel: bool,
    #[serde(default)]
    pub pause: bool,
    #[serde(default)]
    pub resume: bool,
    #[serde(default)]
    pub stream_events: bool,
    #[serde(default)]
    pub state_slices: bool,
    #[serde(default)]
    pub candidates: bool,
    #[serde(default)]
    pub checkpoints: bool,
    #[serde(default)]
    pub checkpoint_evaluations: bool,
    #[serde(default)]
    pub inference_endpoint: bool,
    #[serde(default)]
    pub local_slot_binding: bool,
}

impl OptimizerCapabilities {
    pub fn for_algorithm(algorithm_id: &str) -> Self {
        match algorithm_id {
            "gepa" => Self {
                cancel: true,
                pause: true,
                resume: true,
                stream_events: true,
                state_slices: true,
                candidates: true,
                ..Self::default()
            },
            "go-ex" => Self {
                cancel: true,
                pause: true,
                resume: true,
                stream_events: true,
                state_slices: true,
                candidates: true,
                checkpoints: true,
                checkpoint_evaluations: true,
                local_slot_binding: true,
                ..Self::default()
            },
            "sft" | "cispo" | "ppo" => Self {
                cancel: true,
                pause: true,
                resume: true,
                stream_events: true,
                state_slices: true,
                checkpoints: true,
                checkpoint_evaluations: true,
                inference_endpoint: true,
                local_slot_binding: true,
                ..Self::default()
            },
            // Local `eval` can be cancelled, paused, and resumed, and streams
            // events, candidates, and state. It has no checkpoints and no
            // inference endpoint, and must not claim them: a scorecard is not
            // a model. Pausing holds the matrix; in-flight trials still seal.
            "eval" => Self {
                cancel: true,
                pause: true,
                resume: true,
                stream_events: true,
                state_slices: true,
                candidates: true,
                ..Self::default()
            },
            _ => Self {
                stream_events: true,
                state_slices: true,
                ..Self::default()
            },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerUsageSummary {
    /// Settled money, or `null` when no event has reported any. A run that
    /// never reports cost is unknown, not free — missing is never 0.
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub prompt_tokens: u64,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub completion_tokens: u64,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub rollouts: u64,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub wall_time_ms: u64,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerResourceRef {
    pub kind: String,
    pub id: String,
    #[serde(default)]
    pub digest: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub metadata: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerExecutionBinding {
    pub kind: String,
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub metadata: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerRunRecord {
    pub schema_version: String,
    pub id: String,
    pub algorithm_id: String,
    #[serde(default)]
    pub algorithm_version: Option<String>,
    pub status: String,
    pub source: String,
    #[serde(default)]
    pub objective: Option<String>,
    #[serde(default)]
    pub project_ref: Option<String>,
    #[serde(default)]
    pub session_ref: Option<String>,
    pub created_at: String,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub finished_at: Option<String>,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub cursor_seq: u64,
    #[serde(default)]
    pub capabilities: OptimizerCapabilities,
    #[serde(default)]
    pub execution_bindings: Vec<OptimizerExecutionBinding>,
    #[serde(default)]
    pub input_refs: Vec<OptimizerResourceRef>,
    #[serde(default)]
    pub output_refs: Vec<OptimizerResourceRef>,
    #[serde(default)]
    pub visual_refs: Vec<OptimizerResourceRef>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub summary: Value,
    #[serde(default)]
    pub usage: OptimizerUsageSummary,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub error: Option<Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerRelationship {
    pub from_kind: String,
    pub from_id: String,
    pub edge: String,
    pub to_kind: String,
    pub to_id: String,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub metadata: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerEventEnvelope {
    pub schema_version: String,
    #[serde(default)]
    pub event_id: Option<String>,
    #[serde(rename = "type")]
    pub event_type: String,
    #[specta(type = specta_typescript::Number)]
    pub sequence_number: u64,
    pub occurred_at: String,
    pub optimizer_run_id: String,
    pub algorithm_id: String,
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub item: Option<Value>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub delta: Map<String, Value>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub snapshot: Option<Map<String, Value>>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub usage_delta: Option<Map<String, Value>>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub artifact_refs: Vec<Value>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub error: Option<Value>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub raw: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerStateSlice {
    pub schema_version: String,
    pub projection_schema_version: String,
    pub run_id: String,
    pub algorithm_id: String,
    pub slice_id: String,
    #[specta(type = specta_typescript::Number)]
    pub cursor_seq: u64,
    pub updated_at: String,
    #[specta(type = specta_typescript::Unknown)]
    pub data: Value,
}

#[derive(Clone, Debug, Default, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerQuery {
    pub status: Option<String>,
    pub algorithm_id: Option<String>,
    pub source: Option<String>,
    pub search: Option<String>,
    pub session_ref: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub limit: Option<i64>,
    #[specta(type = specta_typescript::Number)]
    pub offset: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerCreateRequest {
    pub algorithm_id: String,
    #[serde(default)]
    pub algorithm_version: Option<String>,
    #[serde(default)]
    pub objective: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub project_ref: Option<String>,
    #[serde(default)]
    pub session_ref: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub execution_bindings: Option<Vec<OptimizerExecutionBinding>>,
    #[serde(default)]
    pub input_refs: Option<Vec<OptimizerResourceRef>>,
    #[serde(default)]
    pub capabilities: Option<OptimizerCapabilities>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub summary: Option<Value>,
    #[serde(default)]
    pub open_visual: Option<bool>,
    #[serde(default)]
    pub seed_fixture: Option<String>,
    /// Cloud create payload: `{ config_toml }` or `{ config_json }`.
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub cloud_config: Option<Value>,
    /// Import a local OSS / optimizers-beta workspace or events file.
    #[serde(default)]
    pub local_path: Option<String>,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerImportLocalRequest {
    pub path: String,
    #[serde(default)]
    pub session_ref: Option<String>,
    #[serde(default)]
    pub open_visual: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerReconcileRequest {
    pub optimizer_run_id: String,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub after_seq: Option<u64>,
    #[serde(default)]
    pub open_visual: Option<bool>,
}

/// Starts one of the product-owned, bounded optimizer recipes. Recipe inputs
/// deliberately do not include commands, paths, environment variables, or
/// credentials: those are resolved by the Rust host. An optional `base_model`
/// must be an id from `docs/sft_tinker_base_models.toml`; omitted uses that
/// file's default.
#[derive(Clone, Debug, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerRecipeRunRequest {
    pub recipe_id: String,
    #[serde(default)]
    pub session_ref: Option<String>,
    #[serde(default)]
    pub open_visual: Option<bool>,
    /// Tinker student id from `docs/sft_tinker_base_models.toml`. Omitted uses that file's default.
    #[serde(default)]
    pub base_model: Option<String>,
    /// Allowlisted dataset shard id. Ignored except on recipes that publish
    /// `limits.datasetShards`. Selecting a shard is not supplying a path.
    #[serde(default)]
    pub dataset_shard: Option<String>,
    /// Immutable candidate set staged before the run. Required by `eval.*`
    /// recipes and ignored elsewhere. This is an id, never a path: policy
    /// source is content-addressed at staging time, not at launch.
    #[serde(default)]
    pub candidate_set_id: Option<String>,
    /// Explicit registered-container identity for a container-backed baseline
    /// evaluation. This is an opaque host id, never a URL or a user-provided
    /// path. When multiple healthy pools advertise the same family, omission
    /// fails closed rather than selecting whichever probe happened last.
    #[serde(default)]
    pub container_id: Option<String>,
    /// Managed training artifact to evaluate. When set, Workshop stages an
    /// `mlx-lora.v1` candidate set from that record and retains its identity
    /// on the Eval receipt. Mutually exclusive with `candidate_set_id`.
    #[serde(default)]
    pub training_artifact_id: Option<String>,
    /// Trusted recipe subset selection. The optimizer worker validates that
    /// every candidate, seed, model, and effort only narrows the recipe.
    #[serde(default)]
    pub plan_override: Option<Value>,
    /// Optional GEPA search overrides. Omitted fields keep the recipe defaults.
    /// `proposalsPerGeneration` is capped at 10; `policyConcurrency` at 120.
    #[serde(default)]
    pub search: Option<OptimizerSearchOverrides>,
}

#[derive(Clone, Debug, Default, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerSearchOverrides {
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub proposals_per_generation: Option<i64>,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub max_in_flight_candidates: Option<i64>,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub policy_concurrency: Option<i64>,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub rollout_concurrency: Option<i64>,
}

#[cfg(test)]
mod status_contract_tests {
    use super::OptimizerRunStatus;
    use std::collections::BTreeSet;

    /// Files that still spell an optimizer run status by hand rather than
    /// asking [`OptimizerRunStatus`]. Shrink-only: a file that stops offending
    /// must be removed from this list, so the debt cannot quietly grow back
    /// under a name that is already excused.
    ///
    /// Empty on purpose. The original P0-2 done-when grep
    /// (`"succeeded"|"canceled"|"done"|"aborted"` under `src/optimizers`) is
    /// unsatisfiable as written: sidecar *training-job* status in
    /// `sidecar_training.rs`, `local.rs`, and `sft_result.rs` is a second
    /// vocabulary (proposed P0-15). This lock is the run-domain slice — it
    /// only inspects [`RUN_RECEIVERS`] — so those files do not belong here.
    const HANDWRITTEN_STATUS_ALLOWLIST: &[&str] = &[];

    /// Sidecar training-job status files. Not the run domain. P0-15.
    const TRAINING_JOB_STATUS_FILES: &[&str] = &["sidecar_training.rs", "local.rs"];

    /// Receivers that are an `OptimizerRunRecord`. Deliberately explicit: the
    /// sidecar training *job* and the plugin *service* both have a `status`
    /// field with a different vocabulary, and folding them in here would either
    /// fail the test or force the two vocabularies to merge.
    const RUN_RECEIVERS: &[&str] = &[
        "run",
        "current",
        "stored",
        "durable",
        "record",
        "projected_run",
        "existing",
        "seed",
    ];

    fn optimizers_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/optimizers")
    }

    /// Balanced-paren body of the `macro!(` that starts at `open`.
    fn macro_body(source: &str, open: usize) -> Option<&str> {
        let bytes = source.as_bytes();
        let mut depth = 0usize;
        for (offset, byte) in bytes.iter().enumerate().skip(open) {
            match byte {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&source[open + 1..offset]);
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn is_run_receiver(expression: &str) -> bool {
        let expression = expression.trim().trim_end_matches(".as_str()");
        expression
            .strip_suffix(".status")
            .is_some_and(|receiver| RUN_RECEIVERS.contains(&receiver.trim()))
    }

    /// Every status word compared against, assigned to, or matched on an
    /// optimizer run record is one the enum knows, spelled canonically.
    ///
    /// This reads source rather than types because the field is still a
    /// `String` at the storage edge: the hazard is a new literal, and only a
    /// textual check sees one arrive.
    #[test]
    fn every_run_status_literal_is_in_the_enum() {
        let mut offenders: Vec<String> = Vec::new();
        let mut allowlisted_files: BTreeSet<String> = BTreeSet::new();
        let mut scanned = 0usize;
        let mut checked = 0usize;

        for entry in std::fs::read_dir(optimizers_dir()).expect("read src/optimizers") {
            let path = entry.expect("dir entry").path();
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_owned();
            if !name.ends_with(".rs") || name == "models.rs" {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("read optimizer source");
            scanned += 1;
            let mut found: Vec<(usize, String)> = Vec::new();

            // `<run>.status == "..."`, `!= "..."`, `= "..."`.
            for receiver in RUN_RECEIVERS {
                let needle = format!("{receiver}.status");
                for (offset, _) in source.match_indices(&needle) {
                    if offset > 0 {
                        let previous = source[..offset].chars().next_back().unwrap_or(' ');
                        if previous.is_alphanumeric() || previous == '_' {
                            continue;
                        }
                    }
                    let mut tail = &source[offset + needle.len()..];
                    tail = tail.strip_prefix(".as_str()").unwrap_or(tail);
                    let tail = tail.trim_start();
                    let rest = ["==", "!=", "="]
                        .iter()
                        .find_map(|operator| tail.strip_prefix(*operator));
                    let Some(rest) = rest else { continue };
                    let rest = rest.trim_start();
                    if let Some(literal) = rest.strip_prefix('"') {
                        if let Some(end) = literal.find('"') {
                            found.push((offset, literal[..end].to_owned()));
                        }
                    }
                }
            }

            // `matches!(<run>.status(.as_str())?, "..." | "...")`.
            for (offset, _) in source.match_indices("matches!(") {
                let open = offset + "matches!".len();
                let Some(body) = macro_body(&source, open) else {
                    continue;
                };
                let Some((head, arms)) = body.split_once(',') else {
                    continue;
                };
                if !is_run_receiver(head) {
                    continue;
                }
                let mut rest = arms;
                while let Some(quote) = rest.find('"') {
                    let literal = &rest[quote + 1..];
                    let Some(end) = literal.find('"') else { break };
                    found.push((offset, literal[..end].to_owned()));
                    rest = &literal[end + 1..];
                }
            }

            for (offset, literal) in found {
                if literal.contains('{') || literal.is_empty() {
                    continue;
                }
                checked += 1;
                if OptimizerRunStatus::parse(&literal)
                    .is_some_and(|status| status.as_str() == literal)
                {
                    continue;
                }
                if HANDWRITTEN_STATUS_ALLOWLIST.contains(&name.as_str()) {
                    allowlisted_files.insert(name.clone());
                    continue;
                }
                let line = source[..offset].matches('\n').count() + 1;
                offenders.push(format!(
                    "{name}:{line} writes run status {literal:?}, which is not an \
                     OptimizerRunStatus spelling"
                ));
            }
        }

        assert!(
            scanned > 10,
            "expected to scan the optimizer module, saw {scanned} files"
        );
        assert!(
            checked > 10,
            "expected to check real status literals, checked {checked}"
        );
        assert!(
            offenders.is_empty(),
            "optimizer run status is OptimizerRunStatus, not a string:\n  {}",
            offenders.join("\n  ")
        );
        let unused: Vec<&&str> = HANDWRITTEN_STATUS_ALLOWLIST
            .iter()
            .filter(|name| !allowlisted_files.contains(**name))
            .collect();
        assert!(
            unused.is_empty(),
            "these files no longer spell a status by hand; remove them from \
             HANDWRITTEN_STATUS_ALLOWLIST: {unused:?}"
        );
    }

    /// Backend P0-3's done-when, handed to workshop: the hosted-SFT mirror
    /// must not keep a second `succeeded → completed` fold. `parse` is the one.
    #[test]
    fn hosted_sft_does_not_map_succeeded_onto_completed() {
        let source = include_str!("hosted_sft.rs");
        assert!(
            !source.contains("\"succeeded\" => \"completed\""),
            "persist_remote_terminal must not keep a second succeeded→completed fold"
        );
    }

    /// The original P0-2 done-when grep is a second vocabulary, not leftover
    /// run-status aliases. These files must keep speaking training-job status
    /// until P0-15; if one stops, drop it from the list (shrink-only).
    #[test]
    fn training_job_status_vocabulary_is_still_a_second_domain() {
        for name in TRAINING_JOB_STATUS_FILES {
            let source = std::fs::read_to_string(optimizers_dir().join(name)).expect(name);
            let speaks_job_status = source.contains("job.status")
                || source.contains("\"succeeded\"")
                || source.contains("\"canceled\"");
            assert!(
                speaks_job_status,
                "{name} was listed as P0-15 training-job vocabulary but no longer speaks it; \
                 remove it from TRAINING_JOB_STATUS_FILES"
            );
        }
    }

    /// The database refuses exactly the words the enum refuses.
    ///
    /// Two authorities for one vocabulary is the failure this whole item
    /// deletes; the migration text and the enum must not be able to disagree.
    #[test]
    fn migration_28_status_domain_equals_the_enum() {
        let source = include_str!("../storage/migrations.rs");
        let migration = source
            .split("const MIGRATION_28: &str = r#\"")
            .nth(1)
            .expect("MIGRATION_28 is defined")
            .split("\"#;")
            .next()
            .expect("MIGRATION_28 is terminated");
        let domains: Vec<BTreeSet<String>> = migration
            .match_indices("NOT IN (")
            .map(|(offset, _)| {
                let tail = &migration[offset + "NOT IN (".len()..];
                let inner = &tail[..tail.find(')').expect("closed NOT IN list")];
                inner
                    .split(',')
                    .map(|word| word.trim().trim_matches('\'').to_owned())
                    .collect()
            })
            .collect();
        assert_eq!(
            domains.len(),
            3,
            "expected the data pass plus the insert and update triggers"
        );
        let expected: BTreeSet<String> = OptimizerRunStatus::ALL
            .iter()
            .map(|status| status.as_str().to_owned())
            .collect();
        for domain in &domains {
            assert_eq!(
                domain, &expected,
                "the migration-28 status domain must be OptimizerRunStatus::ALL"
            );
        }
    }

    /// Every alias the migration folds is an alias the enum accepts, and folds
    /// onto the same canonical word.
    #[test]
    fn migration_28_alias_pass_matches_the_enum() {
        for (alias, canonical) in [
            ("succeeded", "completed"),
            ("canceled", "cancelled"),
            ("done", "failed"),
            ("stopped", "failed"),
            ("aborted", "failed"),
            ("error", "failed"),
            ("created", "queued"),
        ] {
            assert_eq!(
                OptimizerRunStatus::parse(alias).map(OptimizerRunStatus::as_str),
                Some(canonical),
                "alias {alias} must normalize to {canonical}"
            );
        }
    }

    #[test]
    fn terminal_is_one_set() {
        let terminal: Vec<&str> = OptimizerRunStatus::ALL
            .iter()
            .filter(|status| status.is_terminal())
            .map(|status| status.as_str())
            .collect();
        assert_eq!(
            terminal,
            vec![
                "degraded",
                "completed",
                "failed",
                "failed_evidence",
                "cancelled",
                "interrupted",
                "infrastructure_lost",
                "cap_reached",
            ]
        );
        // An unrecognised word never reads as finished — and never as running.
        assert!(!OptimizerRunStatus::str_is_terminal("who_knows"));
        assert!(OptimizerRunStatus::parse("who_knows").is_none());
        // Legacy spellings still settle.
        assert!(OptimizerRunStatus::str_is_terminal("succeeded"));
        assert!(OptimizerRunStatus::str_is_terminal("canceled"));
    }

    #[test]
    fn terminal_states_do_not_reopen() {
        for status in OptimizerRunStatus::ALL.iter().filter(|s| s.is_terminal()) {
            assert!(!status.can_transition_to(OptimizerRunStatus::Running));
            assert!(status.can_transition_to(*status));
        }
        assert!(OptimizerRunStatus::Running.can_transition_to(OptimizerRunStatus::Paused));
        assert!(!OptimizerRunStatus::Queued.can_transition_to(OptimizerRunStatus::Paused));
        assert!(OptimizerRunStatus::Paused.can_transition_to(OptimizerRunStatus::Running));
    }
}
