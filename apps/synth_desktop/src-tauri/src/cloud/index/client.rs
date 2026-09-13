//! Minimal authenticated Synth Index contributor client (backend `/api/v1/index`).
//! The API key never leaves Rust. Storage transfers use a separate client with
//! no credentials, no redirects and only validated HTTPS (or loopback) targets.

use reqwest::{Client, Method, StatusCode, Url};
use serde::Serialize;
use serde_json::Value;
use std::{collections::BTreeMap, fmt, time::Duration};

const USER_AGENT: &str = "synth-desktop-index/0.1";

#[derive(Debug)]
pub enum IndexClientError {
    Configuration(String),
    Transport(reqwest::Error),
    Http { status: StatusCode, code: String },
    Protocol(String),
}

impl fmt::Display for IndexClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(message) => write!(f, "Index client configuration: {message}"),
            Self::Transport(error) => write!(f, "Index backend unavailable: {error}"),
            Self::Http { status, code } => write!(f, "Index HTTP {}: {code}", status.as_u16()),
            Self::Protocol(message) => write!(f, "Index protocol error: {message}"),
        }
    }
}

impl std::error::Error for IndexClientError {}

fn protocol(message: impl Into<String>) -> IndexClientError {
    IndexClientError::Protocol(message.into())
}

fn segment(value: &str) -> Result<&str, IndexClientError> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value.starts_with(|c: char| c.is_ascii_alphanumeric())
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_.-".contains(c));
    if valid {
        Ok(value)
    } else {
        Err(protocol(format!("{value:?} is not an Index identifier")))
    }
}

/// Validate one prepared storage target before any byte leaves the machine.
pub fn validate_upload_target(
    url: &str,
    headers: &BTreeMap<String, String>,
) -> Result<Url, IndexClientError> {
    let parsed = Url::parse(url).map_err(|error| protocol(format!("upload URL: {error}")))?;
    let loopback = matches!(
        parsed.host_str(),
        Some("127.0.0.1" | "localhost" | "[::1]" | "::1")
    );
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.fragment().is_some()
        || !(parsed.scheme() == "https" || (parsed.scheme() == "http" && loopback))
    {
        return Err(protocol(
            "upload target must be HTTPS or explicit loopback storage",
        ));
    }
    if headers.keys().any(|name| {
        matches!(
            name.to_ascii_lowercase().as_str(),
            "authorization" | "cookie" | "proxy-authorization" | "host"
        )
    }) {
        return Err(protocol(
            "upload target carries forbidden credential or routing headers",
        ));
    }
    Ok(parsed)
}

#[derive(Clone)]
pub struct IndexClient {
    base_url: Url,
    api_key: String,
    http: Client,
}

impl IndexClient {
    pub fn connect(
        base_url: &str,
        api_key: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, IndexClientError> {
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return Err(IndexClientError::Configuration(
                "API key is required".into(),
            ));
        }
        let base_url =
            Url::parse(&format!("{}/", base_url.trim_end_matches('/'))).map_err(|error| {
                IndexClientError::Configuration(format!("invalid backend URL: {error}"))
            })?;
        if !matches!(base_url.scheme(), "http" | "https") {
            return Err(IndexClientError::Configuration(
                "backend URL must use http or https".into(),
            ));
        }
        let http = crate::http::http_client_builder()
            .timeout(timeout)
            .user_agent(USER_AGENT)
            .build()
            .map_err(IndexClientError::Transport)?;
        Ok(Self {
            base_url,
            api_key,
            http,
        })
    }

    async fn json<B: Serialize>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
        idempotency_key: Option<&str>,
    ) -> Result<Value, IndexClientError> {
        let url = self
            .base_url
            .join(path)
            .map_err(|error| protocol(format!("request path: {error}")))?;
        let mut request = self
            .http
            .request(method, url)
            .bearer_auth(&self.api_key)
            .header("Accept", "application/json");
        if let Some(key) = idempotency_key {
            request = request.header("Idempotency-Key", key);
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.map_err(IndexClientError::Transport)?;
        let status = response.status();
        let payload: Value = response.json().await.unwrap_or(Value::Null);
        if !status.is_success() {
            let code = payload
                .pointer("/detail/code")
                .and_then(Value::as_str)
                .unwrap_or("index_unavailable")
                .to_owned();
            return Err(IndexClientError::Http { status, code });
        }
        Ok(payload)
    }

    fn revision_path(
        contribution_id: &str,
        revision_id: &str,
        suffix: &str,
    ) -> Result<String, IndexClientError> {
        Ok(format!(
            "api/v1/index/contributions/{}/revisions/{}{suffix}",
            segment(contribution_id)?,
            segment(revision_id)?
        ))
    }

    /// Private, user-owned draft; replaying the key never allocates a second one.
    pub async fn create_draft(&self, idempotency_key: &str) -> Result<Value, IndexClientError> {
        segment(idempotency_key)?;
        self.json(
            Method::POST,
            "api/v1/index/contributions",
            Some(&serde_json::json!({})),
            Some(idempotency_key),
        )
        .await
    }

    pub async fn prepare_upload(
        &self,
        contribution_id: &str,
        revision_id: &str,
        publication_id: &str,
        package: &Value,
    ) -> Result<Value, IndexClientError> {
        let body = serde_json::json!({"publication_id": publication_id, "package": package});
        let path = Self::revision_path(contribution_id, revision_id, "/upload")?;
        self.json(Method::POST, &path, Some(&body), None).await
    }

    pub async fn finalize(
        &self,
        contribution_id: &str,
        revision_id: &str,
        publication_id: &str,
    ) -> Result<Value, IndexClientError> {
        let body = serde_json::json!({"publication_id": publication_id});
        let path = Self::revision_path(contribution_id, revision_id, "/finalize")?;
        self.json(Method::POST, &path, Some(&body), None).await
    }

    /// Seal for independent review. Never approves or publishes.
    pub async fn submit(
        &self,
        contribution_id: &str,
        revision_id: &str,
        publication_id: &str,
    ) -> Result<Value, IndexClientError> {
        let body = serde_json::json!({"publication_id": publication_id});
        let path = Self::revision_path(contribution_id, revision_id, "/submit")?;
        self.json(Method::POST, &path, Some(&body), None).await
    }

    pub async fn revision(
        &self,
        contribution_id: &str,
        revision_id: &str,
    ) -> Result<Value, IndexClientError> {
        let path = Self::revision_path(contribution_id, revision_id, "")?;
        self.json::<Value>(Method::GET, &path, None, None).await
    }
}

/// PUT prepared bytes with a fresh credential-free client. Targets are validated
/// first so no byte is sent if any instruction is unsafe.
pub async fn transfer(
    targets: &[(String, BTreeMap<String, String>, Vec<u8>)],
    timeout: Duration,
) -> Result<(), IndexClientError> {
    let urls = targets
        .iter()
        .map(|(url, headers, _)| validate_upload_target(url, headers))
        .collect::<Result<Vec<_>, _>>()?;
    let storage = Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .user_agent(USER_AGENT)
        .build()
        .map_err(IndexClientError::Transport)?;
    for (url, (_, headers, bytes)) in urls.into_iter().zip(targets) {
        let mut request = storage.put(url).body(bytes.clone());
        for (name, value) in headers {
            request = request.header(name, value);
        }
        let response = request.send().await.map_err(IndexClientError::Transport)?;
        if !matches!(response.status().as_u16(), 200 | 201 | 204) {
            return Err(protocol(format!(
                "storage transfer failed with HTTP {}",
                response.status()
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upload_targets_must_be_https_or_loopback_without_credentials() {
        let none = BTreeMap::new();
        assert!(validate_upload_target("https://storage.example/obj?sig=1", &none).is_ok());
        assert!(validate_upload_target("http://127.0.0.1:9000/obj", &none).is_ok());
        assert!(validate_upload_target("http://storage.example/obj", &none).is_err());
        assert!(validate_upload_target("https://user:pw@storage.example/obj", &none).is_err());
        let auth = BTreeMap::from([("Authorization".to_owned(), "Bearer x".to_owned())]);
        assert!(validate_upload_target("https://storage.example/obj", &auth).is_err());
    }

    #[test]
    fn path_segments_are_identifiers() {
        assert!(segment("ctr_a").is_ok());
        assert!(segment("../x").is_err());
        assert!(IndexClient::revision_path("ctr_a", "rev/1", "").is_err());
    }
}
