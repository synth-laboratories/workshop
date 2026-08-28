//! Workshop-owned policy for non-shell approval kinds.
//!
//! The canonical `approval_policy` vocabulary is shared with Codex, but host
//! mutations must interpret it themselves. In particular, `never` means the
//! operator selected permissive behavior; falling back to a modal would turn
//! that setting into an undocumented "Always ask" mode.

use super::approval::{ApprovalDecision, ApprovalKind};
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
}

impl EffectiveApprovalProfile {
    pub(crate) fn receipt_payload(&self, session_id: &str) -> Value {
        json!({
            "sessionId": session_id,
            "approvalPolicy": self.approval_policy,
            "sandbox": self.sandbox_mode,
            "mcpToolsApprovalMode": self.mcp_tools_approval_mode,
            "machineConfigPath": self.machine_config_path,
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

