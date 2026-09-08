//! Shared named operation adapters. Compatibility binaries and the unified
//! Workshop catalogue use these exact definitions and handlers.

pub mod annotations;
pub mod computer_use;
pub mod containers;
pub mod diagnostics;
pub mod display;
pub mod human_annotations;
pub mod optimizers;
pub mod plugins;
pub mod secrets;
pub mod session;
pub mod visuals;
pub mod traces;

use serde_json::Value;

pub struct Adapter {
    pub domain: &'static str,
    pub tools: fn() -> Value,
    pub call: fn(&str, &Value) -> Result<Value, String>,
}

/// Ordered compatibility registrations. Domain handlers still own validation,
/// approvals and persistence. A duplicate public name retains the typed API.
pub fn registrations() -> Vec<Adapter> {
    vec![
        Adapter { domain: "annotations", tools: annotations::tools, call: annotations::call_tool },
        Adapter { domain: "computer_use", tools: computer_use::tools, call: computer_use::call_tool },
        Adapter { domain: "containers", tools: containers::tools, call: containers::call_tool },
        Adapter { domain: "diagnostics", tools: diagnostics::tools, call: diagnostics::call_tool },
        Adapter { domain: "display", tools: display::tools, call: display::call_tool },
        Adapter { domain: "human_annotations", tools: human_annotations::tools, call: human_annotations::call_tool },
        Adapter { domain: "optimizers", tools: optimizers::tools, call: optimizers::call_tool },
        Adapter { domain: "plugins", tools: plugins::tools, call: plugins::call_tool },
        Adapter { domain: "secrets", tools: secrets::tools, call: secrets::call_tool },
        Adapter { domain: "session", tools: session::public_tools, call: session::call_public },
        Adapter { domain: "visuals", tools: visuals::tools, call: visuals::call_public },
        Adapter { domain: "traces", tools: traces::tools, call: traces::call_tool },
    ]
}

/// Apply the same human-decision boundary to compatibility facades as to
/// named desktop commands. Nested operations are not an authorization escape.
pub fn check_agent_call(name: &str, args: &Value) -> anyhow::Result<()> {
    if name == "computer_use" && !crate::context::mcp_group_enabled(crate::context::COMPUTER_USE_MCP_GROUP) {
        anyhow::bail!("human_action_required: enable Computer Use in Settings > Context");
    }
    let operation = args.get("operation").and_then(Value::as_str).unwrap_or("");
    if let Some(surface) = crate::contract::desktop_policy::human_surface(operation) {
        anyhow::bail!("human_action_required: {operation} requires {surface}");
    }
    if name == "secrets_manage" && matches!(operation,
        "locator_request" | "locator_remove" | "source_request" | "source_remove" | "request_use" | "use_revoke" | "request_env_import") {
        anyhow::bail!("human_action_required: credential changes require Workshop credential consent controls");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn facade_operations_cannot_supply_human_decisions() {
        assert!(check_agent_call("human_annotation_manage", &json!({"operation":"human_annotation_campaign_adjudicate"})).is_err());
        assert!(check_agent_call("secrets_manage", &json!({"operation":"request_env_import"})).is_err());
        assert!(check_agent_call("human_annotation_manage", &json!({"operation":"human_annotation_get"})).is_ok());
        assert!(check_agent_call("secrets_manage", &json!({"operation":"bindings_list"})).is_ok());
    }
}
