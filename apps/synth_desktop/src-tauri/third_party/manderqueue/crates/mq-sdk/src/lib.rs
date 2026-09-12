//! Typed HTTP client for Manderqueue.

mod catch_up;
pub use catch_up::{CatchUpOutcome, CatchUpSupervisor};

use mq_core::{
    CreateThread, Message, Participant, PrincipalKind, PublishMessage, Role, ScopeBinding, ScopeKind, Thread, ThreadId,
};
use reqwest::{Client, StatusCode};
use thiserror::Error;
use uuid::Uuid;

const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

async fn bounded_body(mut response: reqwest::Response) -> Result<Vec<u8>, SdkError> {
    let status = response.status();
    let limit = if status.is_success() { MAX_RESPONSE_BYTES } else { 64 * 1024 };
    let too_large = || if status.is_success() {
        SdkError::Decode("response body exceeds limit".into())
    } else {
        SdkError::Api { status, body: "response body exceeds limit".into() }
    };
    if response.content_length().is_some_and(|length| length > limit as u64) {
        return Err(too_large());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if chunk.len() > limit - body.len() { return Err(too_large()); }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[derive(Debug, Error)]
pub enum SdkError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("api {status}: {body}")]
    Api { status: StatusCode, body: String },
    #[error("bad response: {0}")]
    Decode(String),
}

#[derive(Clone)]
pub struct MqClient {
    http: Client,
    base: String,
    token: String,
}

impl MqClient {
    pub fn new(base_url: impl Into<String>, bearer_token: impl Into<String>) -> Self {
        Self::try_new(base_url, bearer_token).expect("invalid MQ client configuration")
    }

    /// Validate a host-configured origin before attaching credentials. TLS and
    /// endpoint/account trust remain the host's responsibility.
    pub fn try_new(base_url: impl Into<String>, bearer_token: impl Into<String>) -> Result<Self, SdkError> {
        let url = reqwest::Url::parse(&base_url.into())
            .map_err(|_| SdkError::Decode("invalid MQ origin".into()))?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none()
            || !url.username().is_empty() || url.password().is_some()
            || url.query().is_some() || url.fragment().is_some() || url.path() != "/"
        {
            return Err(SdkError::Decode("MQ endpoint must be an HTTP origin without credentials, path, query or fragment".into()));
        }
        let token = bearer_token.into();
        if token.trim().is_empty() || token.trim() != token
            || reqwest::header::HeaderValue::from_str(&format!("Bearer {token}")).is_err()
        {
            return Err(SdkError::Decode("invalid MQ bearer credential".into()));
        }
        Ok(Self {
            http: Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(SdkError::Http)?,
            base: url.origin().ascii_serialization(),
            token,
        })
    }

    /// Dev helper: `kind:org:id` token form used by mq-server.
    pub fn with_dev_principal(
        base_url: impl Into<String>,
        kind: &str,
        org_id: &str,
        id: &str,
    ) -> Self {
        Self::new(base_url, format!("{kind}:{org_id}:{id}"))
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    async fn send_json<T: serde::de::DeserializeOwned>(
        &self,
        req: reqwest::RequestBuilder,
    ) -> Result<T, SdkError> {
        let res = req
            .header("authorization", format!("Bearer {}", self.token))
            .send()
            .await?;
        let status = res.status();
        let bytes = bounded_body(res).await?;
        if !status.is_success() {
            return Err(SdkError::Api {
                status,
                body: String::from_utf8_lossy(&bytes).into(),
            });
        }
        serde_json::from_slice(&bytes).map_err(|e| SdkError::Decode(e.to_string()))
    }

    async fn send_empty(&self, req: reqwest::RequestBuilder) -> Result<(), SdkError> {
        let res = req
            .header("authorization", format!("Bearer {}", self.token))
            .send()
            .await?;
        let status = res.status();
        if status == StatusCode::NO_CONTENT || status.is_success() {
            return Ok(());
        }
        let body = String::from_utf8_lossy(&bounded_body(res).await?).into_owned();
        Err(SdkError::Api { status, body })
    }

    pub async fn health(&self) -> Result<(), SdkError> {
        let res = self.http.get(self.url("/health")).send().await?;
        if res.status().is_success() {
            Ok(())
        } else {
            let status = res.status();
            Err(SdkError::Api {
                status,
                body: String::from_utf8_lossy(&bounded_body(res).await?).into_owned(),
            })
        }
    }

    pub async fn create_thread(&self, req: CreateThread) -> Result<Thread, SdkError> {
        self.send_json(self.http.post(self.url("/v1/threads")).json(&req))
            .await
    }

    /// Idempotent create — requires `idempotency_key` (SMR/Intern binding).
    pub async fn ensure_thread(&self, req: CreateThread) -> Result<Thread, SdkError> {
        self.send_json(self.http.post(self.url("/v1/threads/ensure")).json(&req))
            .await
    }

    pub async fn get_thread(&self, thread_id: ThreadId) -> Result<Thread, SdkError> {
        self.send_json(
            self.http
                .get(self.url(&format!("/v1/threads/{}", thread_id.0))),
        )
        .await
    }

    pub async fn list_threads(
        &self,
        scope: Option<&ScopeBinding>,
    ) -> Result<Vec<Thread>, SdkError> {
        let mut req = self.http.get(self.url("/v1/threads"));
        if let Some(s) = scope {
            let kind = match s.kind {
                ScopeKind::Org => "org",
                ScopeKind::Factory => "factory",
                ScopeKind::Effort => "effort",
                ScopeKind::Project => "project",
                ScopeKind::SyncSession => "sync_session",
                ScopeKind::AsyncRuntime => "async_runtime",
            };
            req = req.query(&[("scope_kind", kind), ("scope_id", s.id.as_str())]);
        }
        self.send_json(req).await
    }

    pub async fn add_participant(
        &self,
        thread_id: ThreadId,
        participant: Participant,
    ) -> Result<(), SdkError> {
        self.send_empty(
            self.http
                .post(self.url(&format!("/v1/threads/{}/participants", thread_id.0)))
                .json(&participant),
        )
        .await
    }

    /// Set a participant role, including revocation. Authorization is enforced
    /// by the server; uncertain responses are returned without automatic retry.
    pub async fn set_participant_role(
        &self,
        thread_id: ThreadId,
        kind: PrincipalKind,
        principal_id: &str,
        role: Role,
    ) -> Result<(), SdkError> {
        if principal_id.trim().is_empty() || matches!(principal_id, "." | "..") {
            return Err(SdkError::Decode("invalid participant identity".into()));
        }
        let kind = match kind {
            PrincipalKind::Human => "human",
            PrincipalKind::InternAsync => "intern_async",
            PrincipalKind::InternSync => "intern_sync",
            PrincipalKind::Actor => "actor",
            PrincipalKind::System => "system",
        };
        let mut url = reqwest::Url::parse(&self.url(&format!("/v1/threads/{}/participants", thread_id.0)))
            .map_err(|_| SdkError::Decode("invalid MQ URL".into()))?;
        url.path_segments_mut().map_err(|_| SdkError::Decode("invalid MQ URL".into()))?
            .push(kind).push(principal_id);
        self.send_empty(self.http.patch(url).json(&serde_json::json!({"role":role}))).await
    }

    pub async fn publish(
        &self,
        thread_id: ThreadId,
        req: PublishMessage,
    ) -> Result<Message, SdkError> {
        self.send_json(
            self.http
                .post(self.url(&format!("/v1/threads/{}/messages", thread_id.0)))
                .json(&req),
        )
        .await
    }

    pub async fn read_messages(
        &self,
        thread_id: ThreadId,
        after_seq: u64,
        limit: usize,
    ) -> Result<Vec<Message>, SdkError> {
        self.send_json(
            self.http
                .get(self.url(&format!("/v1/threads/{}/messages", thread_id.0)))
                .query(&[("after_seq", after_seq), ("limit", limit as u64)]),
        )
        .await
    }
}

pub fn thread_id(uuid: Uuid) -> ThreadId {
    ThreadId(uuid)
}
