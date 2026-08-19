//! Versioned product-telemetry event dictionary.
//!
//! Every event has an owner, retention, sensitivity class, and an explicit
//! allowlist of properties. Events not in this table are refused. Properties
//! not on the allowlist are stripped. Sensitive fields never belong here.

use serde::Serialize;

pub const DICTIONARY_VERSION: &str = "workshop.product-telemetry.v1";
pub const COLLECTION_POLICY_VERSION: &str = "workshop.product-telemetry.policy.v1";
pub const RETENTION_DAYS_OPTIONAL: i64 = 90;
pub const RETENTION_DAYS_ESSENTIAL: i64 = 365;

/// Envelope fields attached to every accepted event. Not product content.
pub const ENVELOPE_PROPERTIES: &[&str] = &[
    "schema_version",
    "app_version",
    "release_channel",
    "platform",
    "architecture",
    "install_id",
    "collection_policy_version",
    "outcome",
    "duration_ms",
    "error_class",
    "workflow_family",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    Optional,
    Essential,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct EventSpec {
    pub name: &'static str,
    pub purpose: &'static str,
    pub owner: &'static str,
    pub sensitivity: Sensitivity,
    pub retention_days: i64,
    pub allowed_properties: &'static [&'static str],
}

pub const EVENTS: &[EventSpec] = &[
    EventSpec {
        name: "download_initiated",
        purpose: "Count signed-artifact download starts from the public funnel.",
        owner: "workshop-distribution",
        sensitivity: Sensitivity::Optional,
        retention_days: RETENTION_DAYS_OPTIONAL,
        allowed_properties: &["release_channel", "platform", "architecture"],
    },
    EventSpec {
        name: "download_served",
        purpose: "Count successful signed-artifact delivery, distinct from download clicks.",
        owner: "workshop-distribution",
        sensitivity: Sensitivity::Optional,
        retention_days: RETENTION_DAYS_OPTIONAL,
        allowed_properties: &["release_channel", "platform", "architecture"],
    },
    EventSpec {
        name: "app_first_launch",
        purpose: "Record the first launch of an install, distinct from signup.",
        owner: "workshop-activation",
        sensitivity: Sensitivity::Optional,
        retention_days: RETENTION_DAYS_OPTIONAL,
        allowed_properties: &["platform", "architecture"],
    },
    EventSpec {
        name: "signup_completed",
        purpose: "Record first-time account creation on this install.",
        owner: "workshop-identity",
        sensitivity: Sensitivity::Optional,
        retention_days: RETENTION_DAYS_OPTIONAL,
        allowed_properties: &["outcome"],
    },
    EventSpec {
        name: "signin_completed",
        purpose: "Record a successful device pairing or session restore.",
        owner: "workshop-identity",
        sensitivity: Sensitivity::Optional,
        retention_days: RETENTION_DAYS_OPTIONAL,
        allowed_properties: &["outcome"],
    },
    EventSpec {
        name: "signout_completed",
        purpose: "Record completed local session removal without session material.",
        owner: "workshop-identity",
        sensitivity: Sensitivity::Optional,
        retention_days: RETENTION_DAYS_OPTIONAL,
        allowed_properties: &["outcome"],
    },
    EventSpec {
        name: "local_activation_completed",
        purpose: "Record a successful local-only activation milestone.",
        owner: "workshop-activation",
        sensitivity: Sensitivity::Optional,
        retention_days: RETENTION_DAYS_OPTIONAL,
        allowed_properties: &["workflow_family", "outcome"],
    },
    EventSpec {
        name: "hosted_activation_completed",
        purpose: "Record a successful entitled hosted activation milestone.",
        owner: "workshop-activation",
        sensitivity: Sensitivity::Optional,
        retention_days: RETENTION_DAYS_OPTIONAL,
        allowed_properties: &["workflow_family", "outcome"],
    },
    EventSpec {
        name: "first_workspace_opened",
        purpose: "Record the first agent or workspace opened on this install.",
        owner: "workshop-activation",
        sensitivity: Sensitivity::Optional,
        retention_days: RETENTION_DAYS_OPTIONAL,
        allowed_properties: &["workflow_family"],
    },
    EventSpec {
        name: "first_run_succeeded",
        purpose: "Record the first successful local or hosted run.",
        owner: "workshop-activation",
        sensitivity: Sensitivity::Optional,
        retention_days: RETENTION_DAYS_OPTIONAL,
        allowed_properties: &["workflow_family", "outcome"],
    },
    EventSpec {
        name: "first_experiment_visual",
        purpose: "Record the first experiment visual created on this install.",
        owner: "workshop-activation",
        sensitivity: Sensitivity::Optional,
        retention_days: RETENTION_DAYS_OPTIONAL,
        allowed_properties: &["workflow_family"],
    },
    EventSpec {
        name: "first_report_shared",
        purpose: "Record the first private or team report share.",
        owner: "workshop-activation",
        sensitivity: Sensitivity::Optional,
        retention_days: RETENTION_DAYS_OPTIONAL,
        allowed_properties: &["outcome"],
    },
    EventSpec {
        name: "hosted_job_started",
        purpose: "Count hosted job starts for activation and reliability.",
        owner: "workshop-reliability",
        sensitivity: Sensitivity::Optional,
        retention_days: RETENTION_DAYS_OPTIONAL,
        allowed_properties: &["workflow_family"],
    },
    EventSpec {
        name: "hosted_job_completed",
        purpose: "Count hosted job completions without run contents.",
        owner: "workshop-reliability",
        sensitivity: Sensitivity::Optional,
        retention_days: RETENTION_DAYS_OPTIONAL,
        allowed_properties: &["workflow_family", "outcome", "duration_ms"],
    },
    EventSpec {
        name: "hosted_job_failed",
        purpose: "Count hosted job failures by normalized error class only.",
        owner: "workshop-reliability",
        sensitivity: Sensitivity::Optional,
        retention_days: RETENTION_DAYS_OPTIONAL,
        allowed_properties: &["workflow_family", "outcome", "error_class"],
    },
    EventSpec {
        name: "workflow_started",
        purpose: "Count workflow starts by family for reliability.",
        owner: "workshop-reliability",
        sensitivity: Sensitivity::Optional,
        retention_days: RETENTION_DAYS_OPTIONAL,
        allowed_properties: &["workflow_family"],
    },
    EventSpec {
        name: "workflow_terminal",
        purpose: "Count workflow terminals by coarse outcome.",
        owner: "workshop-reliability",
        sensitivity: Sensitivity::Optional,
        retention_days: RETENTION_DAYS_OPTIONAL,
        allowed_properties: &["workflow_family", "outcome", "duration_ms", "error_class"],
    },
    EventSpec {
        name: "artifact_created",
        purpose: "Count artifact creation without artifact contents.",
        owner: "workshop-activation",
        sensitivity: Sensitivity::Optional,
        retention_days: RETENTION_DAYS_OPTIONAL,
        allowed_properties: &["workflow_family"],
    },
    EventSpec {
        name: "recipe_saved",
        purpose: "Count recipe saves without recipe names or contents.",
        owner: "workshop-activation",
        sensitivity: Sensitivity::Optional,
        retention_days: RETENTION_DAYS_OPTIONAL,
        allowed_properties: &["workflow_family"],
    },
    EventSpec {
        name: "report_created",
        purpose: "Count report creation, distinct from sharing.",
        owner: "workshop-activation",
        sensitivity: Sensitivity::Optional,
        retention_days: RETENTION_DAYS_OPTIONAL,
        allowed_properties: &["outcome"],
    },
    EventSpec {
        name: "report_published",
        purpose: "Count approved private/team report publication.",
        owner: "workshop-activation",
        sensitivity: Sensitivity::Optional,
        retention_days: RETENTION_DAYS_OPTIONAL,
        allowed_properties: &["outcome"],
    },
    EventSpec {
        name: "recovery_attempted",
        purpose: "Essential reliability: session or run recovery was attempted.",
        owner: "workshop-reliability",
        sensitivity: Sensitivity::Essential,
        retention_days: RETENTION_DAYS_ESSENTIAL,
        allowed_properties: &["error_class", "outcome"],
    },
    EventSpec {
        name: "recovery_succeeded",
        purpose: "Essential reliability: session or run recovery succeeded.",
        owner: "workshop-reliability",
        sensitivity: Sensitivity::Essential,
        retention_days: RETENTION_DAYS_ESSENTIAL,
        allowed_properties: &["outcome"],
    },
];

pub fn spec(name: &str) -> Option<&'static EventSpec> {
    EVENTS.iter().find(|event| event.name == name)
}

pub fn allowed_property(spec: &EventSpec, key: &str) -> bool {
    ENVELOPE_PROPERTIES.contains(&key) || spec.allowed_properties.contains(&key)
}
