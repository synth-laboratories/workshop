use crate::domain::{InternBinding, InternMode, RuntimeTarget};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const APP_EVENT_SCHEMA_VERSION: &str = "synth.desktop-app-event.v1";
/// Matches `storage/migrations.rs` `MIGRATIONS.len()`.
pub const SCHEMA_VERSION: i64 = 16;

fn default_session_kind() -> String {
    "codex".into()
}

/// Codegen-only mirror of RuntimeTarget's custom serde wire representation.
#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind")]
pub enum RuntimeTargetContract {
    #[serde(rename = "local")]
    Local {
        model: String,
        adapter: Option<String>,
    },
    #[serde(rename = "remote")]
    Remote {
        model: String,
        adapter: Option<String>,
    },
    #[serde(rename = "cloud")]
    Cloud {
        model: String,
        adapter: Option<String>,
    },
    #[serde(rename = "intern")]
    Intern {
        mode: InternMode,
        binding: Option<InternBinding>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum EventSource {
    Local,
    Remote,
    Intern,
    Codex,
    System,
    Mlx,
    Visual,
    Report,
}

impl EventSource {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Remote => "remote",
            Self::Intern => "intern",
            Self::Codex => "codex",
            Self::System => "system",
            Self::Mlx => "mlx",
            Self::Visual => "visual",
            Self::Report => "report",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "remote" => Self::Remote,
            "intern" => Self::Intern,
            "codex" => Self::Codex,
            "system" => Self::System,
            "mlx" => Self::Mlx,
            "visual" => Self::Visual,
            "report" => Self::Report,
            _ => Self::Local,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AppEvent {
    pub schema_version: String,
    #[specta(type = specta_typescript::Number)]
    pub sequence: i64,
    pub event_id: String,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub session_sequence: Option<i64>,
    #[serde(default)]
    pub run_id: Option<String>,
    pub source: EventSource,
    pub kind: String,
    #[specta(type = specta_typescript::Unknown)]
    pub payload: Value,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub remote_sequence: Option<i64>,
    #[serde(default)]
    pub command_id: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub id: String,
    pub title: String,
    /// SessionKind as a DB/wire string (`codex` | `intern`). Prefer
    /// `SessionKind::parse` at call sites — do not re-read `target.kind`.
    #[serde(default = "default_session_kind")]
    pub kind: String,
    /// Typed runtime substrate (DB column remains `target_json`).
    #[specta(type = RuntimeTargetContract)]
    pub target: RuntimeTarget,
    pub project_id: Option<String>,
    pub remote_id: Option<String>,
    pub codex_thread_id: Option<String>,
    pub status: String,
    #[specta(type = specta_typescript::Number)]
    pub state_generation: Option<i64>,
    #[specta(type = specta_typescript::Number)]
    pub latest_cursor: i64,
    pub active_run_id: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub metadata: Value,
    pub created_at: String,
    pub updated_at: String,
}

impl SessionRecord {
    /// Opaque JSON bag for call sites that still need `Value` (prefer `target`).
    pub fn target_json(&self) -> Value {
        self.target.to_json_value()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub id: String,
    pub session_id: String,
    pub mode: String,
    pub status: String,
    #[specta(type = specta_typescript::Number)]
    pub latest_cursor: i64,
    #[specta(type = specta_typescript::Unknown)]
    pub checkpoint: Option<Value>,
    #[specta(type = specta_typescript::Unknown)]
    pub outcome: Option<Value>,
    pub model: Option<String>,
    pub adapter: Option<String>,
    #[specta(type = specta_typescript::Unknown)]
    pub metadata: Value,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CommandReceiptRecord {
    pub command_id: String,
    pub session_id: String,
    pub run_id: Option<String>,
    pub source: EventSource,
    pub kind: String,
    pub status: String,
    #[specta(type = specta_typescript::Unknown)]
    pub request: Value,
    #[specta(type = specta_typescript::Unknown)]
    pub response: Option<Value>,
    #[specta(type = specta_typescript::Number)]
    pub remote_cursor: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CoreDiagnostics {
    pub database_path: String,
    #[specta(type = specta_typescript::Number)]
    pub schema_version: i64,
    pub integrity_ok: bool,
    pub content_store_path: String,
    #[specta(type = specta_typescript::Number)]
    pub journal_head: i64,
    #[specta(type = specta_typescript::Number)]
    pub session_count: i64,
    #[specta(type = specta_typescript::Number)]
    pub run_count: i64,
    #[specta(type = specta_typescript::Number)]
    pub visual_count: i64,
    pub migration_complete: bool,
}
