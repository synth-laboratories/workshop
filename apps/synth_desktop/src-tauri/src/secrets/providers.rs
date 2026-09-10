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

/// Locate the usage object on an OpenAI-compatible body.
///
/// Chat Completions puts it at the top level. The Responses API puts it there
/// too on a non-streaming reply, but nests it under `response` on the
/// streaming lifecycle events (`response.completed`) that Codex's
/// `wire_api = "responses"` lane actually returns. Reading only the top level
/// records the call and its generation id while silently losing every token.
fn usage_object(body: &Value) -> Value {
    for pointer in ["/usage", "/response/usage"] {
        if let Some(usage) = body.pointer(pointer).filter(|value| value.is_object()) {
            return usage.clone();
        }
    }
    Value::Null
}

pub fn parse_usage(body: &Value) -> MeasuredUsage {
    let usage = usage_object(body);
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

/// Recover response identity and inline usage from an OpenAI-compatible SSE
/// response. The proxy buffers provider bodies before relaying them, so it can
/// account for the final `stream_options.include_usage` chunk without changing
/// a byte of the worker-visible stream.
pub fn parse_sse_usage(bytes: &[u8]) -> (Option<String>, MeasuredUsage) {
    let text = String::from_utf8_lossy(bytes);
    let mut response_id = None;
    let mut measured = MeasuredUsage {
        calls: 1,
        input_tokens: 0,
        output_tokens: 0,
        cost_usd: None,
    };
    for line in text.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let Ok(chunk) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        if response_id.is_none() {
            response_id = response_id_for_owned(&chunk);
        }
        // Chat Completions reports usage on the final chunk; the Responses
        // API reports it inside `response` on `response.completed`. Both are
        // the same accounting record and neither may be dropped.
        if usage_object(&chunk).is_object() {
            let usage = parse_usage(&chunk);
            measured.input_tokens = usage.input_tokens;
            measured.output_tokens = usage.output_tokens;
            measured.cost_usd = usage.cost_usd;
        }
    }
    (response_id, measured)
}

fn response_id_for_owned(body: &Value) -> Option<String> {
    response_id(body).map(str::to_owned)
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

