//! Provider adapters. Each adapter owns its allowlisted endpoints and the
//! header used to inject a credential. No generic URL forwarding.

use anyhow::{anyhow, Result};
use serde_json::Value;

use super::backend::SecretBytes;
use super::capability::MeasuredUsage;

#[derive(Clone, Debug)]
pub struct ProviderRoute {
    pub provider: &'static str,
    pub operation: &'static str,
    pub method: &'static str,
    pub local_path: &'static str,
    pub upstream_url: &'static str,
    pub auth: AuthStyle,
}

#[derive(Clone, Copy, Debug)]
pub enum AuthStyle {
    Bearer,
    AnthropicKey,
}

pub const ROUTES: &[ProviderRoute] = &[
    ProviderRoute {
        provider: "tinker",
        operation: "chat.completions.create",
        method: "POST",
        local_path: "/v1/providers/tinker/chat/completions",
        upstream_url:
            "https://tinker.thinkingmachines.dev/services/tinker-prod/oai/api/v1/chat/completions",
        auth: AuthStyle::Bearer,
    },
    ProviderRoute {
        provider: "groq",
        operation: "chat.completions.create",
        method: "POST",
        local_path: "/v1/providers/groq/chat/completions",
        upstream_url: "https://api.groq.com/openai/v1/chat/completions",
        auth: AuthStyle::Bearer,
    },
    ProviderRoute {
        provider: "openrouter",
        operation: "chat.completions.create",
        method: "POST",
        local_path: "/v1/providers/openrouter/chat/completions",
        upstream_url: "https://openrouter.ai/api/v1/chat/completions",
        auth: AuthStyle::Bearer,
    },
    ProviderRoute {
        provider: "openrouter",
        operation: "responses.create",
        method: "POST",
        local_path: "/v1/providers/openrouter/responses",
        upstream_url: "https://openrouter.ai/api/v1/responses",
        auth: AuthStyle::Bearer,
    },
    ProviderRoute {
        provider: "openai",
        operation: "chat.completions.create",
        method: "POST",
        local_path: "/v1/providers/openai/chat/completions",
        upstream_url: "https://api.openai.com/v1/chat/completions",
        auth: AuthStyle::Bearer,
    },
    ProviderRoute {
        provider: "openai",
        operation: "responses.create",
        method: "POST",
        local_path: "/v1/providers/openai/responses",
        upstream_url: "https://api.openai.com/v1/responses",
        auth: AuthStyle::Bearer,
    },
    ProviderRoute {
        provider: "anthropic",
        operation: "messages.create",
        method: "POST",
        local_path: "/v1/providers/anthropic/messages",
        upstream_url: "https://api.anthropic.com/v1/messages",
        auth: AuthStyle::AnthropicKey,
    },
];

pub fn route_for(method: &str, path: &str) -> Option<&'static ProviderRoute> {
    let path = path.split('?').next().unwrap_or(path);
    ROUTES
        .iter()
        .find(|route| route.method.eq_ignore_ascii_case(method) && route.local_path == path)
}

pub fn classify_variable(name: &str) -> Option<&'static str> {
    let upper = name.to_ascii_uppercase();
    match upper.as_str() {
        "OPENAI_API_KEY" => Some("openai"),
        "ANTHROPIC_API_KEY" => Some("anthropic"),
        "OPENROUTER_API_KEY" => Some("openrouter"),
        "TINKER_API_KEY" => Some("tinker"),
        "GROQ_API_KEY" => Some("groq"),
        _ if upper.contains("OPENAI") && upper.contains("KEY") => Some("openai"),
        _ if upper.contains("ANTHROPIC") && upper.contains("KEY") => Some("anthropic"),
        _ if upper.contains("DATABASE") || upper.ends_with("_DSN") || upper == "DATABASE_URL" => {
            Some("database")
        }
        _ => None,
    }
}

pub fn classification_label(provider: Option<&str>) -> &'static str {
    match provider {
        Some("openai") | Some("anthropic") | Some("openrouter") | Some("tinker") | Some("groq") => {
            "provider_api_key"
        }
        Some("database") => "database_url",
        _ => "secret",
    }
}

pub fn request_model(body: &Value) -> Option<&str> {
    body.get("model").and_then(Value::as_str)
}

pub fn request_effort(body: &Value) -> Option<&str> {
    body.get("reasoning")
        .and_then(|value| value.get("effort"))
        .and_then(Value::as_str)
        .or_else(|| body.get("reasoning_effort").and_then(Value::as_str))
}

pub fn parse_usage(body: &Value) -> MeasuredUsage {
    let usage = body.get("usage").cloned().unwrap_or(Value::Null);
    let int = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| usage.get(*key).and_then(Value::as_u64))
            .unwrap_or(0)
    };
    let input = int(&["prompt_tokens", "input_tokens"]);
    let output = int(&["completion_tokens", "output_tokens"]);
    let cost_usd = ["cost", "cost_usd"]
        .iter()
        .find_map(|key| usage.get(*key).and_then(Value::as_f64))
        .filter(|cost| cost.is_finite() && *cost >= 0.0);
    MeasuredUsage {
        calls: 1,
        input_tokens: input,
        output_tokens: output,
        cost_usd,
    }
}

/// Stable provider response identity retained for asynchronous accounting.
/// OpenRouter returns the generation id on the top-level chat response; the
/// Responses API carries the same identity inside `response`.
pub(crate) fn response_id(body: &Value) -> Option<&str> {
    body.get("id")
        .and_then(Value::as_str)
        .or_else(|| body.get("response")?.get("id")?.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

/// Parse OpenRouter's authoritative `/generation` accounting response. This
/// endpoint is the documented fallback when the generation id is available
/// but the inline response has not yet populated its `usage` object.
pub(crate) fn parse_openrouter_generation_usage(body: &Value) -> MeasuredUsage {
    let data = body.get("data").unwrap_or(body);
    let integer = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| data.get(*key).and_then(Value::as_u64))
            .unwrap_or(0)
    };
    let cost_usd = ["total_cost", "usage"]
        .iter()
        .find_map(|key| data.get(*key).and_then(Value::as_f64))
        .filter(|cost| cost.is_finite() && *cost >= 0.0);
    MeasuredUsage {
        calls: 1,
        input_tokens: integer(&["tokens_prompt", "native_tokens_prompt"]),
        output_tokens: integer(&["tokens_completion", "native_tokens_completion"]),
        cost_usd,
    }
}

pub fn inject_auth(
    builder: reqwest::RequestBuilder,
    route: &ProviderRoute,
    secret: &SecretBytes,
) -> Result<reqwest::RequestBuilder> {
    let value = secret
        .as_utf8()
        .map_err(|_| anyhow!("provider credential is not valid UTF-8"))?;
    Ok(match route.auth {
        AuthStyle::Bearer => builder.bearer_auth(value),
        AuthStyle::AnthropicKey => builder
            .header("x-api-key", value)
            .header("anthropic-version", "2023-06-01"),
    })
}

pub fn sanitize_error_message(message: &str) -> String {
    let mut out = message.to_owned();
    for needle in ["Bearer ", "sk-", "sk-proj-", "sk-ant-", "x-api-key"] {
        if let Some(index) = out.find(needle) {
            out.truncate(index);
            out.push_str("<redacted>");
            break;
        }
    }
    out
}

pub fn default_alias(provider: &str) -> String {
    match provider {
        "openai" => "Personal OpenAI".into(),
        "anthropic" => "Personal Anthropic".into(),
        "openrouter" => "Personal OpenRouter".into(),
        "tinker" => "Personal Tinker".into(),
        "groq" => "Personal Groq".into(),
        other => format!("Personal {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_key_inventory_is_explicit() {
        assert_eq!(classify_variable("OPENROUTER_API_KEY"), Some("openrouter"));
        assert_eq!(classify_variable("TINKER_API_KEY"), Some("tinker"));
        assert_eq!(classify_variable("GROQ_API_KEY"), Some("groq"));
        assert_eq!(classify_variable("UNRELATED_API_KEY"), None);
    }

    #[test]
    fn openai_compatible_routes_are_provider_scoped() {
        for (provider, upstream) in [
            ("openrouter", "https://openrouter.ai/"),
            ("tinker", "https://tinker.thinkingmachines.dev/"),
            ("groq", "https://api.groq.com/"),
        ] {
            let path = format!("/v1/providers/{provider}/chat/completions");
            let route = route_for("POST", &path).expect("allowlisted provider route");
            assert_eq!(route.provider, provider);
            assert!(route.upstream_url.starts_with(upstream));
        }
        assert!(route_for("POST", "/v1/providers/tinker/anything").is_none());
    }

    #[test]
    fn bearer_auth_is_injected_for_openrouter() {
        let route = route_for("POST", "/v1/providers/openrouter/chat/completions").unwrap();
        let request = inject_auth(
            crate::http::http_client().post(route.upstream_url),
            route,
            &SecretBytes::from_utf8("test-provider-key"),
        )
        .unwrap()
        .build()
        .unwrap();
        assert_eq!(
            request.headers().get("authorization").unwrap(),
            "Bearer test-provider-key"
        );
    }

    #[test]
    fn usage_without_provider_cost_remains_unpriced() {
        let usage = parse_usage(&serde_json::json!({
            "model": "openai/gpt-5.6-luna",
            "usage": {"prompt_tokens": 12, "completion_tokens": 8}
        }));
        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.output_tokens, 8);
        assert_eq!(usage.cost_usd, None);
    }

    #[test]
    fn exact_provider_cost_is_preserved() {
        let usage = parse_usage(&serde_json::json!({
            "usage": {"input_tokens": 12, "output_tokens": 8, "cost": 0.0042}
        }));
        assert_eq!(usage.cost_usd, Some(0.0042));
    }

    #[test]
    fn response_identity_survives_both_openai_compatible_shapes() {
        assert_eq!(
            response_id(&serde_json::json!({"id": "gen-top"})),
            Some("gen-top")
        );
        assert_eq!(
            response_id(&serde_json::json!({"response": {"id": "resp-nested"}})),
            Some("resp-nested")
        );
        assert_eq!(response_id(&serde_json::json!({"id": "  "})), None);
    }

    #[test]
    fn openrouter_generation_metadata_recovers_exact_cost_and_tokens() {
        let usage = parse_openrouter_generation_usage(&serde_json::json!({
            "data": {
                "tokens_prompt": 1479,
                "tokens_completion": 83,
                "native_tokens_prompt": 1400,
                "native_tokens_completion": 80,
                "total_cost": 0.0003107,
                "usage": 0.9
            }
        }));
        assert_eq!(usage.input_tokens, 1479);
        assert_eq!(usage.output_tokens, 83);
        assert_eq!(usage.cost_usd, Some(0.0003107));
    }
}
