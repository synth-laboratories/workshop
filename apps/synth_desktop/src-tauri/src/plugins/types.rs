//! Canonical plugin status and action-receipt types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PLUGIN_STATUS_SCHEMA: &str = "synth.plugin-status.v1";
pub const PLUGIN_ACTION_RECEIPT_SCHEMA: &str = "synth.plugin-action-receipt.v1";
pub const PLUGIN_NOT_READY_CODE: &str = "plugin_not_ready";
pub const OPTIMIZERS_PLUGIN_ID: &str = "optimizers";
pub const PLUGIN_PUBLISHER: &str = "Synth Laboratories";

pub const PLUGIN_PHASES: [&str; 13] = [
    "not_installed",
    "downloading",
    "verifying",
    "installed",
    "starting",
    "ready",
    "stopping",
    "stopped",
    "updating",
    "removing",
    "degraded",
    "error",
    "disabled",
];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginServiceStatus {
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default)]
    pub active_runs: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginStatus {
    pub schema_version: String,
    pub plugin_id: String,
    pub enabled: bool,
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    pub service: PluginServiceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities_digest: Option<String>,
    #[serde(default)]
    pub algorithms: Vec<String>,
    #[serde(default)]
    pub templates: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_action_receipt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginActionReceipt {
    pub schema_version: String,
    pub receipt_id: String,
    pub plugin_id: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_receipt_id: Option<String>,
    pub started_at: String,
    pub finished_at: String,
    pub result: String,
    pub retained_data: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<PluginStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PluginNotReady {
    pub code: String,
    pub plugin_id: String,
    pub phase: String,
    pub suggested_operation: String,
}

impl PluginNotReady {
    pub fn new(phase: impl Into<String>, suggested: impl Into<String>) -> Self {
        Self {
            code: PLUGIN_NOT_READY_CODE.into(),
            plugin_id: OPTIMIZERS_PLUGIN_ID.into(),
            phase: phase.into(),
            suggested_operation: suggested.into(),
        }
    }

    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| {
            serde_json::json!({
                "code": PLUGIN_NOT_READY_CODE,
                "pluginId": OPTIMIZERS_PLUGIN_ID,
            })
        })
    }
}

impl std::fmt::Display for PluginNotReady {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}",
            serde_json::to_string(&self.to_json()).unwrap_or_else(|_| self.code.clone())
        )
    }
}

impl std::error::Error for PluginNotReady {}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub plugin_id: String,
    pub version: String,
    pub publisher: String,
    pub package: String,
    pub network_host: String,
    pub download_size_bytes: u64,
    pub workshop_compat: String,
    pub algorithms: Vec<String>,
    pub templates: Vec<String>,
    pub recipe_schema_version: String,
    pub bounded_recipes: Vec<String>,
}

pub fn digest_ref(hex: &str) -> String {
    let trimmed = hex.trim();
    if trimmed.starts_with("sha256:") {
        trimmed.to_owned()
    } else {
        format!("sha256:{trimmed}")
    }
}

pub fn redact_secrets(value: &str) -> String {
    let mut out = value.to_owned();
    for needle in [
        "http://127.0.0.1:",
        "http://localhost:",
        "synth-opt-",
        "Bearer ",
        "SYNTH_OPTIMIZER_API_KEY",
    ] {
        if let Some(index) = out.find(needle) {
            let rest = &out[index + needle.len()..];
            let cut = rest
                .find(|ch: char| ch.is_whitespace() || ch == '"' || ch == '\'')
                .unwrap_or(rest.len());
            out.replace_range(index..index + needle.len() + cut, "[redacted]");
        }
    }
    out
}
