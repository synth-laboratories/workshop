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
            && !id.bytes().all(|byte| byte == b'.')
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

/// Only HTTPS hosts, or plain HTTP on loopback, without credentials/query/fragment.
fn validate_base_url(base: &str) -> Result<reqwest::Url, AppError> {
    let base_url = reqwest::Url::parse(base)
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
    Ok(base_url)
}

fn parse_request(request_json: &str) -> Result<Request, AppError> {
    if request_json.len() > MAX_REQUEST_BYTES {
        return Err(AppError::invalid_argument(
            "Index request exceeds the byte bound",
        ));
    }
    let request: Request = serde_json::from_str(request_json)
        .map_err(|_| AppError::invalid_argument("Invalid Index relay request"))?;
    validate_request(&request)?;
    Ok(request)
}

/// Bounded native public transport. JSON strings keep tokens out of typed diagnostics.
#[tauri::command]
#[specta::specta]
pub async fn index_public_search_request(request_json: String) -> Result<String, AppError> {
    let request = parse_request(&request_json)?;
    let base = synth_config::public_backend_url()
        .map_err(|_| AppError::untyped("Index backend configuration is unavailable"))?;
    relay(&base, request, Duration::from_secs(120)).await
}

async fn relay(base: &str, request: Request, timeout: Duration) -> Result<String, AppError> {
    validate_base_url(base)?;
    let client = http::http_client_builder()
        .timeout(timeout)
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

    #[test]
    fn relay_rejects_dot_segment_lifecycle_ids() {
        for path in [
            "/api/v1/index/public/searches/.",
            "/api/v1/index/public/searches/..",
            "/api/v1/index/public/searches/../cancel",
            "/api/v1/index/public/searches/%2e%2e",
        ] {
            let mut poll = request(
                if path.ends_with("/cancel") {
                    "POST"
                } else {
                    "GET"
                },
                path,
            );
            poll.headers
                .insert("x-search-token".into(), "token_12345678".into());
            assert!(validate_request(&poll).is_err(), "{path} must be refused");
        }
    }

    #[test]
    fn relay_refuses_bodies_off_create_and_oversize_envelopes() {
        let mut capabilities = request("GET", "/api/v1/index/public/capabilities");
        capabilities.body = Some(serde_json::json!({"query": "q"}));
        assert!(validate_request(&capabilities).is_err());
        let mut create = request("POST", "/api/v1/index/public/search");
        create
            .headers
            .insert("x-search-token".into(), "token_12345678".into());
        assert!(validate_request(&create).is_err(), "no token on create");
        let oversize = format!(
            r#"{{"method":"POST","path":"/api/v1/index/public/search","headers":{{}},"identity":"anonymous","body":{{"query":"{}"}}}}"#,
            "q".repeat(MAX_REQUEST_BYTES)
        );
        assert!(parse_request(&oversize).is_err());
        assert!(parse_request(
            r#"{"method":"GET","path":"/api/v1/index/public/capabilities","headers":{},"identity":"anonymous","body":null,"api_key":"x"}"#
        )
        .is_err(), "unknown envelope fields are refused");
    }

    #[test]
    fn relay_base_url_requires_https_off_loopback() {
        assert!(validate_base_url("https://api.example.test").is_ok());
        assert!(validate_base_url("http://127.0.0.1:8000").is_ok());
        assert!(validate_base_url("http://localhost:8000").is_ok());
        assert!(validate_base_url("http://[::1]:8000").is_ok());
        for base in [
            "http://api.example.test",
            "http://localhost.example.test",
            "ftp://api.example.test",
            "https://user:pass@api.example.test",
            "https://api.example.test/?key=secret",
            "https://api.example.test/#frag",
            "not a url",
        ] {
            assert!(validate_base_url(base).is_err(), "{base} must be refused");
        }
    }

    /// One-shot loopback HTTP server: returns the raw request it saw.
    async fn serve_once(response: Vec<u8>) -> (String, tokio::task::JoinHandle<String>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let handle = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut seen = Vec::new();
            let mut buffer = [0u8; 4096];
            loop {
                let read = socket.read(&mut buffer).await.unwrap();
                seen.extend_from_slice(&buffer[..read]);
                let text = String::from_utf8_lossy(&seen);
                if let Some(end) = text.find("\r\n\r\n") {
                    let length = text[..end]
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|value| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if seen.len() >= end + 4 + length {
                        break;
                    }
                }
                if read == 0 {
                    break;
                }
            }
            socket.write_all(&response).await.ok();
            socket.shutdown().await.ok();
            String::from_utf8_lossy(&seen).into_owned()
        });
        (base, handle)
    }

    fn parse(json: &str) -> Request {
        parse_request(json).unwrap()
    }

    #[tokio::test]
    async fn relay_sends_no_credentials_and_filters_response_headers() {
        let body = r#"{"state":"running"}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\nset-cookie: s=1\r\nx-search-token: tok_next_12345\r\nx-internal-debug: leak\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        let (base, server) = serve_once(response.into_bytes()).await;
        let out = relay(
            &base,
            parse(r#"{"method":"GET","path":"/api/v1/index/public/searches/srch_1","headers":{"x-search-token":"token_12345678"},"identity":"anonymous","body":null}"#),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        let seen = server.await.unwrap().to_ascii_lowercase();
        assert!(seen.starts_with("get /api/v1/index/public/searches/srch_1 http/1.1"));
        assert!(seen.contains("x-search-token: token_12345678"));
        for forbidden in ["authorization", "cookie", "x-api-key"] {
            assert!(!seen.contains(forbidden), "relay sent {forbidden}");
        }
        let out: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(out["status"], 200);
        assert_eq!(out["body"]["state"], "running");
        let headers = out["headers"].as_object().unwrap();
        assert_eq!(
            headers.len(),
            1,
            "only whitelisted headers return: {headers:?}"
        );
        assert_eq!(headers["x-search-token"], "tok_next_12345");
    }

    #[tokio::test]
    async fn relay_does_not_follow_redirects() {
        let response = b"HTTP/1.1 302 Found\r\nlocation: http://127.0.0.1:9/steal\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_vec();
        let (base, server) = serve_once(response).await;
        let out = relay(
            &base,
            parse(r#"{"method":"GET","path":"/api/v1/index/public/capabilities","headers":{},"identity":"anonymous","body":null}"#),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        server.await.unwrap();
        let out: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(out["status"], 302);
        assert!(out["headers"].as_object().unwrap().is_empty());
    }

    #[tokio::test]
    async fn relay_bounds_response_size_with_and_without_content_length() {
        let big = "x".repeat(MAX_RESPONSE_BYTES + 1);
        for declared in [true, false] {
            let head = if declared {
                format!(
                    "HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    big.len()
                )
            } else {
                "HTTP/1.1 200 OK\r\nconnection: close\r\n\r\n".to_owned()
            };
            let (base, server) = serve_once(format!("{head}{big}").into_bytes()).await;
            let result = relay(
                &base,
                parse(r#"{"method":"GET","path":"/api/v1/index/public/capabilities","headers":{},"identity":"anonymous","body":null}"#),
                Duration::from_secs(5),
            )
            .await;
            assert!(result.is_err(), "declared={declared} must exceed the bound");
            server.abort();
        }
    }

    #[tokio::test]
    async fn relay_times_out_a_silent_backend() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let hold = tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(10)).await;
        });
        let result = relay(
            &base,
            parse(r#"{"method":"GET","path":"/api/v1/index/public/capabilities","headers":{},"identity":"anonymous","body":null}"#),
            Duration::from_millis(300),
        )
        .await;
        assert!(result.is_err());
        hold.abort();
    }
}
