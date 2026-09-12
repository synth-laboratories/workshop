//! Grant-credential MQ transport (contract §6, §8).
//!
//! Publish and granted `/history` reads use the vendored `mq_sdk::MqClient`
//! (snapshot 02db5d4): canonical origin, no redirects, a 30 s request
//! deadline and bounded bodies. The SDK has no SSE reader, so the wake
//! stream lives here with the same hardening (no redirects, bounded
//! connection setup and line length).
use super::grant::{validate_origin, EndpointPolicy, SecretToken};
use crate::cloud::storage::MqHistoryPage;
use anyhow::Result;
use futures_util::StreamExt;
use mq_core::{Message, PublishMessage, ThreadId};
use serde_json::Value;

const MAX_ERROR_BYTES: usize = 64 * 1024;
const MAX_SSE_LINE: usize = 8 * 1024;

/// Classified MQ failure (contract §7). Only `Uncertain` can hide a
/// committed publish; it is never treated as a rejection or retried blindly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MqCallError {
    /// 401/403 with the contract code (`grant_revoked`, `unauthenticated`, …).
    Denied { status: u16, code: String },
    /// Other definitive 4xx refusal.
    Rejected { status: u16, code: String },
    /// Transport loss, timeout, 5xx or unreadable success: outcome unknown.
    Uncertain(String),
}

impl std::fmt::Display for MqCallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Denied { status, code } => write!(f, "MQ denied ({status} {code})"),
            Self::Rejected { status, code } => write!(f, "MQ rejected ({status} {code})"),
            Self::Uncertain(detail) => write!(f, "MQ outcome uncertain: {detail}"),
        }
    }
}
impl std::error::Error for MqCallError {}

fn error_code(body: &[u8]) -> String {
    serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| match value.get("error") {
            Some(Value::String(code)) => Some(code.clone()),
            Some(Value::Object(object)) => object.get("code").and_then(Value::as_str).map(str::to_owned),
            _ => value.pointer("/detail/code").and_then(Value::as_str).map(str::to_owned),
        })
        .unwrap_or_else(|| "unknown".into())
}

fn classify(status: u16, body: &[u8]) -> MqCallError {
    let code = error_code(body);
    match status {
        401 | 403 => MqCallError::Denied { status, code },
        400..=499 => MqCallError::Rejected { status, code },
        _ => MqCallError::Uncertain(format!("server status {status}")),
    }
}

/// Wake hints only. They never advance a durable cursor (contract §8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WakeEvent {
    Wake,
    Resync,
    Revoked,
}

pub struct MqGrantTransport {
    stream_http: reqwest::Client,
    sdk: mq_sdk::MqClient,
    origin: String,
    token: SecretToken,
    thread: ThreadId,
}

impl MqGrantTransport {
    pub fn try_new(endpoint: &str, token: SecretToken, thread: ThreadId, policy: EndpointPolicy) -> Result<Self> {
        let origin = validate_origin(endpoint, policy)?;
        let sdk = mq_sdk::MqClient::try_new(origin.clone(), token.expose().to_owned())
            .map_err(|_| anyhow::anyhow!("invalid MQ grant credential configuration"))?;
        Ok(Self {
            // Streams stay open; only connection establishment is bounded.
            stream_http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            sdk,
            origin,
            token,
            thread,
        })
    }

    /// `GET /v1/threads/{id}/history?after_seq=&limit=` (1..=200) through
    /// the SDK. The page is still re-validated before any commit.
    pub async fn history(&self, after_seq: u64, limit: usize) -> Result<MqHistoryPage, MqCallError> {
        let limit = limit.clamp(1, 200);
        let page = self.sdk.read_history(self.thread, after_seq, limit).await.map_err(sdk_error)?;
        if page.messages.len() > limit || page.thread_id != self.thread {
            return Err(MqCallError::Uncertain("history page exceeds the requested bound or thread".into()));
        }
        Ok(page)
    }

    /// Publish the exact persisted request through the SDK.
    pub async fn publish(&self, request: PublishMessage) -> Result<Message, MqCallError> {
        self.sdk.publish(self.thread, request).await.map_err(sdk_error)
    }

    /// Open the wake stream. Events are hints; the caller fetches history.
    pub async fn wakes(&self) -> Result<WakeStream, MqCallError> {
        let response = self
            .stream_http
            .get(format!("{}/v1/threads/{}/events", self.origin, self.thread.0))
            .header("authorization", format!("Bearer {}", self.token.expose()))
            .header("accept", "text/event-stream")
            .send()
            .await
            .map_err(|error| MqCallError::Uncertain(error.without_url().to_string()))?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            let body = bounded(response, MAX_ERROR_BYTES).await.unwrap_or_default();
            return Err(classify(status, &body));
        }
        Ok(WakeStream { bytes: Box::pin(response.bytes_stream()), buffer: Vec::new(), event: None })
    }
}

fn sdk_error(error: mq_sdk::SdkError) -> MqCallError {
    match error {
        mq_sdk::SdkError::Api { status, body } => classify(status.as_u16(), body.as_bytes()),
        // A transport failure or an unreadable 2xx may hide a commit.
        mq_sdk::SdkError::Http(error) => MqCallError::Uncertain(error.without_url().to_string()),
        mq_sdk::SdkError::Decode(detail) => MqCallError::Uncertain(detail),
    }
}

async fn bounded(response: reqwest::Response, limit: usize) -> Result<Vec<u8>, MqCallError> {
    if response.content_length().is_some_and(|length| length > limit as u64) {
        return Err(MqCallError::Uncertain("response exceeds limit".into()));
    }
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| MqCallError::Uncertain(error.without_url().to_string()))?;
        if chunk.len() > limit - body.len() {
            return Err(MqCallError::Uncertain("response exceeds limit".into()));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

type ByteStream = std::pin::Pin<Box<dyn futures_util::Stream<Item = reqwest::Result<bytes::Bytes>> + Send>>;

/// Minimal `text/event-stream` reader for `thread_wake`/`resync`/`revoked`.
pub struct WakeStream {
    bytes: ByteStream,
    buffer: Vec<u8>,
    event: Option<String>,
}

impl WakeStream {
    /// `None` when the server closed the stream.
    pub async fn next(&mut self) -> Option<Result<WakeEvent, MqCallError>> {
        loop {
            while let Some(position) = self.buffer.iter().position(|byte| *byte == b'\n') {
                let line: Vec<u8> = self.buffer.drain(..=position).collect();
                let line = String::from_utf8_lossy(&line).trim_end_matches(['\r', '\n']).to_owned();
                if line.is_empty() {
                    if let Some(event) = self.event.take() {
                        match event.as_str() {
                            "thread_wake" => return Some(Ok(WakeEvent::Wake)),
                            "resync" => return Some(Ok(WakeEvent::Resync)),
                            "revoked" => return Some(Ok(WakeEvent::Revoked)),
                            _ => {}
                        }
                    }
                } else if let Some(name) = line.strip_prefix("event:") {
                    self.event = Some(name.trim().to_owned());
                }
                // `data:` carries a thread id or reason; comments are keep-alives.
            }
            if self.buffer.len() > MAX_SSE_LINE {
                return Some(Err(MqCallError::Uncertain("wake stream line exceeds limit".into())));
            }
            match self.bytes.next().await {
                Some(Ok(chunk)) => self.buffer.extend_from_slice(&chunk),
                Some(Err(error)) => return Some(Err(MqCallError::Uncertain(error.without_url().to_string()))),
                None => return None,
            }
        }
    }
}
