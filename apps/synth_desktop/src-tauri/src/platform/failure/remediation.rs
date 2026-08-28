//! Typed remediation actions. UI buttons are generated from these variants.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum SettingsRoute {
    Secrets,
    Containers,
    Models,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum ResourceRef {
    Containers,
    Evaluations,
    Visuals,
    Sessions,
    Approvals,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RepairRequest {
    pub target: String,
    pub action: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureRemediation {
    ApproveRestart { container_id: String },
    ApproveResume { session_id: String },
    Retry { resume_token: String },
    Repair(RepairRequest),
    OpenSettings(SettingsRoute),
    OpenResource(ResourceRef),
    OpenDiagnostics,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct FailureRemediationView {
    pub kind: String,
    pub label: String,
    pub container_id: Option<String>,
    pub session_id: Option<String>,
    pub resume_token: Option<String>,
    pub settings_route: Option<String>,
    pub resource_ref: Option<String>,
}

impl FailureRemediation {
    pub fn view(&self) -> FailureRemediationView {
        match self {
            Self::ApproveRestart { container_id } => FailureRemediationView {
                kind: "approve".into(),
                label: "Restart container".into(),
                container_id: Some(container_id.clone()),
                session_id: None,
                resume_token: None,
                settings_route: None,
                resource_ref: None,
            },
            Self::ApproveResume { session_id } => FailureRemediationView {
                kind: "approve".into(),
                label: "Resume task".into(),
                container_id: None,
                session_id: Some(session_id.clone()),
                resume_token: None,
                settings_route: None,
                resource_ref: None,
            },
            Self::Retry { resume_token } => FailureRemediationView {
                kind: "retry".into(),
                label: "Retry".into(),
                container_id: None,
                session_id: None,
                resume_token: Some(resume_token.clone()),
                settings_route: None,
                resource_ref: None,
            },
            Self::Repair(request) => FailureRemediationView {
                kind: "repair".into(),
                label: request.action.clone(),
                container_id: None,
                session_id: None,
                resume_token: None,
                settings_route: None,
                resource_ref: Some(request.target.clone()),
            },
            Self::OpenSettings(route) => FailureRemediationView {
                kind: "open_settings".into(),
                label: "Open settings".into(),
                container_id: None,
                session_id: None,
                resume_token: None,
                settings_route: Some(
                    match route {
                        SettingsRoute::Secrets => "secrets",
                        SettingsRoute::Containers => "connectors",
                        SettingsRoute::Models => "models",
                    }
                    .into(),
                ),
                resource_ref: None,
            },
            Self::OpenResource(resource) => FailureRemediationView {
                kind: "open_resource".into(),
                label: "Open resource".into(),
                container_id: None,
                session_id: None,
                resume_token: None,
                settings_route: None,
                resource_ref: Some(
                    match resource {
                        ResourceRef::Containers => "containers",
                        ResourceRef::Evaluations => "evaluations",
                        ResourceRef::Visuals => "visuals",
                        ResourceRef::Sessions => "sessions",
                        ResourceRef::Approvals => "approvals",
                    }
                    .into(),
                ),
            },
            Self::OpenDiagnostics => FailureRemediationView {
                kind: "open_diagnostics".into(),
                label: "Open diagnostics".into(),
                container_id: None,
                session_id: None,
                resume_token: None,
                settings_route: None,
                resource_ref: None,
            },
        }
    }
}
