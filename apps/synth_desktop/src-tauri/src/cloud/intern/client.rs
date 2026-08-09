use super::models::*;
use reqwest::{Client, Method, StatusCode, Url};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::{error::Error, fmt, time::Duration};

const USER_AGENT: &str = "synth-desktop-rust/0.1";

#[derive(Debug)]
pub enum InternClientError {
    Configuration(String),
    Transport(reqwest::Error),
    Http { status: StatusCode, detail: String },
    Protocol(String),
}

impl InternClientError {
    pub fn is_auth_failure(&self) -> bool {
        matches!(self, Self::Http { status, .. } if *status == StatusCode::UNAUTHORIZED || *status == StatusCode::FORBIDDEN)
    }

    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Transport(_) => true,
            Self::Http { status, .. } => {
                status.is_server_error() || *status == StatusCode::TOO_MANY_REQUESTS
            }
            Self::Configuration(_) | Self::Protocol(_) => false,
        }
    }
}

impl fmt::Display for InternClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Configuration(message) => write!(f, "Intern client configuration: {message}"),
            Self::Transport(error) => write!(f, "Intern backend unavailable: {error}"),
            Self::Http { status, detail } => write!(f, "Intern HTTP {}: {detail}", status.as_u16()),
            Self::Protocol(message) => write!(f, "Intern protocol error: {message}"),
        }
    }
}

impl Error for InternClientError {}

#[derive(Clone)]
pub struct InternClient {
    base_url: Url,
    api_key: String,
    http: Client,
}

impl InternClient {
    pub fn new(
        base_url: &str,
        api_key: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, InternClientError> {
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return Err(InternClientError::Configuration(
                "API key is required".into(),
            ));
        }
        let normalized = format!("{}/", base_url.trim_end_matches('/'));
        let base_url = Url::parse(&normalized).map_err(|error| {
            InternClientError::Configuration(format!("invalid backend URL: {error}"))
        })?;
        if !matches!(base_url.scheme(), "http" | "https") {
            return Err(InternClientError::Configuration(
                "backend URL must use http or https".into(),
            ));
        }
        let http = Client::builder()
            .timeout(timeout)
            .user_agent(USER_AGENT)
            .build()
            .map_err(InternClientError::Transport)?;
        Ok(Self {
            base_url,
            api_key,
            http,
        })
    }

    pub async fn create_sync(
        &self,
        request: &SyncCreateRequest,
    ) -> Result<RuntimeProjection, InternClientError> {
        request.validate().map_err(protocol)?;
        self.json(
            Method::POST,
            "smr/research-intern/sync-sessions",
            Some(request),
            &[],
        )
        .await
    }

    pub async fn get_sync(&self, runtime_id: &str) -> Result<RuntimeProjection, InternClientError> {
        self.json::<(), _>(
            Method::GET,
            &format!("smr/research-intern/sync-sessions/{}", segment(runtime_id)?),
            None,
            &[],
        )
        .await
    }

    pub async fn command_sync(
        &self,
        runtime_id: &str,
        request: &SyncCommandRequest,
    ) -> Result<CommandReceipt, InternClientError> {
        request.validate().map_err(protocol)?;
        let receipt: CommandReceipt = self
            .json(
                Method::POST,
                &format!(
                    "smr/research-intern/sync-sessions/{}/commands",
                    segment(runtime_id)?
                ),
                Some(request),
                &[],
            )
            .await?;
        receipt
            .validate_for(&request.command_id)
            .map_err(protocol)?;
        if receipt.runtime_kind != RuntimeKind::Sync || receipt.runtime_id != runtime_id {
            return Err(protocol("Sync command receipt runtime identity drifted"));
        }
        Ok(receipt)
    }

    pub async fn sync_events(
        &self,
        runtime_id: &str,
        after_sequence: u64,
        limit: u16,
    ) -> Result<Vec<InternEvent>, InternClientError> {
        self.events(
            &format!(
                "smr/research-intern/runtimes/sync/{}/events",
                segment(runtime_id)?
            ),
            after_sequence,
            limit,
        )
        .await
    }

    pub async fn ensure_async(
        &self,
        request: &AsyncEnsureRequest,
    ) -> Result<RuntimeProjection, InternClientError> {
        request.validate().map_err(protocol)?;
        let projection: RuntimeProjection = self
            .json(
                Method::POST,
                "smr/research-intern/async/ensure",
                Some(request),
                &[],
            )
            .await?;
        projection.validate_async_identity().map_err(protocol)?;
        Ok(projection)
    }

    pub async fn get_async(&self) -> Result<RuntimeProjection, InternClientError> {
        let projection: RuntimeProjection = self
            .json::<(), _>(Method::GET, "smr/research-intern/async", None, &[])
            .await?;
        projection.validate_async_identity().map_err(protocol)?;
        Ok(projection)
    }

    /// Sends a conversational/instruction command through the Desktop-used
    /// `/messages` alias. Lifecycle controls use [`Self::command_async`].
    pub async fn send_async(
        &self,
        request: &AsyncCommandRequest,
    ) -> Result<CommandReceipt, InternClientError> {
        request.validate().map_err(protocol)?;
        if !matches!(
            request.command_kind,
            AsyncCommandKind::Message
                | AsyncCommandKind::Intervene
                | AsyncCommandKind::RedirectObjective
                | AsyncCommandKind::RequestCheckpoint
        ) {
            return Err(protocol(
                "async send only accepts message/instruction commands",
            ));
        }
        self.post_async_command("smr/research-intern/async/messages", request)
            .await
    }

    pub async fn command_async(
        &self,
        request: &AsyncCommandRequest,
    ) -> Result<CommandReceipt, InternClientError> {
        request.validate().map_err(protocol)?;
        self.post_async_command("smr/research-intern/async/commands", request)
            .await
    }

    pub async fn async_events(
        &self,
        after_sequence: u64,
        limit: u16,
    ) -> Result<Vec<InternEvent>, InternClientError> {
        self.events("smr/research-intern/async/events", after_sequence, limit)
            .await
    }

    async fn post_async_command(
        &self,
        path: &str,
        request: &AsyncCommandRequest,
    ) -> Result<CommandReceipt, InternClientError> {
        let receipt: CommandReceipt = self.json(Method::POST, path, Some(request), &[]).await?;
        receipt
            .validate_for(&request.command_id)
            .map_err(protocol)?;
        if receipt.runtime_kind != RuntimeKind::Async {
            return Err(protocol("Async command receipt runtime kind drifted"));
        }
        Ok(receipt)
    }

    async fn events(
        &self,
        path: &str,
        after_sequence: u64,
        limit: u16,
    ) -> Result<Vec<InternEvent>, InternClientError> {
        let bounded = limit.clamp(1, 500);
        let after = after_sequence.to_string();
        let limit = bounded.to_string();
        let response: EventListResponse = self
            .json::<(), _>(
                Method::GET,
                path,
                None,
                &[("after_sequence", &after), ("limit", &limit)],
            )
            .await?;
        let events = response.into_events();
        for event in &events {
            event.validate().map_err(protocol)?;
        }
        Ok(events)
    }

    async fn json<B: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
        query: &[(&str, &str)],
    ) -> Result<R, InternClientError> {
        let url = self.base_url.join(path).map_err(|error| {
            InternClientError::Configuration(format!("invalid Intern path: {error}"))
        })?;
        let mut request = self
            .http
            .request(method, url)
            .bearer_auth(&self.api_key)
            .query(query);
        if let Some(body) = body {
            request = request.json(body);
        }
        let response = request.send().await.map_err(InternClientError::Transport)?;
        let status = response.status();
        if !status.is_success() {
            let value = response.json::<Value>().await.unwrap_or(Value::Null);
            let detail = value
                .get("detail")
                .and_then(Value::as_str)
                .unwrap_or("request failed")
                .to_owned();
            return Err(InternClientError::Http { status, detail });
        }
        response
            .json()
            .await
            .map_err(|error| protocol(format!("invalid JSON response: {error}")))
    }
}

fn segment(value: &str) -> Result<&str, InternClientError> {
    if value.is_empty()
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(InternClientError::Configuration(
            "invalid runtime id".into(),
        ));
    }
    Ok(value)
}

fn protocol(message: impl fmt::Display) -> InternClientError {
    InternClientError::Protocol(message.to_string())
}
