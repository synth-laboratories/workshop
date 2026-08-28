//! Map canonical `.synth` / Desktop approval_policy onto plugin and compute actions.

use crate::session::approval::{ApprovalDecision, ApprovalKind, ApprovalScope, PaidComputeCap};
use anyhow::{anyhow, Result};
use serde_json::json;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PluginRisk {
    Read,
    Low,
    High,
    /// Requires a person at the keyboard. Unreachable from `auto_decision` under
    /// every policy, `never` included. See `ApprovalKind::requires_human`.
    HandOff,
}

pub fn classify(kind: &ApprovalKind, active_runs: u64) -> PluginRisk {
    if kind.requires_human() {
        return PluginRisk::HandOff;
    }
    match kind {
        ApprovalKind::PluginLifecycle { action, .. } => match action.as_str() {
            "enable" | "disable" => PluginRisk::Low,
            "start" | "stop" if active_runs == 0 => PluginRisk::Low,
            "stop" | "install" | "update" | "remove" => PluginRisk::High,
            _ => PluginRisk::High,
        },
        ApprovalKind::SidecarLifecycle { action, .. } => match action.as_str() {
            "start" | "stop" if active_runs == 0 => PluginRisk::Low,
            _ => PluginRisk::High,
        },
        ApprovalKind::ContainerLifecycle { .. } => PluginRisk::HandOff,
        // Unreachable: `requires_human` above already returned `HandOff`.
        // Spelled out anyway because the match is exhaustive on purpose — a
        // kind added later must not be able to acquire a risk class by
        // falling into somebody else's arm.
        ApprovalKind::VisualTemplatePersist { .. } => PluginRisk::HandOff,
        ApprovalKind::PaidCompute { .. } => PluginRisk::High,
        ApprovalKind::CredentialAccess { .. } => PluginRisk::High,
        ApprovalKind::ShellCommand { .. } => PluginRisk::High,
        // Non-hazard computer use: driving an app the operator has not yet
        // allowed. Hazard actions never reach here — `requires_human` above
        // classified them `HandOff`.
        ApprovalKind::ComputerUse { .. } => PluginRisk::High,
    }
}

/// Returns `Some(decision)` when the policy auto-settles, or `None` when the
/// native approval card must be shown. Maximally permissive `never` is honored
/// rather than silently falling back to Always ask.
pub fn auto_decision(
    approval_policy: &str,
    kind: &ApprovalKind,
    active_runs: u64,
) -> Result<Option<ApprovalDecision>> {
    let risk = classify(kind, active_runs);
    match (approval_policy, risk) {
        // Listed policy-by-policy rather than as `(_, HandOff)` so that an
        // unrecognized policy still falls through to the error arm.
        ("never" | "on-request" | "untrusted", PluginRisk::HandOff) => Ok(None),
        (_, PluginRisk::Read) => Ok(Some(approve_kind(kind))),
        ("never", _) => Ok(Some(approve_kind(kind))),
        ("on-request", PluginRisk::Low) => Ok(Some(approve_kind(kind))),
        ("on-request", PluginRisk::High) => Ok(None),
        ("untrusted", PluginRisk::Low | PluginRisk::High) => Ok(None),
        (other, _) => Err(anyhow!("unsupported approval policy `{other}`")),
    }
}

pub(crate) fn approve_kind(kind: &ApprovalKind) -> ApprovalDecision {
    match kind {
        ApprovalKind::PaidCompute { requested_cap, .. } => ApprovalDecision::ApproveWithCap {
            cap: requested_cap.clone(),
        },
        ApprovalKind::PluginLifecycle {
            always_supported: true,
            ..
        }
        | ApprovalKind::SidecarLifecycle { .. } => ApprovalDecision::Approve {
            scope: ApprovalScope::Session,
        },
        _ => ApprovalDecision::Approve {
            scope: ApprovalScope::Once,
        },
    }
}

pub fn plugin_kind(
    action: &str,
    catalog: &super::types::CatalogEntry,
    active_runs: u64,
    digest: Option<String>,
    retention: &str,
) -> ApprovalKind {
    let (service_effect, always_supported) = match action {
        "enable" => ("Enable Optimizers navigation and new-session MCP advertisement", true),
        "disable" => (
            "Disable Optimizers navigation and new-session MCP advertisement; running service is left unchanged",
            true,
        ),
        "install" | "update" => (
            "Download, verify, and materialize an offline-startable optimizer distribution",
            true,
        ),
        "start" => ("Start the installed optimizer service", true),
        "stop" if active_runs == 0 => ("Stop the idle optimizer service; retain runs and visuals", true),
        "stop" => (
            "Stop the optimizer service while jobs are active; product safety may refuse",
            false,
        ),
        "remove" => (
            "Remove the installed optimizer runtime; retained runs and visuals are kept",
            false,
        ),
        _ => ("Mutate the Optimizers product plugin", false),
    };
    ApprovalKind::PluginLifecycle {
        plugin_id: catalog.plugin_id.clone(),
        action: action.into(),
        version: Some(catalog.version.clone()),
        publisher: catalog.publisher.clone(),
        digest,
        download_size_bytes: matches!(action, "install" | "update")
            .then_some(catalog.download_size_bytes),
        network_host: matches!(action, "install" | "update").then(|| catalog.network_host.clone()),
        service_effect: service_effect.into(),
        active_runs,
        retention: retention.into(),
        always_supported,
    }
}

pub fn compute_kind(
    recipe_id: &str,
    preparation_digest: &str,
    max_cost_usd: f64,
    max_rollouts: u64,
    proposer_model: &str,
    timeout_seconds: u64,
) -> ApprovalKind {
    let micros = (max_cost_usd * 1_000_000.0).round() as u64;
    ApprovalKind::PaidCompute {
        operation: recipe_id.into(),
        parameters: json!({
            "recipeId": recipe_id,
            "preparationDigest": preparation_digest,
        }),
        estimated_cost_usd_micros: Some(micros),
        requested_cap: PaidComputeCap {
            max_cost_usd_micros: Some(micros),
            max_rollouts: Some(max_rollouts),
        },
        requesting_agent: "agent".into(),
        recipe_id: Some(recipe_id.into()),
        dataset: Some(recipe_id.into()),
        proposer_model: Some(proposer_model.into()),
        evaluator_model: Some(recipe_id.into()),
        timeout_seconds: Some(timeout_seconds),
        credential_names: vec!["OPENAI_API_KEY".into()],
        preparation_digest: Some(preparation_digest.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::approval::PaidComputeCap;

    fn install_kind() -> ApprovalKind {
        ApprovalKind::PluginLifecycle {
            plugin_id: "optimizers".into(),
            action: "install".into(),
            version: Some("0.2.0".into()),
            publisher: "Synth Laboratories".into(),
            digest: None,
            download_size_bytes: Some(1),
            network_host: Some("pypi.org".into()),
            service_effect: "download".into(),
            active_runs: 0,
            retention: "keep".into(),
            always_supported: true,
        }
    }

    fn start_kind() -> ApprovalKind {
        ApprovalKind::PluginLifecycle {
            plugin_id: "optimizers".into(),
            action: "start".into(),
            version: Some("0.2.0".into()),
            publisher: "Synth Laboratories".into(),
            digest: None,
            download_size_bytes: None,
            network_host: None,
            service_effect: "start".into(),
            active_runs: 0,
            retention: "keep".into(),
            always_supported: true,
        }
    }

    fn compute() -> ApprovalKind {
        ApprovalKind::PaidCompute {
            operation: "gepa.banking77.smoke.v1".into(),
            parameters: json!({"recipeId":"gepa.banking77.smoke.v1"}),
            estimated_cost_usd_micros: Some(2_450_000),
            requested_cap: PaidComputeCap {
                max_cost_usd_micros: Some(2_450_000),
                max_rollouts: Some(240),
            },
            requesting_agent: "agent:test".into(),
            recipe_id: Some("gepa.banking77.smoke.v1".into()),
            dataset: Some("banking77".into()),
            proposer_model: Some("gpt-5.6-luna".into()),
            evaluator_model: Some("banking77_candidate".into()),
            timeout_seconds: Some(300),
            credential_names: vec!["OPENAI_API_KEY".into()],
            preparation_digest: Some("sha256:abc".into()),
        }
    }

    #[test]
    fn never_auto_authorizes_risky_actions() {
        assert!(auto_decision("never", &install_kind(), 0)
            .unwrap()
            .is_some());
        assert!(auto_decision("never", &compute(), 0).unwrap().is_some());
        match auto_decision("never", &compute(), 0).unwrap() {
            Some(ApprovalDecision::ApproveWithCap { cap }) => {
                assert_eq!(cap.max_rollouts, Some(240));
            }
            other => panic!("expected capped auto-approval, got {other:?}"),
        }
    }

    #[test]
    fn on_request_prompts_for_install_and_compute_but_not_idle_start() {
        assert!(auto_decision("on-request", &install_kind(), 0)
            .unwrap()
            .is_none());
        assert!(auto_decision("on-request", &compute(), 0)
            .unwrap()
            .is_none());
        assert!(auto_decision("on-request", &start_kind(), 0)
            .unwrap()
            .is_some());
    }

    fn computer_use(hazard: bool) -> ApprovalKind {
        ApprovalKind::ComputerUse {
            app: "com.apple.mail".into(),
            action: "click".into(),
            payload: json!({ "recipient": "board@example.com", "subject": "Q3" }),
            hazard,
            element_index: Some(42),
        }
    }

    /// G6. The permissive policy is honored for every other risk class in this
    /// file; a hazard action is the one thing it must not reach.
    #[test]
    fn hazard_actions_refuse_under_every_policy_including_never() {
        for policy in ["never", "on-request", "untrusted"] {
            assert!(
                auto_decision(policy, &computer_use(true), 0)
                    .unwrap()
                    .is_none(),
                "`{policy}` auto-settled a hazard action"
            );
        }
        assert_eq!(classify(&computer_use(true), 0), PluginRisk::HandOff);
    }

    /// A misspelled policy must not be laundered into "ask the human" — that
    /// reads as working and hides the misconfiguration.
    #[test]
    fn unknown_policy_still_errors_for_hazard_actions() {
        assert!(auto_decision("always-ask", &computer_use(true), 0).is_err());
    }

    /// Driving an app is not a hazard by itself: it takes the ordinary High
    /// path, so `never` settles it like any other plugin mutation.
    #[test]
    fn routine_computer_use_follows_the_ordinary_high_risk_path() {
        assert_eq!(classify(&computer_use(false), 0), PluginRisk::High);
        assert!(auto_decision("never", &computer_use(false), 0)
            .unwrap()
            .is_some());
        assert!(auto_decision("on-request", &computer_use(false), 0)
            .unwrap()
            .is_none());
        assert!(auto_decision("untrusted", &computer_use(false), 0)
            .unwrap()
            .is_none());
    }

    #[test]
    fn untrusted_always_asks() {
        assert!(auto_decision("untrusted", &start_kind(), 0)
            .unwrap()
            .is_none());
        assert!(auto_decision("untrusted", &install_kind(), 0)
            .unwrap()
            .is_none());
    }

    #[test]
    fn compute_approval_discloses_the_recipe_id_not_a_shipped_task_family() {
        let approval = compute_kind(
            "gepa.workspace.v1",
            "sha256:prep",
            1.50,
            6,
            "gpt-5.6-luna",
            300,
        );
        match approval {
            ApprovalKind::PaidCompute {
                dataset,
                evaluator_model,
                requested_cap,
                preparation_digest,
                ..
            } => {
                assert_eq!(dataset.as_deref(), Some("gepa.workspace.v1"));
                assert_eq!(evaluator_model.as_deref(), Some("gepa.workspace.v1"));
                assert_eq!(requested_cap.max_cost_usd_micros, Some(1_500_000));
                assert_eq!(requested_cap.max_rollouts, Some(6));
                assert_eq!(preparation_digest.as_deref(), Some("sha256:prep"));
            }
            other => panic!("expected paid compute approval, got {other:?}"),
        }
    }
}
