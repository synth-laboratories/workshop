//! Workshop grant contract client (manderqueue
//! `docs/WORKSHOP_GRANT_CONTRACT.md` **contract version 2**, committed at
//! 02db5d4, sha256 9a442993…; v2 adds device sign-out via enrollment
//! revocation and the local-only loopback identity origin).
//!
//! The credential source is the [`GrantAuthority`] trait; [`HttpGrantAuthority`]
//! is a thin adapter over the backend endpoints in contract §3. Everything
//! returned is validated against the verified desktop identity before it is
//! persisted. Mutations are never retried automatically (§5): on transport
//! uncertainty the caller re-reads with a GET and decides.
use crate::cloud::storage::{CloudScopeIdentity, EnrollmentBinding, GrantLifecycle, GrantSnapshot, ParticipantRecord};
use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const CONTRACT_VERSION: u32 = 2;
pub const CONTRACT_SHA256: &str = "9a4429931ce762c4a41c98e88ba204120f0abcc396c70b0b8b8bf423df180563";
/// Credential lifetime bound from contract §3 (`ttl_seconds` on credential).
pub const MAX_CREDENTIAL_SECS: i64 = 300;
const MAX_BODY: usize = 1024 * 1024;

/// A bearer secret. Never printed, serialized into logs or persisted.
#[derive(Clone, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct SecretToken(String);
impl SecretToken {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Debug for SecretToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PrincipalDoc {
    pub kind: String,
    pub id: String,
    pub org_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct EnrollmentDoc {
    pub enrollment_id: String,
    pub org_id: String,
    pub owner: PrincipalDoc,
    pub device_id: String,
    pub session_id: String,
    #[serde(default)]
    pub label: Option<String>,
    pub principal: PrincipalDoc,
    pub incarnation: u64,
    /// Device sign-out (contract v2 §5.1). Once set the enrollment is dead.
    #[serde(default)]
    pub revoked_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct IdentityDoc {
    pub backend_origin: String,
    pub backend_id: String,
    pub profile_id: String,
    pub account_id: String,
    pub org_id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct EnrollResponse {
    pub enrollment: EnrollmentDoc,
    pub mq_endpoint: String,
    pub identity: IdentityDoc,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct GrantDoc {
    pub grant_id: String,
    pub org_id: String,
    pub thread_id: String,
    pub enrollment_id: String,
    pub principal: PrincipalDoc,
    pub operations: Vec<String>,
    pub history_after_seq: u64,
    pub expires_at: DateTime<Utc>,
    pub incarnation: u64,
    pub generation: u64,
    pub status: String,
    pub state: String,
    pub granted_by: PrincipalDoc,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CredentialDoc {
    pub mq_endpoint: String,
    pub token: SecretToken,
    pub token_type: String,
    pub expires_at: DateTime<Utc>,
    pub kid: String,
    pub grant: GrantDoc,
}

#[derive(Clone, Debug, Serialize)]
pub struct EnrollRequest {
    pub device_id: String,
    pub session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CreateGrantRequest {
    pub thread_id: String,
    pub enrollment_id: String,
    pub operations: Vec<String>,
    pub ttl_seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub history_after_seq: Option<u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CredentialRequest {
    #[serde(skip)]
    pub grant_id: String,
    pub enrollment_id: String,
    pub incarnation: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_seconds: Option<u64>,
}

/// Typed failure of a backend call, keyed by the contract §7 codes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AuthorityError {
    /// Definitive refusal carrying the contract code.
    Refused { status: u16, code: String },
    /// Server-side unavailability (502/503 or transport loss on a read).
    Unavailable { status: u16, code: String },
    /// A mutation may or may not have taken effect. Re-read, never retry.
    Uncertain(String),
    /// Malformed response or a contract violation detected locally.
    Invalid(String),
}

impl AuthorityError {
    pub fn code(&self) -> Option<&str> {
        match self {
            Self::Refused { code, .. } | Self::Unavailable { code, .. } => Some(code),
            _ => None,
        }
    }
    /// The Synth key or membership is gone: the host must sign out.
    pub fn identity_revoked(&self) -> bool {
        matches!(self, Self::Refused { status: 401, code } if code == "desktop_cloud_identity_revoked_or_unavailable")
    }
}

impl std::fmt::Display for AuthorityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused { status, code } => write!(f, "grant authority refused ({status} {code})"),
            Self::Unavailable { status, code } => write!(f, "grant authority unavailable ({status} {code})"),
            Self::Uncertain(detail) => write!(f, "grant authority outcome uncertain: {detail}"),
            Self::Invalid(detail) => write!(f, "grant authority response invalid: {detail}"),
        }
    }
}
impl std::error::Error for AuthorityError {}

/// Credential source. The concrete HTTP issuer is a thin adapter; fixtures
/// implement this directly or serve the same HTTP contract in-process.
pub trait GrantAuthority: Send + Sync {
    fn enroll(&self, request: EnrollRequest) -> BoxFuture<'_, Result<EnrollResponse, AuthorityError>>;
    fn create_grant(&self, request: CreateGrantRequest) -> BoxFuture<'_, Result<GrantDoc, AuthorityError>>;
    fn list_grants(&self, enrollment_id: String, thread_id: String) -> BoxFuture<'_, Result<Vec<GrantDoc>, AuthorityError>>;
    fn get_grant(&self, grant_id: String) -> BoxFuture<'_, Result<GrantDoc, AuthorityError>>;
    fn revoke_grant(&self, grant_id: String) -> BoxFuture<'_, Result<GrantDoc, AuthorityError>>;
    fn credential(&self, request: CredentialRequest) -> BoxFuture<'_, Result<CredentialDoc, AuthorityError>>;
    /// Device sign-out (v2 §5.1): idempotent, permanent for this enrollment.
    fn revoke_enrollment(&self, enrollment_id: String) -> BoxFuture<'_, Result<EnrollmentDoc, AuthorityError>>;
    fn get_enrollment(&self, enrollment_id: String) -> BoxFuture<'_, Result<EnrollmentDoc, AuthorityError>>;
}

/// Which MQ/backend origins are acceptable. Production requires https;
/// loopback http is accepted only when the verified backend is itself a
/// local slot (see [`local_loopback_origin`]) or in in-process fixtures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EndpointPolicy {
    pub allow_loopback_http: bool,
}
impl EndpointPolicy {
    pub const PRODUCTION: Self = Self { allow_loopback_http: false };
    pub const LOCAL_SLOT: Self = Self { allow_loopback_http: true };
}

/// The backend's local-only origin form (contract v2 §9, backend
/// `services/desktop_cloud_identity._canonical_origin`): `http` with host
/// exactly `127.0.0.1`, `localhost` or `[::1]`, an explicit port and no
/// path, query or userinfo. The backend emits it only when
/// `APP_ENVIRONMENT` is exactly `local`; the identity document has no other
/// environment field, so this origin form is the local signal. Returns the
/// canonical origin, or `None` for anything else.
pub fn local_loopback_origin(value: &str) -> Option<String> {
    let url = reqwest::Url::parse(value).ok()?;
    let host = url.host_str()?;
    let port = url.port()?;
    if url.scheme() != "http"
        || !matches!(host, "127.0.0.1" | "localhost" | "[::1]")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return None;
    }
    let canonical = format!("http://{host}:{port}");
    (canonical == value.trim_end_matches('/')).then_some(canonical)
}

pub fn validate_origin(value: &str, policy: EndpointPolicy) -> Result<String> {
    let url = reqwest::Url::parse(value).context("invalid origin")?;
    let scheme_ok = url.scheme() == "https" || (policy.allow_loopback_http && local_loopback_origin(value).is_some());
    if !scheme_ok
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        bail!("endpoint must be an https origin without path, query, fragment or userinfo");
    }
    let origin = url.origin().ascii_serialization();
    if origin != value.trim_end_matches('/') {
        bail!("endpoint is not a canonical origin");
    }
    Ok(origin)
}

fn canonical_uuid(value: &str) -> Result<()> {
    let id = uuid::Uuid::parse_str(value).context("identifier must be a UUID")?;
    if id.is_nil() || id.to_string() != value {
        bail!("identifier must be a canonical UUID");
    }
    Ok(())
}

impl EnrollResponse {
    /// Bind the enrollment to exactly the verified identity (contract §3).
    pub fn validate(&self, identity: &CloudScopeIdentity, device_id: &str, session_id: &str, policy: EndpointPolicy) -> Result<EnrollmentBinding> {
        let expected_origin = identity.backend_origin.trim_end_matches('/');
        if self.identity.backend_origin.trim_end_matches('/') != expected_origin
            || self.identity.backend_id != identity.backend_id
            || self.identity.profile_id != identity.profile_id
            || self.identity.account_id != identity.account_id
            || self.identity.org_id != identity.org_id
        {
            bail!("enrollment identity differs from the verified account");
        }
        let enrollment = &self.enrollment;
        canonical_uuid(&enrollment.enrollment_id)?;
        if enrollment.org_id != identity.org_id
            || enrollment.owner != (PrincipalDoc { kind: "human".into(), id: identity.account_id.clone(), org_id: identity.org_id.clone() })
            || enrollment.principal != (PrincipalDoc { kind: "actor".into(), id: format!("enrollment:{}", enrollment.enrollment_id), org_id: identity.org_id.clone() })
            || enrollment.device_id != device_id
            || enrollment.session_id != session_id
            || enrollment.incarnation == 0
        {
            bail!("enrollment is not the server-derived principal for this device/session");
        }
        if enrollment.revoked_at.is_some() {
            bail!("enrollment was signed out; enroll a new session");
        }
        Ok(EnrollmentBinding {
            enrollment_id: enrollment.enrollment_id.clone(),
            device_id: enrollment.device_id.clone(),
            incarnation: enrollment.incarnation,
            principal_id: enrollment.principal.id.clone(),
            org_id: enrollment.org_id.clone(),
            mq_endpoint: validate_origin(&self.mq_endpoint, policy)?,
        })
    }
}

impl GrantDoc {
    pub fn validate(&self, participant: &ParticipantRecord) -> Result<GrantSnapshot> {
        canonical_uuid(&self.grant_id)?;
        let principal = PrincipalDoc { kind: "actor".into(), id: participant.principal_id.clone(), org_id: participant.org_id.clone() };
        if self.org_id != participant.org_id
            || self.thread_id != participant.thread_id
            || self.enrollment_id != participant.enrollment_id
            || self.principal != principal
        {
            bail!("grant does not belong to this participant");
        }
        let mut operations = self.operations.clone();
        operations.sort();
        operations.dedup();
        if operations.is_empty() || operations.len() != self.operations.len() || operations.iter().any(|op| !matches!(op.as_str(), "read" | "publish")) {
            bail!("grant operations are invalid");
        }
        let lifecycle = match (self.status.as_str(), self.state.as_str()) {
            ("active", "active") => GrantLifecycle::Active,
            ("revoked", "revoked") => GrantLifecycle::Revoked,
            ("active", "expired") => GrantLifecycle::Expired,
            _ => bail!("grant status/state pair is not recognized"),
        };
        Ok(GrantSnapshot {
            grant_id: self.grant_id.clone(),
            thread_id: self.thread_id.clone(),
            enrollment_id: self.enrollment_id.clone(),
            principal_id: self.principal.id.clone(),
            org_id: self.org_id.clone(),
            operations,
            history_after_seq: self.history_after_seq,
            expires_at_ms: self.expires_at.timestamp_millis(),
            incarnation: self.incarnation,
            generation: self.generation,
            lifecycle,
        })
    }
}

impl CredentialDoc {
    /// The endpoint must equal the enrolled endpoint: never fall back to
    /// another MQ origin (contract §7 502/503 row). Lifetime ≤ 300 s.
    pub fn validate(&self, participant: &ParticipantRecord, now: DateTime<Utc>, policy: EndpointPolicy) -> Result<GrantSnapshot> {
        if self.token_type != "Bearer" || self.token.expose().trim().is_empty() || self.kid.trim().is_empty() {
            bail!("credential is malformed");
        }
        if validate_origin(&self.mq_endpoint, policy)? != participant.mq_endpoint {
            bail!("credential endpoint differs from the enrolled MQ endpoint");
        }
        if self.expires_at <= now || self.expires_at > now + chrono::Duration::seconds(MAX_CREDENTIAL_SECS + 5) {
            bail!("credential lifetime is outside the contract bound");
        }
        let snapshot = self.grant.validate(participant)?;
        if snapshot.lifecycle != GrantLifecycle::Active || snapshot.incarnation != participant.incarnation {
            bail!("credential grant is not active for this incarnation");
        }
        Ok(snapshot)
    }
}

/// Thin HTTP adapter over the backend endpoints (contract §3).
pub struct HttpGrantAuthority {
    http: reqwest::Client,
    origin: String,
    api_key: SecretToken,
}

impl HttpGrantAuthority {
    pub fn try_new(backend_origin: &str, api_key: SecretToken, policy: EndpointPolicy) -> Result<Self> {
        let origin = validate_origin(backend_origin, policy)?;
        if api_key.expose().trim().is_empty() || reqwest::header::HeaderValue::from_str(&format!("Bearer {}", api_key.expose())).is_err() {
            bail!("invalid Synth API credential");
        }
        Ok(Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            origin,
            api_key,
        })
    }

    async fn call<T: serde::de::DeserializeOwned>(&self, method: reqwest::Method, path: &str, body: Option<Value>, mutation: bool) -> Result<T, AuthorityError> {
        let mut request = self
            .http
            .request(method, format!("{}{path}", self.origin))
            .header("authorization", format!("Bearer {}", self.api_key.expose()));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) if mutation => return Err(AuthorityError::Uncertain(format!("transport: {}", error.without_url()))),
            Err(error) => return Err(AuthorityError::Unavailable { status: 0, code: format!("transport: {}", error.without_url()) }),
        };
        let status = response.status().as_u16();
        let bytes = bounded(response).await.map_err(|error| if mutation { AuthorityError::Uncertain(error) } else { AuthorityError::Unavailable { status, code: error } })?;
        if (200..300).contains(&status) {
            return serde_json::from_slice(&bytes).map_err(|error| AuthorityError::Invalid(error.to_string()));
        }
        let code = serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|value| value.pointer("/detail/code").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_else(|| "unknown".into());
        if matches!(status, 502 | 503) {
            return Err(AuthorityError::Unavailable { status, code });
        }
        if status >= 500 && mutation {
            return Err(AuthorityError::Uncertain(format!("server error {status}")));
        }
        Err(AuthorityError::Refused { status, code })
    }
}

async fn bounded(mut response: reqwest::Response) -> Result<Vec<u8>, String> {
    if response.content_length().is_some_and(|length| length > MAX_BODY as u64) {
        return Err("response exceeds limit".into());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| error.without_url().to_string())? {
        if chunk.len() > MAX_BODY - body.len() {
            return Err("response exceeds limit".into());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[derive(Deserialize)]
struct GrantEnvelope {
    grant: GrantDoc,
}
#[derive(Deserialize)]
struct GrantList {
    grants: Vec<GrantDoc>,
}
#[derive(Deserialize)]
struct EnrollmentEnvelope {
    enrollment: EnrollmentDoc,
}

fn segment(id: &str) -> Result<&str, AuthorityError> {
    canonical_uuid(id).map_err(|error| AuthorityError::Invalid(error.to_string()))?;
    Ok(id)
}

impl GrantAuthority for HttpGrantAuthority {
    fn enroll(&self, request: EnrollRequest) -> BoxFuture<'_, Result<EnrollResponse, AuthorityError>> {
        Box::pin(async move { self.call(reqwest::Method::POST, "/api/v1/mq/enrollments", Some(json!(request)), true).await })
    }
    fn create_grant(&self, request: CreateGrantRequest) -> BoxFuture<'_, Result<GrantDoc, AuthorityError>> {
        Box::pin(async move { Ok(self.call::<GrantEnvelope>(reqwest::Method::POST, "/api/v1/mq/grants", Some(json!(request)), true).await?.grant) })
    }
    fn list_grants(&self, enrollment_id: String, thread_id: String) -> BoxFuture<'_, Result<Vec<GrantDoc>, AuthorityError>> {
        Box::pin(async move {
            let path = format!("/api/v1/mq/grants?enrollment_id={}&thread_id={}", segment(&enrollment_id)?, segment(&thread_id)?);
            Ok(self.call::<GrantList>(reqwest::Method::GET, &path, None, false).await?.grants)
        })
    }
    fn get_grant(&self, grant_id: String) -> BoxFuture<'_, Result<GrantDoc, AuthorityError>> {
        Box::pin(async move {
            let path = format!("/api/v1/mq/grants/{}", segment(&grant_id)?);
            Ok(self.call::<GrantEnvelope>(reqwest::Method::GET, &path, None, false).await?.grant)
        })
    }
    fn revoke_grant(&self, grant_id: String) -> BoxFuture<'_, Result<GrantDoc, AuthorityError>> {
        Box::pin(async move {
            let path = format!("/api/v1/mq/grants/{}/revoke", segment(&grant_id)?);
            Ok(self.call::<GrantEnvelope>(reqwest::Method::POST, &path, Some(json!({})), true).await?.grant)
        })
    }
    fn credential(&self, request: CredentialRequest) -> BoxFuture<'_, Result<CredentialDoc, AuthorityError>> {
        Box::pin(async move {
            let path = format!("/api/v1/mq/grants/{}/credential", segment(&request.grant_id)?);
            // Issuing a credential mutates nothing durable; a lost response is
            // safe to re-request, but the host still never loops on it.
            self.call(reqwest::Method::POST, &path, Some(json!(request)), false).await
        })
    }
    fn revoke_enrollment(&self, enrollment_id: String) -> BoxFuture<'_, Result<EnrollmentDoc, AuthorityError>> {
        Box::pin(async move {
            let path = format!("/api/v1/mq/enrollments/{}/revoke", segment(&enrollment_id)?);
            Ok(self.call::<EnrollmentEnvelope>(reqwest::Method::POST, &path, Some(json!({})), true).await?.enrollment)
        })
    }
    fn get_enrollment(&self, enrollment_id: String) -> BoxFuture<'_, Result<EnrollmentDoc, AuthorityError>> {
        Box::pin(async move {
            let path = format!("/api/v1/mq/enrollments/{}", segment(&enrollment_id)?);
            Ok(self.call::<EnrollmentEnvelope>(reqwest::Method::GET, &path, None, false).await?.enrollment)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secrets_never_render_and_origins_are_strict() {
        let token = SecretToken::new("synth-secret-canary");
        assert_eq!(format!("{token:?}"), "<redacted>");
        let doc: CredentialDoc = serde_json::from_value(json!({
            "mq_endpoint":"https://mq.example.test","token":"synth-secret-canary","token_type":"Bearer",
            "expires_at":"2026-09-12T00:05:00Z","kid":"k1","grant":{
                "grant_id":"00000000-0000-4000-8000-000000000001","org_id":"o","thread_id":"t","enrollment_id":"e",
                "principal":{"kind":"actor","id":"enrollment:e","org_id":"o"},"operations":["read"],"history_after_seq":0,
                "expires_at":"2026-09-13T00:00:00Z","incarnation":1,"generation":0,"status":"active","state":"active",
                "granted_by":{"kind":"human","id":"h","org_id":"o"},"created_at":"x","updated_at":"x"}
        })).unwrap();
        assert!(!format!("{doc:?}").contains("canary"));
        assert!(validate_origin("https://mq.example.test", EndpointPolicy::PRODUCTION).is_ok());
        for bad in ["http://mq.example.test", "https://u:p@mq.example.test", "https://mq.example.test/v1", "https://mq.example.test/?a=b", "http://127.0.0.1:9"] {
            assert!(validate_origin(bad, EndpointPolicy::PRODUCTION).is_err(), "{bad}");
        }
        assert!(validate_origin("http://127.0.0.1:9", EndpointPolicy { allow_loopback_http: true }).is_ok());
        assert!(validate_origin("http://10.0.0.1:9", EndpointPolicy { allow_loopback_http: true }).is_err());
    }

    /// Mirrors the backend's `_canonical_origin(local=True)` cases.
    #[test]
    fn local_loopback_origin_matches_the_backend_local_rule_exactly() {
        for ok in ["http://127.0.0.1:8000", "http://localhost:8000", "http://[::1]:8000"] {
            assert_eq!(local_loopback_origin(ok).as_deref(), Some(ok), "{ok}");
        }
        for refused in [
            "http://127.0.0.2:8000",   // other loopback addresses
            "http://127.0.0.1",        // implicit port
            "http://localhost",        // implicit port
            "http://127.0.0.1:8000/x", // path
            "http://127.0.0.1:8000/?q=1",
            "http://u@127.0.0.1:8000",
            "http://example.test:8000", // non-loopback http
            "https://127.0.0.1:8000",   // not the http local form
            "http://LOCALHOST:8000",    // noncanonical spelling
        ] {
            assert_eq!(local_loopback_origin(refused), None, "{refused}");
        }
        assert!(validate_origin("http://127.0.0.1", EndpointPolicy::LOCAL_SLOT).is_err());
        assert!(validate_origin("http://127.0.0.1:8000", EndpointPolicy::PRODUCTION).is_err());
    }
}
