use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

pub const OPTIMIZER_RUN_SCHEMA_VERSION: &str = "optimizer_run.v1";
pub const OPTIMIZER_EVENT_SCHEMA_VERSION: &str = "optimizer_event.v1";
pub const OPTIMIZER_STATE_SLICE_SCHEMA_VERSION: &str = "optimizer_state_slice.v1";

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
            "sft" => Self {
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
}
