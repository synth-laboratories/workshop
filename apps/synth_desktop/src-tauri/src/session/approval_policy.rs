//! Workshop-owned policy for non-shell approval kinds.
//!
//! The canonical `approval_policy` vocabulary is shared with Codex, but host
//! mutations must interpret it themselves. In particular, `never` means the
//! operator selected permissive behavior; falling back to a modal would turn
//! that setting into an undocumented "Always ask" mode.

use super::approval::{ApprovalDecision, ApprovalKind, ApprovalScope};
use anyhow::{anyhow, Result};

pub(crate) fn auto_decision(
    approval_policy: &str,
    kind: &ApprovalKind,
) -> Result<Option<ApprovalDecision>> {
    match approval_policy {
        "never" => Ok(Some(approve_kind(kind))),
        "on-request" => match kind {
            ApprovalKind::SidecarLifecycle { action, .. }
                if matches!(action.as_str(), "start" | "stop") =>
            {
                Ok(Some(approve_kind(kind)))
            }
            _ => Ok(None),
        },
        "untrusted" => Ok(None),
        other => Err(anyhow!("unsupported approval policy `{other}`")),
    }
}

pub(crate) fn operator_decision(kind: &ApprovalKind) -> ApprovalDecision {
    approve_kind(kind)
}

fn approve_kind(kind: &ApprovalKind) -> ApprovalDecision {
    match kind {
        ApprovalKind::PaidCompute { requested_cap, .. } => ApprovalDecision::ApproveWithCap {
            cap: requested_cap.clone(),
        },
        ApprovalKind::SidecarLifecycle { .. } => ApprovalDecision::Approve {
            scope: ApprovalScope::Session,
        },
        _ => ApprovalDecision::Approve {
            scope: ApprovalScope::Once,
        },
    }
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
        }
    }

    #[test]
    fn permissive_policy_auto_approves_with_the_declared_cap() {
        match auto_decision("never", &paid()).unwrap() {
            Some(ApprovalDecision::ApproveWithCap { cap }) => {
                assert_eq!(cap.max_rollouts, Some(8));
                assert_eq!(cap.max_cost_usd_micros, None);
            }
            other => panic!("expected capped decision, got {other:?}"),
        }
    }

    #[test]
    fn paid_and_credentials_are_never_implicitly_remembered() {
        assert!(auto_decision("on-request", &paid()).unwrap().is_none());
        let credential = ApprovalKind::CredentialAccess {
            provider: "openai".into(),
            purpose: "bounded optimizer recipe".into(),
        };
        assert!(auto_decision("on-request", &credential).unwrap().is_none());
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
}
