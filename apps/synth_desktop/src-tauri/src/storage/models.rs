use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const APP_EVENT_SCHEMA_VERSION: &str = "synth.desktop-app-event.v1";
pub const SCHEMA_VERSION: i64 = 7;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum EventSource {
    Local,
    Remote,
    Intern,
    Codex,
    System,
    Mlx,
    Visual,
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
            _ => Self::Local,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppEvent {
    pub schema_version: String,
    pub sequence: i64,
    pub event_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_sequence: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    pub source: EventSource,
    pub kind: String,
    pub payload: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_sequence: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_id: Option<String>,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub id: String,
    pub title: String,
    pub target_json: Value,
    pub project_id: Option<String>,
    pub remote_id: Option<String>,
    pub codex_thread_id: Option<String>,
    pub status: String,
    pub state_generation: Option<i64>,
    pub latest_cursor: i64,
    pub active_run_id: Option<String>,
    pub metadata: Value,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RunRecord {
    pub id: String,
    pub session_id: String,
    pub mode: String,
    pub status: String,
    pub latest_cursor: i64,
    pub checkpoint: Option<Value>,
    pub outcome: Option<Value>,
    pub model: Option<String>,
    pub adapter: Option<String>,
    pub metadata: Value,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CommandReceiptRecord {
    pub command_id: String,
    pub session_id: String,
    pub run_id: Option<String>,
    pub source: EventSource,
    pub kind: String,
    pub status: String,
    pub request: Value,
    pub response: Option<Value>,
    pub remote_cursor: Option<i64>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoreDiagnostics {
    pub database_path: String,
    pub schema_version: i64,
    pub integrity_ok: bool,
    pub content_store_path: String,
    pub journal_head: i64,
    pub session_count: i64,
    pub run_count: i64,
    pub visual_count: i64,
    pub migration_complete: bool,
}
