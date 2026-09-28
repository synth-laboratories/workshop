//! Anonymous Index relay for hosted profiles; no account credentials or persistence.
//! See apps/synth_desktop/INDEX_SEARCH_BRIDGE.md.

use crate::{http, synth_config, AppError};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::time::Duration;

const MAX_REQUEST_BYTES: usize = 16_384;
const MAX_RESPONSE_BYTES: usize = 262_144;
const SEARCHES: &str = "/api/v1/index/public/searches/";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Option<serde_json::Value>,
    identity: String,
}

fn validate_request(request: &Request) -> Result<(), AppError> {
    if request.identity != "anonymous" {
        return Err(AppError::invalid_argument(
            "Index public relay requires anonymous identity",
        ));
    }
    let start = request.method == "POST" && request.path == "/api/v1/index/public/search";
    let capabilities =
        request.method == "GET" && request.path == "/api/v1/index/public/capabilities";
    let lifecycle = request.path.strip_prefix(SEARCHES).is_some_and(|suffix| {
        let (id, cancel) = suffix
            .strip_suffix("/cancel")
            .map_or((suffix, false), |id| (id, true));
        !id.is_empty()
            && id.len() <= 160
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
            && request.method == if cancel { "POST" } else { "GET" }
    });
    if !start && !capabilities && !lifecycle {
        return Err(AppError::invalid_argument(
            "Index relay route is not allowed",
        ));
    }
    if !start && request.body.is_some() {
        return Err(AppError::invalid_argument(
            "Index lifecycle requests cannot carry a body",
        ));
    }
    if request.headers.iter().any(|(key, value)| {
        !key.eq_ignore_ascii_case("x-search-token")
            || !lifecycle
            || value.len() < 8
            || value.len() > 1024
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.~=:-".contains(&byte))
    }) {
        return Err(AppError::invalid_argument(
            "Index relay header is not allowed",
        ));
    }
    if lifecycle && request.headers.len() != 1 {
        return Err(AppError::invalid_argument(
            "Index lifecycle request requires its search token",
        ));
    }
    Ok(())
}

/// Bounded native public transport. JSON strings keep tokens out of typed diagnostics.
#[tauri::command]
#[specta::specta]
pub async fn index_public_search_request(request_json: String) -> Result<String, AppError> {
    if request_json.len() > MAX_REQUEST_BYTES {
        return Err(AppError::invalid_argument(
            "Index request exceeds the byte bound",
        ));
    }
    let request: Request = serde_json::from_str(&request_json)
        .map_err(|_| AppError::invalid_argument("Invalid Index relay request"))?;
    validate_request(&request)?;
    let base = synth_config::public_backend_url()
        .map_err(|_| AppError::untyped("Index backend configuration is unavailable"))?;
    let base_url = reqwest::Url::parse(&base)
        .map_err(|_| AppError::invalid_argument("Invalid Index backend URL"))?;
    let loopback = matches!(
        base_url.host_str(),
        Some("localhost" | "127.0.0.1" | "[::1]" | "::1")
    );
    if !base_url.username().is_empty()
        || base_url.password().is_some()
        || base_url.query().is_some()
        || base_url.fragment().is_some()
    {
        return Err(AppError::invalid_argument(
            "Index backend URL must not contain credentials, query or fragment",
        ));
    }
    if base_url.scheme() != "https" && !(base_url.scheme() == "http" && loopback) {
        return Err(AppError::invalid_argument(
            "Hosted Index transport requires HTTPS",
        ));
    }
    let client = http::http_client_builder()
        .timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| AppError::untyped("Index transport could not initialize"))?;
    let method = if request.method == "POST" {
        reqwest::Method::POST
    } else {
        reqwest::Method::GET
    };
    let mut outbound = client.request(
        method,
        format!("{}{}", base.trim_end_matches('/'), request.path),
    );
    for (key, value) in request.headers {
        outbound = outbound.header(key, value);
    }
    if let Some(body) = request.body {
        outbound = outbound.json(&body);
    }
    let mut response = outbound
        .send()
        .await
        .map_err(|_| AppError::untyped("Index transport is unavailable"))?;
    let status = response.status().as_u16();
    let headers: BTreeMap<String, String> = response
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            let name = name.as_str();
            matches!(
                name,
                "retry-after"
                    | "x-index-search-id"
                    | "x-search-token"
                    | "x-search-token-expires-at"
                    | "x-index-customer-charge-cents"
                    | "x-index-monitor-release"
                    | "x-index-monitor-delivery"
            )
            .then(|| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.to_owned(), value.to_owned()))
            })
            .flatten()
        })
        .collect();
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(AppError::untyped("Index response exceeds the byte bound"));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| AppError::untyped("Index response was interrupted"))?
    {
        if bytes.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(AppError::untyped("Index response exceeds the byte bound"));
        }
        bytes.extend_from_slice(&chunk);
    }
    let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
    serde_json::to_string(&serde_json::json!({"status": status, "headers": headers, "body": body}))
        .map_err(|_| AppError::untyped("Index response could not be encoded"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(method: &str, path: &str) -> Request {
        Request {
            method: method.into(),
            path: path.into(),
            headers: BTreeMap::new(),
            body: None,
            identity: "anonymous".into(),
        }
    }

    #[test]
    fn relay_only_accepts_public_routes_and_lifecycle_tokens() {
        assert!(validate_request(&request("GET", "/api/v1/index/public/capabilities")).is_ok());
        assert!(validate_request(&request("POST", "/api/v1/index/public/search")).is_ok());
        for path in [
            "/api/v1/index/search",
            "/api/v1/admin/logs",
            "/api/v1/index/public/searches/../admin",
            "/api/v1/index/public/searches/id?token=secret",
        ] {
            assert!(validate_request(&request("GET", path)).is_err());
        }
        let mut poll = request("GET", "/api/v1/index/public/searches/id");
        assert!(validate_request(&poll).is_err());
        poll.headers
            .insert("X-Search-Token".into(), "token_12345678".into());
        assert!(validate_request(&poll).is_ok());
        poll.headers
            .insert("Authorization".into(), "Bearer secret".into());
        assert!(validate_request(&poll).is_err());
        let mut account = request("POST", "/api/v1/index/public/search");
        account.identity = "account".into();
        assert!(validate_request(&account).is_err());
    }
}
