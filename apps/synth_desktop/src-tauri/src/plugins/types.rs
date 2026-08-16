//! Canonical plugin status and action-receipt types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PLUGIN_STATUS_SCHEMA: &str = "synth.plugin-status.v1";
pub const PLUGIN_ACTION_RECEIPT_SCHEMA: &str = "synth.plugin-action-receipt.v1";
pub const PLUGIN_NOT_READY_CODE: &str = "plugin_not_ready";
pub const OPTIMIZERS_PLUGIN_ID: &str = "optimizers";
pub const PLUGIN_PUBLISHER: &str = "Synth Laboratories";
pub const OFFICIAL_RELEASE_CHANNEL: &str = "official";
pub const DEV_RELEASE_CHANNEL: &str = "dev";

/// Installed, but the operating system has not granted something the plugin
/// needs. Distinct from `degraded`: nothing is broken and nothing here can fix
/// it — a person has to say yes in System Settings. Distinct from `installed`,
/// which would invite `start`, and from `ready`, which would be a lie.
pub const PHASE_NEEDS_PERMISSIONS: &str = "needs_permissions";

pub const PLUGIN_PHASES: [&str; 14] = [
    "not_installed",
    "downloading",
    "verifying",
    "installed",
    PHASE_NEEDS_PERMISSIONS,
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

/// Grant states as macOS reports them. `not_applicable` covers grants that are
/// asked per-target at first use — Apple Events is per-app — and therefore have
/// no single global answer to display.
pub const PLUGIN_PERMISSION_STATES: [&str; 4] =
    ["granted", "denied", "not_determined", "not_applicable"];

/// One row in the permission list: what the OS was asked for, what it said, and
/// where the operator goes to change it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct PluginPermission {
    /// Stable identifier, e.g. `accessibility`, `screen_recording`.
    pub id: String,
    /// What macOS itself calls this in System Settings. Matching its wording is
    /// what makes the row findable; our own name for it would not be.
    pub label: String,
    /// One of [`PLUGIN_PERMISSION_STATES`].
    pub state: String,
    /// Deep link to the exact Privacy & Security pane, where one exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settings_url: Option<String>,
    /// Why the plugin needs it, in one line. A reason, not a pitch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
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
    pub release_channel: String,
    pub catalog_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    pub service: PluginServiceStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities_digest: Option<String>,
    #[serde(default)]
    pub algorithms: Vec<String>,
    #[serde(default)]
    pub templates: Vec<String>,
    /// OS grants this plugin holds. Empty for plugins that need none, which is
    /// most of them — hence a list rather than an `Option`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub permissions: Vec<PluginPermission>,
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
    /// Permission ids the caller is missing, when `phase` is
    /// `needs_permissions`. Naming them is what separates a refusal an agent
    /// can act on from one it can only relay.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub missing_permissions: Vec<String>,
}

impl PluginNotReady {
    pub fn for_plugin(
        plugin_id: impl Into<String>,
        phase: impl Into<String>,
        suggested: impl Into<String>,
    ) -> Self {
        Self {
            code: PLUGIN_NOT_READY_CODE.into(),
            plugin_id: plugin_id.into(),
            phase: phase.into(),
            suggested_operation: suggested.into(),
            missing_permissions: Vec::new(),
        }
    }

    pub fn new(phase: impl Into<String>, suggested: impl Into<String>) -> Self {
        Self::for_plugin(OPTIMIZERS_PLUGIN_ID, phase, suggested)
    }

    /// G4: refuse with the exact grant that is missing, not with "not ready".
    pub fn missing(mut self, permissions: impl IntoIterator<Item = String>) -> Self {
        self.missing_permissions = permissions.into_iter().collect();
        self
    }

    pub fn to_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| {
            serde_json::json!({
                "code": PLUGIN_NOT_READY_CODE,
                "pluginId": self.plugin_id,
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

/// Catalog fields that mean something to exactly one plugin.
///
/// These were columns on [`CatalogEntry`]. Nothing reads them — the approval
/// card is built from publisher, version, digest, size, and host — so leaving
/// them as columns would only have forced plugin #2 to describe itself with
/// four empty optimizer fields.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CatalogPayload {
    #[serde(rename_all = "camelCase")]
    Optimizers {
        algorithms: Vec<String>,
        templates: Vec<String>,
        recipe_schema_version: String,
        bounded_recipes: Vec<String>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub plugin_id: String,
    pub release_channel: String,
    pub version: String,
    pub publisher: String,
    pub package: String,
    pub network_host: String,
    pub download_size_bytes: u64,
    pub workshop_compat: String,
    pub payload: CatalogPayload,
}

pub fn digest_ref(hex: &str) -> String {
    let trimmed = hex.trim();
    if trimmed.starts_with("sha256:") {
        trimmed.to_owned()
    } else {
        format!("sha256:{trimmed}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The renderer keeps its own copy of the phase list, because the bridge
    /// DTOs are hand-mirrored rather than generated. A phase added here and not
    /// labelled there renders as a raw identifier in the sidebar — `PHASE_LABELS`
    /// falls through to the phase string, so the failure looks like a typo in
    /// the product rather than a missing case.
    #[test]
    fn every_phase_has_a_renderer_label() {
        let presentation =
            include_str!("../../../src/renderer/src/runtime/pluginPresentation.ts");
        for phase in PLUGIN_PHASES {
            assert!(
                presentation.contains(&format!("{phase}:")),
                "`{phase}` has no label in pluginPresentation.ts"
            );
        }
    }

    /// `needs_permissions` must sit between install and start: it is reachable
    /// only once the binary exists, and it blocks `start`.
    #[test]
    fn needs_permissions_sits_between_installed_and_starting() {
        // Unwrapped, not compared as Options: `None < Some(_)`, so a missing
        // phase would satisfy the ordering instead of failing the test.
        let position = |phase: &str| {
            PLUGIN_PHASES
                .iter()
                .position(|item| *item == phase)
                .unwrap_or_else(|| panic!("`{phase}` is not a declared phase"))
        };
        assert!(position("installed") < position(PHASE_NEEDS_PERMISSIONS));
        assert!(position(PHASE_NEEDS_PERMISSIONS) < position("starting"));
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
