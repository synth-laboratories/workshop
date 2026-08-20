use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const OPTIMIZER_RUN_SCHEMA_VERSION: &str = "optimizer_run.v1";
pub const OPTIMIZER_EVENT_SCHEMA_VERSION: &str = "optimizer_event.v1";
pub const OPTIMIZER_STATE_SLICE_SCHEMA_VERSION: &str = "optimizer_state_slice.v1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all(deserialize = "snake_case", serialize = "camelCase"))]
pub struct SavedLoraStorage {
    pub backend: String,
    pub bucket: String,
    pub key: String,
    pub version: Option<String>,
    pub etag: Option<String>,
    pub sha256: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub size_bytes: Option<u64>,
    pub content_type: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all(deserialize = "snake_case", serialize = "camelCase"))]
pub struct SavedLoraLineage {
    pub optimizer_algorithm: Option<String>,
    pub run_id: Option<String>,
    pub attempt_id: Option<String>,
    pub source_checkpoint_id: Option<String>,
    pub provider_checkpoint_reference: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all(deserialize = "snake_case", serialize = "camelCase"))]
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
    #[specta(type = specta_typescript::Unknown)]
    pub step: Option<u64>,
    pub status: String,
    pub storage: SavedLoraStorage,
    #[serde(default)]
    pub lineage: SavedLoraLineage,
    pub tags: Vec<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub metadata: Value,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    pub archived_at: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all(deserialize = "snake_case", serialize = "camelCase"))]
pub struct SavedLoraCheckpointPage {
    pub schema_version: String,
    pub items: Vec<SavedLoraCheckpoint>,
    #[specta(type = specta_typescript::Unknown)]
    pub total: u64,
    #[specta(type = specta_typescript::Unknown)]
    pub limit: u64,
    #[specta(type = specta_typescript::Unknown)]
    pub offset: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all(deserialize = "snake_case", serialize = "camelCase"))]
pub struct SavedLoraRunIdentity {
    pub run_id: String,
    pub attempt_id: Option<String>,
    pub optimizer_algorithm: String,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all(deserialize = "snake_case", serialize = "camelCase"))]
pub struct SavedLoraRunCounts {
    #[specta(type = specta_typescript::Unknown)]
    pub total: u64,
    #[specta(type = specta_typescript::Unknown)]
    pub inference: u64,
    #[specta(type = specta_typescript::Unknown)]
    pub training: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all(deserialize = "snake_case", serialize = "camelCase"))]
pub struct SavedLoraRunPage {
    pub schema_version: String,
    pub run: SavedLoraRunIdentity,
    pub items: Vec<SavedLoraCheckpoint>,
    pub counts: SavedLoraRunCounts,
    #[specta(type = specta_typescript::Unknown)]
    pub total: u64,
    #[specta(type = specta_typescript::Unknown)]
    pub limit: u64,
    #[specta(type = specta_typescript::Unknown)]
    pub offset: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all(deserialize = "snake_case", serialize = "camelCase"))]
pub struct OptimizerRunOutputIdentity {
    pub run_id: String,
    pub attempt_id: Option<String>,
    pub optimizer_algorithm: String,
    pub status: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all(deserialize = "snake_case", serialize = "camelCase"))]
pub struct OptimizerRunOutputArtifact {
    pub artifact_id: String,
    pub run_id: String,
    pub artifact_name: String,
    pub content_type: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
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
#[serde(rename_all(deserialize = "snake_case", serialize = "camelCase"))]
pub struct OptimizerRunOutputCounts {
    #[specta(type = specta_typescript::Unknown)]
    pub artifacts: u64,
    #[specta(type = specta_typescript::Unknown)]
    pub model_checkpoints: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all(deserialize = "snake_case", serialize = "camelCase"))]
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
    pub provider: Option<String>,
    pub checkpoint_kind: Option<String>,
    pub base_model: Option<String>,
    pub run_id: Option<String>,
    pub attempt_id: Option<String>,
    pub source_checkpoint_id: Option<String>,
    pub optimizer_algorithm: Option<String>,
    pub status: Option<String>,
    pub tags: Option<Vec<String>>,
    #[specta(type = specta_typescript::Unknown)]
    pub limit: Option<u64>,
    #[specta(type = specta_typescript::Unknown)]
    pub offset: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all(deserialize = "snake_case", serialize = "camelCase"))]
pub struct SavedLoraDownload {
    pub checkpoint_id: String,
    pub url: String,
    #[specta(type = specta_typescript::Unknown)]
    pub expires_in: u64,
    pub content_type: String,
    #[specta(type = specta_typescript::Unknown)]
    pub size_bytes: Option<u64>,
    pub sha256: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all(deserialize = "snake_case", serialize = "camelCase"))]
pub struct HostedTrainingModel {
    pub model_id: String,
    pub label: String,
    pub provider: String,
    pub provider_revision: String,
    pub architecture: String,
    #[specta(type = specta_typescript::Unknown)]
    pub max_context_length: u64,
    #[specta(type = specta_typescript::Unknown)]
    pub rank: Value,
    #[specta(type = specta_typescript::Unknown)]
    pub algorithms: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all(deserialize = "snake_case", serialize = "camelCase"))]
pub struct HostedTrainingModelCatalog {
    pub schema_version: String,
    pub catalog_revision: String,
    pub live_preflight_required: bool,
    pub models: Vec<HostedTrainingModel>,
    #[specta(type = specta_typescript::Unknown)]
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
    #[specta(type = specta_typescript::Unknown)]
    pub prompt_tokens: u64,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub completion_tokens: u64,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub rollouts: u64,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub algorithm_version: Option<String>,
    pub status: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objective: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_ref: Option<String>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    #[serde(rename = "type")]
    pub event_type: String,
    #[specta(type = specta_typescript::Unknown)]
    pub sequence_number: u64,
    pub occurred_at: String,
    pub optimizer_run_id: String,
    pub algorithm_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[specta(type = specta_typescript::Unknown)]
    pub item: Option<Value>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub delta: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[specta(type = specta_typescript::Unknown)]
    pub snapshot: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[specta(type = specta_typescript::Unknown)]
    pub usage_delta: Option<Map<String, Value>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[specta(type = specta_typescript::Unknown)]
    pub artifact_refs: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
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
    #[specta(type = specta_typescript::Unknown)]
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
    #[specta(type = specta_typescript::Unknown)]
    pub limit: Option<i64>,
    #[specta(type = specta_typescript::Unknown)]
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
    #[specta(type = specta_typescript::Unknown)]
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
    /// Tinker `create_lora_training_client(base_model=...)` id. Ignored except
    /// on the Craftax hosted SFT recipe. Must be in `docs/sft_tinker_base_models.toml`.
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
    /// Managed training artifact to evaluate. When set, Workshop stages an
    /// `mlx-lora.v1` candidate set from that record and retains its identity
    /// on the Eval receipt. Mutually exclusive with `candidate_set_id`.
    #[serde(default)]
    pub training_artifact_id: Option<String>,
    /// Optional GEPA search overrides. Omitted fields keep the recipe defaults.
    /// `proposalsPerGeneration` is capped at 10; `policyConcurrency` at 120.
    #[serde(default)]
    pub search: Option<OptimizerSearchOverrides>,
}

#[derive(Clone, Debug, Default, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OptimizerSearchOverrides {
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub proposals_per_generation: Option<i64>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub max_in_flight_candidates: Option<i64>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub policy_concurrency: Option<i64>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub rollout_concurrency: Option<i64>,
}
