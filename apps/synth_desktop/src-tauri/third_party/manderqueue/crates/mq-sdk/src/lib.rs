//! Typed HTTP client for Manderqueue.

mod catch_up;
pub use catch_up::{CatchUpOutcome, CatchUpSupervisor};

use mq_core::{
    CreateGrant, CreateThread, EnrollDevice, Enrollment, Grant, GrantFilter, GrantIssuance,
    GrantIssuanceRequest, HistoryPage, Message, Participant, PrincipalKind, PublishMessage,
    RenewGrant, Role, ScopeBinding, ScopeKind, Thread, ThreadId,
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

impl SdkError {
    /// Stable server error code (`{"error": code}`), e.g. `grant_revoked`.
    /// See docs/WORKSHOP_GRANT_CONTRACT.md §7.
    pub fn api_code(&self) -> Option<String> {
        match self {
            SdkError::Api { body, .. } => serde_json::from_str::<serde_json::Value>(body)
                .ok()?
                .get("error")?
                .as_str()
                .map(str::to_owned),
            _ => None,
        }
    }

    pub fn status(&self) -> Option<StatusCode> {
        match self {
            SdkError::Api { status, .. } => Some(*status),
            _ => None,
        }
    }
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

    /// Cursor page with explicit history skips; use for catch-up under a grant.
    /// See docs/WORKSHOP_GRANT_CONTRACT.md §8.
    pub async fn read_history(
        &self,
        thread_id: ThreadId,
        after_seq: u64,
        limit: usize,
    ) -> Result<HistoryPage, SdkError> {
        self.send_json(
            self.http
                .get(self.url(&format!("/v1/threads/{}/history", thread_id.0)))
                .query(&[("after_seq", after_seq), ("limit", limit as u64)]),
        )
        .await
    }

    // ---- Enrollment and grant administration ------------------------------
    //
    // These require an unrestricted owner credential (the backend acts as the
    // Synth user). Every mutation is sent exactly once: on a transport error
    // the outcome is unknown, so re-read (`get_grant`/`get_enrollment`) before
    // deciding. Never retry automatically.

    /// Enroll a device session; each call advances the incarnation.
    pub async fn enroll(&self, req: &EnrollDevice) -> Result<Enrollment, SdkError> {
        self.send_json(self.http.post(self.url("/v1/enrollments")).json(req)).await
    }

    pub async fn list_enrollments(&self) -> Result<Vec<Enrollment>, SdkError> {
        self.send_json(self.http.get(self.url("/v1/enrollments"))).await
    }

    pub async fn get_enrollment(&self, enrollment_id: Uuid) -> Result<Enrollment, SdkError> {
        self.send_json(self.http.get(self.url(&format!("/v1/enrollments/{enrollment_id}")))).await
    }

    /// Device sign-out: every grant and incarnation of the enrollment is
    /// refused afterwards and queued deliveries are dead-lettered. Idempotent.
    pub async fn revoke_enrollment(&self, enrollment_id: Uuid) -> Result<Enrollment, SdkError> {
        self.send_json(
            self.http
                .post(self.url(&format!("/v1/enrollments/{enrollment_id}/revoke")))
                .json(&serde_json::json!({})),
        )
        .await
    }

    pub async fn create_grant(&self, req: &CreateGrant) -> Result<Grant, SdkError> {
        self.send_json(self.http.post(self.url("/v1/grants")).json(req)).await
    }

    pub async fn list_grants(&self, filter: &GrantFilter) -> Result<Vec<Grant>, SdkError> {
        let mut query = Vec::new();
        if let Some(enrollment_id) = filter.enrollment_id {
            query.push(("enrollment_id", enrollment_id.to_string()));
        }
        if let Some(thread_id) = filter.thread_id {
            query.push(("thread_id", thread_id.0.to_string()));
        }
        self.send_json(self.http.get(self.url("/v1/grants")).query(&query)).await
    }

    pub async fn get_grant(&self, grant_id: Uuid) -> Result<Grant, SdkError> {
        self.send_json(self.http.get(self.url(&format!("/v1/grants/{grant_id}")))).await
    }

    pub async fn revoke_grant(&self, grant_id: Uuid) -> Result<Grant, SdkError> {
        self.send_json(self.http.post(self.url(&format!("/v1/grants/{grant_id}/revoke"))).json(&serde_json::json!({}))).await
    }

    pub async fn restore_grant(&self, grant_id: Uuid) -> Result<Grant, SdkError> {
        self.send_json(self.http.post(self.url(&format!("/v1/grants/{grant_id}/restore"))).json(&serde_json::json!({}))).await
    }

    /// Extend the grant's own expiry. Refused after revoke.
    pub async fn renew_grant(&self, grant_id: Uuid, ttl_seconds: i64) -> Result<Grant, SdkError> {
        self.send_json(
            self.http
                .post(self.url(&format!("/v1/grants/{grant_id}/renew")))
                .json(&RenewGrant { ttl_seconds }),
        )
        .await
    }

    /// Live issuance authority (backend issuer only). Not itself a credential.
    pub async fn grant_issuance(
        &self,
        grant_id: Uuid,
        req: &GrantIssuanceRequest,
    ) -> Result<GrantIssuance, SdkError> {
        self.send_json(self.http.post(self.url(&format!("/v1/grants/{grant_id}/issuance"))).json(req)).await
    }
}

pub fn thread_id(uuid: Uuid) -> ThreadId {
    ThreadId(uuid)
}
