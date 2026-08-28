//! The embedded product-telemetry contract.
//!
//! `contracts/telemetry-v1.toml` is the single source of truth for the event
//! dictionary, policy version, retention, and sync eligibility. This module
//! parses it once at first use; a malformed contract is a build defect and
//! panics on first access rather than silently recording nothing.

use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

const CONTRACT_TOML: &str = include_str!("../../../../../contracts/telemetry-v1.toml");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    Optional,
    Essential,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncClass {
    /// The flusher may ship this event after explicit consent.
    Eligible,
    /// Never leaves the device, consent or not.
    LocalOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventClass {
    Funnel,
    Product,
    Reliability,
}

#[derive(Clone, Debug, Deserialize)]
pub struct EventSpec {
    pub name: String,
    pub purpose: String,
    pub owner: String,
    pub class: EventClass,
    pub sensitivity: Sensitivity,
    pub sync: SyncClass,
    #[serde(rename = "properties")]
    pub allowed_properties: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct Contract {
    pub schema: u32,
    pub dictionary_version: String,
    pub collection_policy_version: String,
    pub retention_days_optional: i64,
    pub retention_days_essential: i64,
    pub envelope_properties: Vec<String>,
    #[serde(rename = "event")]
    pub events: Vec<EventSpec>,
}

pub fn contract() -> &'static Contract {
    static CONTRACT: OnceLock<Contract> = OnceLock::new();
    CONTRACT.get_or_init(|| {
        let parsed: Contract =
            toml::from_str(CONTRACT_TOML).expect("contracts/telemetry-v1.toml must parse");
        assert_eq!(parsed.schema, 1, "unsupported telemetry contract schema");
        assert!(
            !parsed.events.is_empty(),
            "telemetry contract declares no events"
        );
        for event in &parsed.events {
            assert!(
                !event.purpose.is_empty() && !event.owner.is_empty(),
                "telemetry event {} is missing purpose or owner",
                event.name
            );
            // The essential class exists for local recovery/debugging only;
            // marking one sync-eligible is a contract error, not a choice.
            assert!(
                !(event.sensitivity == Sensitivity::Essential
                    && event.sync == SyncClass::Eligible),
                "essential telemetry event {} must be local_only",
                event.name
            );
        }
        parsed
    })
}

pub fn dictionary_version() -> &'static str {
    &contract().dictionary_version
}

pub fn collection_policy_version() -> &'static str {
    &contract().collection_policy_version
}

pub fn retention_days(sensitivity: Sensitivity) -> i64 {
    match sensitivity {
        Sensitivity::Optional => contract().retention_days_optional,
        Sensitivity::Essential => contract().retention_days_essential,
    }
}

pub fn spec(name: &str) -> Option<&'static EventSpec> {
    contract().events.iter().find(|event| event.name == name)
}

pub fn allowed_property(spec: &EventSpec, key: &str) -> bool {
    contract()
        .envelope_properties
        .iter()
        .any(|value| value == key)
        || spec.allowed_properties.iter().any(|value| value == key)
}

pub fn sensitivity_name(value: Sensitivity) -> &'static str {
    match value {
        Sensitivity::Optional => "optional",
        Sensitivity::Essential => "essential",
    }
}

pub fn class_name(value: EventClass) -> &'static str {
    match value {
        EventClass::Funnel => "funnel",
        EventClass::Product => "product",
        EventClass::Reliability => "reliability",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_parses_and_keeps_essential_local() {
        let parsed = contract();
        assert_eq!(parsed.dictionary_version, "workshop.product-telemetry.v2");
        assert!(parsed
            .events
            .iter()
            .filter(|event| event.sensitivity == Sensitivity::Essential)
            .all(|event| event.sync == SyncClass::LocalOnly));
    }

    #[test]
    fn every_owner_is_a_workshop_team() {
        assert!(contract()
            .events
            .iter()
            .all(|event| event.owner.starts_with("workshop-")));
    }

    /// The backend registry (backend repo, app/api/v1/routes_product.py,
    /// PRODUCT_REGISTRY["workshop"]) mirrors exactly this set. Editing the
    /// contract's sync-eligible events must update the registry in the same
    /// change — this pin is the workshop-side half of that drift gate; the
    /// backend's test_product_usage_events_registry.py is the other half.
    #[test]
    fn sync_eligible_events_match_the_backend_registry_mirror() {
        let mut eligible: Vec<&str> = contract()
            .events
            .iter()
            .filter(|event| event.sync == SyncClass::Eligible)
            .map(|event| event.name.as_str())
            .collect();
        eligible.sort_unstable();
        let mut expected = vec![
            "app_first_launch",
            "artifact_created",
            "download_initiated",
            "download_served",
            "first_experiment_visual",
            "first_report_shared",
            "first_run_succeeded",
            "first_workspace_opened",
            "hosted_activation_completed",
            "hosted_job_completed",
            "hosted_job_failed",
            "hosted_job_started",
            "local_activation_completed",
            "recipe_saved",
            "report_created",
            "report_published",
            "signin_completed",
            "signout_completed",
            "signup_completed",
            "workflow_started",
            "workflow_terminal",
        ];
        expected.sort_unstable();
        assert_eq!(eligible, expected);
    }
}
