//! Workshop-owned policy for non-shell approval kinds.
//!
//! The canonical `approval_policy` vocabulary is shared with Codex, but host
//! mutations must interpret it themselves. In particular, `never` means the
//! operator selected permissive behavior; falling back to a modal would turn
//! that setting into an undocumented "Always ask" mode.

use super::approval::{ApprovalDecision, ApprovalKind};
use crate::synth_config::PaidComputeAutoApprovalPolicy;
use anyhow::{anyhow, Result};
use serde_json::{json, Value};

/// Every bundled MCP server is generated with this tools-approval mode; the
/// sealed session profile records the same value so the receipt describes the
/// whole approval surface, not just the host and provider layers.
pub(crate) const MCP_TOOLS_APPROVAL_MODE: &str = "approve";

/// The approval surface a session actually runs under, resolved once at
/// session start and sealed for the attachment's lifetime. Host authorization
/// consults this instead of re-reading machine config, so the layers cannot
/// drift apart mid-session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct EffectiveApprovalProfile {
    pub approval_policy: String,
    pub sandbox_mode: String,
    pub mcp_tools_approval_mode: String,
    /// Where the machine half of the agreement was read from.
    pub machine_config_path: String,
    /// Frozen at session start. Config edits apply only to new conversations.
    pub paid_compute: PaidComputeAutoApprovalPolicy,
}

impl EffectiveApprovalProfile {
    pub(crate) fn receipt_payload(&self, session_id: &str) -> Value {
        json!({
            "sessionId": session_id,
            "approvalPolicy": self.approval_policy,
            "sandbox": self.sandbox_mode,
            "mcpToolsApprovalMode": self.mcp_tools_approval_mode,
            "machineConfigPath": self.machine_config_path,
            "paidComputeAutoApproval": {
                "enabled": self.paid_compute.enabled,
                "maxRequestUsdMicros": self.paid_compute.max_request_usd_micros,
                "maxConversationUsdMicros": self.paid_compute.max_conversation_usd_micros,
                "providers": self.paid_compute.providers,
            },
        })
    }
}

/// Resolve the session's requested approval policy and sandbox against machine
/// config. `None` inherits the machine value; an explicit request that
/// disagrees with machine config fails closed — a divergent layer is how an
/// "unattended" run ends up blocked on a card nobody can answer.
pub(crate) fn resolve_effective(
    requested_approval: Option<&str>,
    requested_sandbox: Option<&str>,
) -> Result<EffectiveApprovalProfile> {
    let machine = crate::synth_config::desktop_permission_settings()?;
    let approval_policy = match requested_approval {
        None => machine.approval_policy.clone(),
        Some(requested) if requested == machine.approval_policy => requested.to_owned(),
        Some(requested) => {
            return Err(anyhow!(
                "session approval policy `{requested}` disagrees with machine policy `{}` from {}; \
                 align the session request or the machine config before starting",
                machine.approval_policy,
                machine.config_path,
            ))
        }
    };
    let sandbox_mode = match requested_sandbox {
        None => machine.sandbox_mode.clone(),
        Some(requested) if requested == machine.sandbox_mode => requested.to_owned(),
        Some(requested) => {
            return Err(anyhow!(
                "session sandbox `{requested}` disagrees with machine sandbox `{}` from {}; \
                 align the session request or the machine config before starting",
                machine.sandbox_mode,
                machine.config_path,
            ))
        }
    };
    Ok(EffectiveApprovalProfile {
        approval_policy,
        sandbox_mode,
        mcp_tools_approval_mode: MCP_TOOLS_APPROVAL_MODE.into(),
        machine_config_path: machine.config_path,
        paid_compute: machine.paid_compute.policy()?,
    })
}

/// One policy table for every host approval call site. Host callers have no
/// run-count context; zero active runs is the reading that reproduces the
/// historical session table exactly (see the agreement test below).
pub(crate) fn auto_decision(
    approval_policy: &str,
    kind: &ApprovalKind,
) -> Result<Option<ApprovalDecision>> {
    crate::plugins::policy::auto_decision(approval_policy, kind, 0)
}

pub(crate) fn operator_decision(kind: &ApprovalKind) -> ApprovalDecision {
    crate::plugins::policy::approve_kind(kind)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::approval::PaidComputeCap;
    use serde_json::json;

    fn paid() -> ApprovalKind {
        ApprovalKind::PaidCompute {
            operation: "optimizer.recipe.start".into(),
            parameters: json!({"recipeId":"bounded"}),
            estimated_cost_usd_micros: None,
            requested_cap: PaidComputeCap {
                max_cost_usd_micros: None,
                max_rollouts: Some(8),
            },
            requesting_agent: "agent:test".into(),
            recipe_id: None,
            dataset: None,
            proposer_model: None,
            evaluator_model: None,
            timeout_seconds: None,
            credential_names: vec![],
            preparation_digest: None,
        }
    }

    #[test]
    fn paid_compute_always_uses_the_native_approval_modal() {
        for policy in ["never", "on-request", "untrusted"] {
            assert!(auto_decision(policy, &paid()).unwrap().is_none());
        }
    }

    #[test]
    fn paid_and_credentials_always_use_native_approval_modals() {
        assert!(auto_decision("on-request", &paid()).unwrap().is_none());
        for consent in [
            crate::session::approval::CredentialConsent::RememberLocator,
            crate::session::approval::CredentialConsent::RegisterSource,
            crate::session::approval::CredentialConsent::IssueLease,
        ] {
            let credential = ApprovalKind::CredentialAccess {
                consent,
                provider: "openai".into(),
                purpose: "bounded optimizer recipe".into(),
                locator_id: None,
                display_path: None,
                variable: None,
                switch_from_display: None,
            };
            for policy in ["never", "on-request", "untrusted"] {
                assert!(auto_decision(policy, &credential).unwrap().is_none());
            }
        }
    }

    #[test]
    fn container_replacement_honors_never_but_remains_modal_otherwise() {
        let replacement = ApprovalKind::ContainerLifecycle {
            container_id: "ctr_craftax".into(),
            declaration_id: "nanohorizon-craftax".into(),
            declaration_digest: "sha256:declaration".into(),
            manifest_path: "/approved/workshop.containers.toml".into(),
            source_root: "/approved".into(),
            source_revision: Some("revision".into()),
            source_digest: Some("sha256:source".into()),
            action: "force_replace".into(),
            effect: "replace the declared workload".into(),
        };
        assert!(auto_decision("never", &replacement).unwrap().is_some());
        for policy in ["on-request", "untrusted"] {
            assert!(auto_decision(policy, &replacement).unwrap().is_none());
        }
        replacement
            .validate_decision(&ApprovalDecision::Approve {
                scope: crate::session::approval::ApprovalScope::Once,
            })
            .unwrap();
        assert!(replacement
            .validate_decision(&ApprovalDecision::Approve {
                scope: crate::session::approval::ApprovalScope::Session,
            })
            .is_err());
    }

    #[test]
    fn on_request_auto_approves_sidecar_start_and_stop_only() {
        let start = ApprovalKind::SidecarLifecycle {
            sidecar: "optimizers".into(),
            action: "start".into(),
        };
        let install = ApprovalKind::SidecarLifecycle {
            sidecar: "optimizers".into(),
            action: "install".into(),
        };
        assert!(auto_decision("on-request", &start).unwrap().is_some());
        assert!(auto_decision("on-request", &install).unwrap().is_none());
        assert!(auto_decision("untrusted", &start).unwrap().is_none());
    }

    #[test]
    fn unknown_policy_fails_closed() {
        assert!(auto_decision("always-ask", &paid()).is_err());
    }

    /// The session table delegates to the plugins table with zero active runs.
    /// This pins the agreement so the two call sites cannot drift apart again:
    /// every (kind, policy) pair must produce the same decision through both
    /// entry points.
    #[test]
    fn host_and_plugin_tables_agree_for_every_kind_and_policy() {
        let shell = ApprovalKind::ShellCommand {
            request_method: "execCommandApproval".into(),
            detail: "Run a shell command".into(),
            scope: None,
            always_supported: true,
        };
        let sidecar_start = ApprovalKind::SidecarLifecycle {
            sidecar: "optimizers".into(),
            action: "start".into(),
        };
        let sidecar_stop = ApprovalKind::SidecarLifecycle {
            sidecar: "optimizers".into(),
            action: "stop".into(),
        };
        let plugin_install = ApprovalKind::PluginLifecycle {
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
        };
        let credential = ApprovalKind::CredentialAccess {
            consent: crate::session::approval::CredentialConsent::IssueLease,
            provider: "openai".into(),
            purpose: "bounded optimizer recipe".into(),
            locator_id: None,
            display_path: None,
            variable: None,
            switch_from_display: None,
        };
        let kinds = [
            shell,
            paid(),
            sidecar_start,
            sidecar_stop,
            plugin_install,
            credential,
        ];
        for policy in ["never", "on-request", "untrusted"] {
            for kind in &kinds {
                let host = auto_decision(policy, kind).unwrap();
                let plugin = crate::plugins::policy::auto_decision(policy, kind, 0).unwrap();
                assert_eq!(host, plugin, "policy `{policy}` kind `{}`", kind.name());
            }
        }
    }

    #[test]
    fn effective_profile_inherits_machine_values_and_fails_closed_on_conflict() {
        let _machine =
            crate::synth_config::test_machine_permissions::install("never", "danger-full-access");
        let inherited = resolve_effective(None, None).unwrap();
        assert_eq!(inherited.approval_policy, "never");
        assert_eq!(inherited.sandbox_mode, "danger-full-access");
        assert_eq!(inherited.mcp_tools_approval_mode, MCP_TOOLS_APPROVAL_MODE);

        let agreeing = resolve_effective(Some("never"), Some("danger-full-access")).unwrap();
        assert_eq!(agreeing, inherited);

        let error = resolve_effective(Some("untrusted"), None).unwrap_err();
        assert!(error.to_string().contains("disagrees with machine policy"));

        let error = resolve_effective(None, Some("read-only")).unwrap_err();
        assert!(error.to_string().contains("disagrees with machine sandbox"));
    }

    fn computer_use(hazard: bool) -> ApprovalKind {
        ApprovalKind::ComputerUse {
            app: "com.apple.mail".into(),
            action: "click".into(),
            payload: json!({ "recipient": "board@example.com" }),
            hazard,
            element_index: None,
        }
    }

    /// The host engine and the plugin engine both honor `never`, so both need
    /// the hazard carve-out. Covering only one leaves the other as the hole.
    #[test]
    fn hazard_computer_use_outranks_the_permissive_policy() {
        for policy in ["never", "on-request", "untrusted"] {
            assert!(
                auto_decision(policy, &computer_use(true))
                    .unwrap()
                    .is_none(),
                "`{policy}` auto-settled a hazard action"
            );
        }
        assert!(auto_decision("always-ask", &computer_use(true)).is_err());
        assert!(auto_decision("never", &computer_use(false))
            .unwrap()
            .is_some());
    }
}
