//! Auth: `MQ_AUTH=dev` spoof bearer, or `MQ_AUTH=jwt` signed credentials.
//!
//! MQ is verification-only for backend-issued credentials: Ed25519 (`EdDSA`)
//! with a required `kid` looked up in a configured public JWKS. Legacy HS256
//! verification stays available only while `MQ_JWT_SECRET` is configured and
//! is never a fallback for grant credentials. See
//! docs/WORKSHOP_GRANT_CONTRACT.md §9.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};

use sha2::{Digest, Sha256};

use jsonwebtoken::jwk::{AlgorithmParameters, EllipticCurve, JwkSet, KeyAlgorithm};
use jsonwebtoken::{decode, decode_header, Algorithm, DecodingKey, Validation};
use mq_core::grants::{is_enrollment_principal, ENROLLMENT_PRINCIPAL_PREFIX};
use mq_core::{GrantFence, GrantOperation, HistoryAuthority, Principal, PrincipalKind, ThreadId};
use serde::Deserialize;

/// Issuer of legacy HS256 credentials.
pub const LEGACY_ISSUER: &str = "manderqueue";
/// Issuer of asymmetric (EdDSA) backend credentials.
pub const SIGNED_ISSUER: &str = "synth-backend";
pub const AUDIENCE: &str = "manderqueue";
/// Upper bound on a grant credential's lifetime (`exp - iat`).
pub const GRANT_TOKEN_MAX_LIFETIME_SECONDS: i64 = 300;

#[derive(Debug, Clone)]
pub enum AuthMode {
    /// `Bearer {kind}:{org_id}:{id}` — local/dev only.
    Dev,
    /// Legacy HS256 only (`MQ_JWT_SECRET`, no JWKS configured).
    Jwt { secret: String },
    /// EdDSA/kid verification from a public JWKS, plus legacy HS256 only if
    /// `MQ_JWT_SECRET` is also configured.
    Keyset(Arc<Verifier>),
}

/// Public-key verifier. Holds no signing material.
pub struct Verifier {
    keys: RwLock<HashMap<String, DecodingKey>>,
    legacy_secret: Option<String>,
    /// Set when keys come from `MQ_JWT_JWKS_FILE`; enables content reloads.
    source: Option<JwksFile>,
}

struct JwksFile {
    path: PathBuf,
    /// SHA-256 of the last successfully applied file content.
    digest: Mutex<[u8; 32]>,
}

impl std::fmt::Debug for Verifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Verifier")
            .field("kids", &self.kids())
            .field("legacy_hs256", &self.legacy_secret.is_some())
            .field("jwks_file", &self.source.as_ref().map(|s| s.path.display().to_string()))
            .finish()
    }
}

fn check_legacy_secret(secret: &str) -> Result<(), String> {
    if secret.len() < 32 {
        return Err("MQ_JWT_SECRET requires at least 32 bytes".into());
    }
    Ok(())
}

/// Parse a JWK set, accepting only Ed25519 OKP keys with unique, nonempty kids.
fn parse_jwks(json: &str) -> Result<HashMap<String, DecodingKey>, String> {
    let set: JwkSet = serde_json::from_str(json).map_err(|_| "MQ JWKS is not a valid JWK set".to_string())?;
    if set.keys.is_empty() {
        return Err("MQ JWKS has no keys".into());
    }
    let mut keys = HashMap::new();
    for jwk in &set.keys {
        let kid = jwk
            .common
            .key_id
            .clone()
            .filter(|kid| !kid.trim().is_empty())
            .ok_or("every MQ JWK requires a kid")?;
        match &jwk.algorithm {
            AlgorithmParameters::OctetKeyPair(params) if matches!(params.curve, EllipticCurve::Ed25519) => {}
            _ => return Err(format!("MQ JWK {kid} is not an OKP Ed25519 key")),
        }
        if jwk.common.key_algorithm.is_some_and(|alg| !matches!(alg, KeyAlgorithm::EdDSA)) {
            return Err(format!("MQ JWK {kid} must use alg EdDSA"));
        }
        let key = DecodingKey::from_jwk(jwk).map_err(|_| format!("MQ JWK {kid} is invalid"))?;
        if keys.insert(kid.clone(), key).is_some() {
            return Err(format!("MQ JWKS has duplicate kid {kid}"));
        }
    }
    Ok(keys)
}

impl Verifier {
    pub fn new(jwks_json: &str, legacy_secret: Option<String>) -> Result<Self, String> {
        if let Some(secret) = legacy_secret.as_deref() {
            check_legacy_secret(secret)?;
        }
        Ok(Self {
            keys: RwLock::new(parse_jwks(jwks_json)?),
            legacy_secret,
            source: None,
        })
    }

    /// Load keys from a JWKS file and remember it for [`Self::reload_if_changed`].
    pub fn from_file(path: impl Into<PathBuf>, legacy_secret: Option<String>) -> Result<Self, String> {
        let path = path.into();
        let bytes = std::fs::read(&path).map_err(|_| "MQ_JWT_JWKS_FILE is unreadable".to_string())?;
        let text = std::str::from_utf8(&bytes).map_err(|_| "MQ_JWT_JWKS_FILE is not UTF-8".to_string())?;
        let mut verifier = Self::new(text, legacy_secret)?;
        verifier.source = Some(JwksFile { path, digest: Mutex::new(Sha256::digest(&bytes).into()) });
        Ok(verifier)
    }

    pub fn has_file_source(&self) -> bool {
        self.source.is_some()
    }

    /// Re-read the JWKS file and apply it if its content changed (rotation
    /// without restart). A missing, unreadable or invalid file keeps the
    /// current keys and returns an error; it never empties the keyset.
    pub fn reload_if_changed(&self) -> Result<bool, String> {
        let Some(source) = &self.source else { return Ok(false) };
        let bytes = std::fs::read(&source.path).map_err(|_| "MQ_JWT_JWKS_FILE is unreadable".to_string())?;
        let digest: [u8; 32] = Sha256::digest(&bytes).into();
        let mut applied = source.digest.lock().map_err(|_| "jwks reload lock poisoned".to_string())?;
        if *applied == digest {
            return Ok(false);
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| "MQ_JWT_JWKS_FILE is not UTF-8".to_string())?;
        self.replace_keys(text)?;
        *applied = digest;
        Ok(true)
    }

    /// Atomically replace the keyset (rotation). Kids absent from the new set
    /// stop verifying immediately; a parse failure keeps the previous set.
    pub fn replace_keys(&self, jwks_json: &str) -> Result<(), String> {
        let keys = parse_jwks(jwks_json)?;
        *self.keys.write().map_err(|_| "keyset lock poisoned".to_string())? = keys;
        Ok(())
    }

    pub fn kids(&self) -> Vec<String> {
        let mut kids: Vec<_> = self
            .keys
            .read()
            .map(|keys| keys.keys().cloned().collect())
            .unwrap_or_default();
        kids.sort();
        kids
    }

    pub fn legacy_hs256_enabled(&self) -> bool {
        self.legacy_secret.is_some()
    }
}

fn nonempty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.trim().is_empty())
}

impl AuthMode {
    pub fn from_env() -> Result<Self, String> {
        let mode = std::env::var("MQ_AUTH")
            .unwrap_or_else(|_| "jwt".into())
            .to_lowercase();
        match mode.as_str() {
            "dev" if std::env::var("MQ_PROFILE").as_deref() == Ok("local") => Ok(Self::Dev),
            "dev" => Err("MQ_AUTH=dev requires MQ_PROFILE=local".into()),
            "jwt" => {
                let legacy = nonempty_env("MQ_JWT_SECRET");
                if let Some(secret) = legacy.as_deref() {
                    check_legacy_secret(secret)?;
                }
                match (nonempty_env("MQ_JWT_JWKS"), nonempty_env("MQ_JWT_JWKS_FILE"), legacy) {
                    (Some(_), Some(_), _) => Err("set only one of MQ_JWT_JWKS and MQ_JWT_JWKS_FILE".into()),
                    (Some(inline), None, legacy) => Ok(Self::Keyset(Arc::new(Verifier::new(&inline, legacy)?))),
                    (None, Some(path), legacy) => Ok(Self::Keyset(Arc::new(Verifier::from_file(path, legacy)?))),
                    (None, None, Some(secret)) => Ok(Self::Jwt { secret }),
                    (None, None, None) => Err(
                        "MQ_AUTH=jwt requires MQ_JWT_JWKS (or MQ_JWT_JWKS_FILE) or MQ_JWT_SECRET".into(),
                    ),
                }
            }
            other => Err(format!("unknown MQ_AUTH={other} (use dev|jwt)")),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            AuthMode::Dev => "dev",
            AuthMode::Jwt { .. } => "jwt-hs256-legacy",
            AuthMode::Keyset(verifier) if verifier.legacy_hs256_enabled() => "jwt-eddsa+hs256-legacy",
            AuthMode::Keyset(_) => "jwt-eddsa",
        }
    }
}

#[derive(Debug, Deserialize)]
struct JwtClaims {
    /// Optional attenuation; it never replaces persisted thread membership.
    #[serde(default)]
    thread_scope: Option<ThreadScope>,
    /// Asymmetric grant credential; checked against live grant storage.
    #[serde(default)]
    grant: Option<GrantClaim>,
    /// Optional standard sub; prefer explicit principal fields.
    #[serde(default)]
    sub: Option<String>,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    org_id: Option<String>,
    /// Nested principal object (preferred).
    #[serde(default)]
    principal: Option<JwtPrincipal>,
    #[serde(default)]
    aud: Option<serde_json::Value>,
    #[serde(default)]
    iss: Option<String>,
    #[serde(default)]
    iat: Option<i64>,
    exp: i64,
    jti: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThreadScope {
    thread_id: uuid::Uuid,
    grant_generation: u64,
    operations: Vec<ThreadOperation>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GrantClaim {
    grant_id: uuid::Uuid,
    thread_id: uuid::Uuid,
    enrollment_id: uuid::Uuid,
    operations: Vec<ThreadOperation>,
    generation: u64,
    incarnation: u64,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThreadOperation {
    Read,
    Publish,
}

impl From<ThreadOperation> for GrantOperation {
    fn from(value: ThreadOperation) -> Self {
        match value {
            ThreadOperation::Read => GrantOperation::Read,
            ThreadOperation::Publish => GrantOperation::Publish,
        }
    }
}

#[derive(Debug, Deserialize)]
struct JwtPrincipal {
    kind: String,
    id: String,
    org_id: String,
}

fn parse_kind(kind: &str) -> Result<PrincipalKind, &'static str> {
    match kind {
        "human" => Ok(PrincipalKind::Human),
        "intern_async" => Ok(PrincipalKind::InternAsync),
        "intern_sync" => Ok(PrincipalKind::InternSync),
        "actor" => Ok(PrincipalKind::Actor),
        "system" => Ok(PrincipalKind::System),
        _ => Err("unknown_principal_kind"),
    }
}

fn principal_from_dev_token(token: &str) -> Result<Principal, &'static str> {
    let mut parts = token.splitn(3, ':');
    let kind = parts.next().ok_or("bad_token")?;
    let org_id = parts.next().ok_or("bad_token")?;
    let id = parts.next().ok_or("bad_token")?;
    if org_id.is_empty() || id.is_empty() {
        return Err("bad_token");
    }
    let principal = Principal {
        kind: parse_kind(kind)?,
        id: id.into(),
        org_id: org_id.into(),
    };
    // Even local spoofing cannot impersonate grant-governed principals.
    if is_enrollment_principal(&principal) {
        return Err("grant_token_required");
    }
    Ok(principal)
}

fn valid_operations(operations: &[ThreadOperation]) -> bool {
    !operations.is_empty()
        && operations.len() <= 2
        && !(operations.len() == 2 && operations[0] == operations[1])
}

/// Signature, algorithm, issuer, audience, expiry and token identity.
fn decode_claims(
    token: &str,
    key: &DecodingKey,
    algorithm: Algorithm,
    issuer: &str,
) -> Result<JwtClaims, &'static str> {
    let mut validation = Validation::new(algorithm);
    validation.set_audience(&[AUDIENCE]);
    validation.set_issuer(&[issuer]);
    validation.leeway = 0;
    validation.set_required_spec_claims(&["exp", "iss", "aud"]);
    let claims = decode::<JwtClaims>(token, key, &validation)
        .map_err(|_| "invalid_jwt")?
        .claims;
    if claims.jti.trim().is_empty() || claims.exp <= chrono::Utc::now().timestamp() {
        return Err("invalid_jwt");
    }
    if claims.iss.as_deref() != Some(issuer) {
        return Err("bad_issuer");
    }
    let audience_ok = match &claims.aud {
        Some(serde_json::Value::String(s)) => s == AUDIENCE,
        Some(serde_json::Value::Array(arr)) => arr.iter().any(|v| v.as_str() == Some(AUDIENCE)),
        _ => false,
    };
    if !audience_ok {
        return Err("bad_audience");
    }
    Ok(claims)
}

fn decode_legacy(token: &str, secret: &str) -> Result<JwtClaims, &'static str> {
    decode_claims(token, &DecodingKey::from_secret(secret.as_bytes()), Algorithm::HS256, LEGACY_ISSUER)
}

/// Returns the verified claims and whether they were asymmetrically signed.
fn verify(verifier: &Verifier, token: &str) -> Result<(JwtClaims, bool), &'static str> {
    let header = decode_header(token).map_err(|_| "invalid_jwt")?;
    match header.alg {
        Algorithm::EdDSA => {
            let kid = header.kid.ok_or("missing_kid")?;
            let keys = verifier.keys.read().map_err(|_| "keyset_unavailable")?;
            let key = keys.get(&kid).ok_or("unknown_kid")?;
            decode_claims(token, key, Algorithm::EdDSA, SIGNED_ISSUER).map(|claims| (claims, true))
        }
        Algorithm::HS256 => match verifier.legacy_secret.as_deref() {
            Some(secret) => decode_legacy(token, secret).map(|claims| (claims, false)),
            None => Err("legacy_hs256_disabled"),
        },
        _ => Err("unsupported_algorithm"),
    }
}

fn claims_principal(claims: &JwtClaims) -> Result<Principal, &'static str> {
    if let Some(p) = &claims.principal {
        if p.id.trim().is_empty() || p.org_id.trim().is_empty() {
            return Err("missing_principal");
        }
        return Ok(Principal {
            kind: parse_kind(&p.kind)?,
            id: p.id.clone(),
            org_id: p.org_id.clone(),
        });
    }
    let kind = claims.kind.as_deref().ok_or("missing_principal")?;
    let id = claims.id.clone().or(claims.sub.clone()).ok_or("missing_principal")?;
    let org_id = claims.org_id.clone().ok_or("missing_principal")?;
    if id.trim().is_empty() || org_id.trim().is_empty() {
        return Err("missing_principal");
    }
    Ok(Principal {
        kind: parse_kind(kind)?,
        id,
        org_id,
    })
}

/// Apply restriction claims. Principal-only access (`access == None`) refuses
/// every restricted credential so restrictions cannot be discarded.
fn authorize_claims(
    claims: JwtClaims,
    asymmetric: bool,
    access: Option<(uuid::Uuid, ThreadOperation)>,
) -> Result<(Principal, HistoryAuthority), &'static str> {
    let principal = claims_principal(&claims)?;
    if claims.grant.is_some() && claims.thread_scope.is_some() {
        return Err("conflicting_restrictions");
    }
    if let Some(grant) = claims.grant {
        if !asymmetric {
            return Err("grant_requires_asymmetric_signature");
        }
        if !valid_operations(&grant.operations) {
            return Err("invalid_grant_claim");
        }
        let iat = claims.iat.ok_or("grant_requires_iat")?;
        let now = chrono::Utc::now().timestamp();
        if claims.exp - iat > GRANT_TOKEN_MAX_LIFETIME_SECONDS || iat > now + 60 {
            return Err("grant_lifetime_exceeded");
        }
        if principal.kind != PrincipalKind::Actor
            || principal.id != format!("{ENROLLMENT_PRINCIPAL_PREFIX}{}", grant.enrollment_id)
        {
            return Err("grant_principal_mismatch");
        }
        let (thread_id, operation) = access.ok_or("thread_scope_required")?;
        if grant.thread_id != thread_id || !grant.operations.contains(&operation) {
            return Err("thread_scope_denied");
        }
        let fence = GrantFence {
            grant_id: grant.grant_id,
            thread_id: ThreadId(grant.thread_id),
            enrollment_id: grant.enrollment_id,
            operations: grant.operations.into_iter().map(GrantOperation::from).collect(),
            generation: grant.generation,
            incarnation: grant.incarnation,
            at: chrono::Utc::now(),
        };
        return Ok((principal, HistoryAuthority::Grant(fence)));
    }
    if is_enrollment_principal(&principal) {
        return Err("grant_token_required");
    }
    if let Some(scope) = &claims.thread_scope {
        if !valid_operations(&scope.operations) {
            return Err("invalid_thread_scope");
        }
        let (thread_id, operation) = access.ok_or("thread_scope_required")?;
        if scope.thread_id != thread_id || !scope.operations.contains(&operation) {
            return Err("thread_scope_denied");
        }
        return Ok((principal, HistoryAuthority::Scoped { generation: scope.grant_generation }));
    }
    Ok((principal, HistoryAuthority::Membership))
}

#[cfg(test)]
fn principal_from_jwt(token: &str, secret: &str) -> Result<Principal, &'static str> {
    principal_from_jwt_for_access(token, secret, None).map(|(principal, _)| principal)
}

#[cfg(test)]
fn principal_from_jwt_for_access(
    token: &str,
    secret: &str,
    access: Option<(uuid::Uuid, ThreadOperation)>,
) -> Result<(Principal, HistoryAuthority), &'static str> {
    authorize_claims(decode_legacy(token, secret)?, false, access)
}

/// Resolve principal from `Authorization` header according to [`AuthMode`].
pub fn principal_from_authorization(
    mode: &AuthMode,
    header: Option<&str>,
) -> Result<Principal, &'static str> {
    principal_for_access(mode, header, None).map(|(principal, _)| principal)
}

/// Unrestricted principal that must be asymmetrically (EdDSA/kid) signed.
/// Used where only the backend issuer may speak (delivery verification);
/// legacy HS256 and dev spoof tokens are refused.
pub fn signed_principal_from_authorization(
    mode: &AuthMode,
    header: Option<&str>,
) -> Result<Principal, &'static str> {
    let token = header
        .ok_or("missing_authorization")?
        .strip_prefix("Bearer ")
        .ok_or("authorization_must_be_bearer")?;
    let AuthMode::Keyset(verifier) = mode else {
        return Err("asymmetric_verification_unconfigured");
    };
    let (claims, asymmetric) = verify(verifier, token)?;
    if !asymmetric {
        return Err("asymmetric_signature_required");
    }
    match authorize_claims(claims, true, None)? {
        (principal, HistoryAuthority::Membership) => Ok(principal),
        _ => Err("restricted_credential"),
    }
}

/// Enforce signed attenuation before the caller checks persisted membership
/// (legacy scope) or live grant storage (grant credential).
pub fn principal_for_thread(
    mode: &AuthMode,
    header: Option<&str>,
    thread: uuid::Uuid,
    operation: ThreadOperation,
) -> Result<(Principal, HistoryAuthority), &'static str> {
    principal_for_access(mode, header, Some((thread, operation)))
}

fn principal_for_access(
    mode: &AuthMode,
    header: Option<&str>,
    access: Option<(uuid::Uuid, ThreadOperation)>,
) -> Result<(Principal, HistoryAuthority), &'static str> {
    let raw = header.ok_or("missing_authorization")?;
    let token = raw
        .strip_prefix("Bearer ")
        .ok_or("authorization_must_be_bearer")?;
    match mode {
        AuthMode::Dev => {
            // Prefer JWT shape if it looks like one and secret is set; else spoof.
            if token.matches('.').count() == 2 {
                if let Ok(secret) = std::env::var("MQ_JWT_SECRET") {
                    if !secret.is_empty() {
                        return authorize_claims(decode_legacy(token, &secret)?, false, access);
                    }
                }
            }
            principal_from_dev_token(token).map(|principal| (principal, HistoryAuthority::Membership))
        }
        AuthMode::Jwt { secret } => authorize_claims(decode_legacy(token, secret)?, false, access),
        AuthMode::Keyset(verifier) => {
            let (claims, asymmetric) = verify(verifier, token)?;
            authorize_claims(claims, asymmetric, access)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};

    const K1_PEM: &str = include_str!("../tests/fixtures/grant_k1.pem");
    const K2_PEM: &str = include_str!("../tests/fixtures/grant_k2.pem");
    const K1_JWKS: &str = include_str!("../tests/fixtures/jwks_k1.json");
    const K2_JWKS: &str = include_str!("../tests/fixtures/jwks_k2.json");

    fn both_jwks() -> String {
        let mut k1: serde_json::Value = serde_json::from_str(K1_JWKS).unwrap();
        let k2: serde_json::Value = serde_json::from_str(K2_JWKS).unwrap();
        k1["keys"].as_array_mut().unwrap().push(k2["keys"][0].clone());
        k1.to_string()
    }

    fn signed(pem: &str, kid: &str, claims: &serde_json::Value) -> String {
        let mut header = Header::new(Algorithm::EdDSA);
        header.kid = Some(kid.into());
        encode(&header, claims, &EncodingKey::from_ed_pem(pem.as_bytes()).unwrap()).unwrap()
    }

    fn grant_claims(thread: uuid::Uuid, enrollment: uuid::Uuid, operations: serde_json::Value) -> serde_json::Value {
        let now = chrono::Utc::now().timestamp();
        serde_json::json!({"iss":SIGNED_ISSUER,"aud":AUDIENCE,"iat":now,"exp":now+300,"jti":"fixture",
            "principal":{"kind":"actor","id":format!("enrollment:{enrollment}"),"org_id":"org"},
            "grant":{"grant_id":uuid::Uuid::new_v4(),"thread_id":thread,"enrollment_id":enrollment,
                "operations":operations,"generation":0,"incarnation":1}})
    }

    fn keyset(legacy: Option<&str>) -> AuthMode {
        AuthMode::Keyset(Arc::new(Verifier::new(&both_jwks(), legacy.map(Into::into)).unwrap()))
    }

    fn bearer(token: &str) -> String {
        format!("Bearer {token}")
    }

    #[test]
    fn signed_scope_cannot_escape_thread_operation_or_be_discarded() {
        let secret = "fixture-secret-at-least-32-bytes-long";
        let thread = uuid::Uuid::new_v4();
        let mut claims = serde_json::json!({"iss":"manderqueue", "aud":"manderqueue",
            "exp":chrono::Utc::now().timestamp()+300, "jti":"fixture",
            "principal":{"kind":"actor","id":"a1","org_id":"org"},
            "thread_scope":{"thread_id":thread,"grant_generation":0,"operations":["read"]}});
        let sign = |value: &serde_json::Value| encode(&Header::default(), value, &EncodingKey::from_secret(secret.as_bytes())).unwrap();
        let token = sign(&claims);
        assert!(principal_from_jwt(&token, secret).is_err());
        assert!(principal_from_jwt_for_access(&token, secret, Some((thread, ThreadOperation::Read))).is_ok());
        assert!(principal_from_jwt_for_access(&token, secret, Some((thread, ThreadOperation::Publish))).is_err());
        assert!(principal_from_jwt_for_access(&token, secret, Some((uuid::Uuid::new_v4(), ThreadOperation::Read))).is_err());
        for operations in [serde_json::json!([]), serde_json::json!(["read","read"]), serde_json::json!(["invite"])] {
            claims["thread_scope"]["operations"] = operations;
            assert!(principal_from_jwt_for_access(&sign(&claims), secret, Some((thread, ThreadOperation::Read))).is_err());
        }
    }

    #[test]
    fn jwt_requires_audience_issuer_expiry_and_token_identity() {
        let secret = "fixture-secret-at-least-32-bytes-long";
        let valid = serde_json::json!({"iss":"manderqueue", "aud":"manderqueue",
            "exp":chrono::Utc::now().timestamp()+300, "jti":"fixture",
            "principal":{"kind":"actor","id":"a1","org_id":"org"}});
        for field in ["iss", "aud", "exp", "jti"] {
            let mut claims = valid.clone();
            claims.as_object_mut().unwrap().remove(field);
            let token = encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_bytes())).unwrap();
            assert!(principal_from_jwt(&token, secret).is_err(), "missing {field}");
        }
        for (field, value) in [
            ("iss", serde_json::json!("other")),
            ("aud", serde_json::json!("other")),
            ("exp", serde_json::json!(1)),
            ("jti", serde_json::json!("")),
        ] {
            let mut claims = valid.clone();
            claims[field] = value;
            let token = encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_bytes())).unwrap();
            assert!(principal_from_jwt(&token, secret).is_err());
        }
    }

    #[test]
    fn dev_bearer_parses_but_cannot_spoof_enrollment_principals() {
        let p = principal_from_authorization(&AuthMode::Dev, Some("Bearer human:org-1:u1")).unwrap();
        assert_eq!(p.id, "u1");
        assert!(principal_from_authorization(&AuthMode::Dev, Some("Bearer actor:org-1:enrollment:x")).is_err());
    }

    #[test]
    fn jwt_hs256_parses() {
        let secret = "test-secret-for-mq";
        let token = encode(
            &Header::default(),
            &serde_json::json!({"iss":"manderqueue","aud":"manderqueue","exp":chrono::Utc::now().timestamp()+3600,
                "jti":"test-token","principal":{"kind":"intern_async","id":"i1","org_id":"org-1"}}),
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap();
        let mode = AuthMode::Jwt { secret: secret.into() };
        let p = principal_from_authorization(&mode, Some(&bearer(&token))).unwrap();
        assert_eq!(p.kind, PrincipalKind::InternAsync);
        assert_eq!(p.id, "i1");
    }

    #[test]
    fn jwks_accepts_only_ed25519_keys_with_unique_kids() {
        assert!(Verifier::new(&both_jwks(), None).is_ok());
        assert!(Verifier::new(r#"{"keys":[]}"#, None).is_err());
        let mut no_kid: serde_json::Value = serde_json::from_str(K1_JWKS).unwrap();
        no_kid["keys"][0].as_object_mut().unwrap().remove("kid");
        assert!(Verifier::new(&no_kid.to_string(), None).is_err());
        let mut dup: serde_json::Value = serde_json::from_str(K1_JWKS).unwrap();
        let first = dup["keys"][0].clone();
        dup["keys"].as_array_mut().unwrap().push(first);
        assert!(Verifier::new(&dup.to_string(), None).is_err());
        let rsa = r#"{"keys":[{"kty":"RSA","kid":"r","n":"AQAB","e":"AQAB"}]}"#;
        assert!(Verifier::new(rsa, None).is_err());
        assert!(Verifier::new(&both_jwks(), Some("short".into())).is_err());
    }

    #[test]
    fn grant_credentials_require_eddsa_kid_and_bound_principal() {
        let thread = uuid::Uuid::new_v4();
        let enrollment = uuid::Uuid::new_v4();
        let secret = "fixture-secret-at-least-32-bytes-long";
        let mode = keyset(Some(secret));
        let claims = grant_claims(thread, enrollment, serde_json::json!(["read"]));
        let good = signed(K1_PEM, "fixture-k1", &claims);
        let (_, authority) = principal_for_thread(&mode, Some(&bearer(&good)), thread, ThreadOperation::Read).unwrap();
        assert!(matches!(authority, HistoryAuthority::Grant(ref f) if f.incarnation == 1 && f.enrollment_id == enrollment));
        // Operation, thread and principal-only routes refuse.
        assert!(principal_for_thread(&mode, Some(&bearer(&good)), thread, ThreadOperation::Publish).is_err());
        assert!(principal_for_thread(&mode, Some(&bearer(&good)), uuid::Uuid::new_v4(), ThreadOperation::Read).is_err());
        assert!(principal_from_authorization(&mode, Some(&bearer(&good))).is_err());
        // Wrong key under a listed kid, unknown kid, missing kid.
        assert!(principal_for_thread(&mode, Some(&bearer(&signed(K2_PEM, "fixture-k1", &claims))), thread, ThreadOperation::Read).is_err());
        assert!(principal_for_thread(&mode, Some(&bearer(&signed(K1_PEM, "fixture-k9", &claims))), thread, ThreadOperation::Read).is_err());
        let no_kid = encode(&Header::new(Algorithm::EdDSA), &claims, &EncodingKey::from_ed_pem(K1_PEM.as_bytes()).unwrap()).unwrap();
        assert!(principal_for_thread(&mode, Some(&bearer(&no_kid)), thread, ThreadOperation::Read).is_err());
        // HS256 is never a fallback for grant credentials, even with the legacy secret.
        let mut legacy_grant = claims.clone();
        legacy_grant["iss"] = serde_json::json!(LEGACY_ISSUER);
        let hs = encode(&Header::default(), &legacy_grant, &EncodingKey::from_secret(secret.as_bytes())).unwrap();
        assert_eq!(principal_for_thread(&mode, Some(&bearer(&hs)), thread, ThreadOperation::Read).unwrap_err(), "grant_requires_asymmetric_signature");
        // Legacy HS256 for a reserved principal without a grant also refuses.
        let mut reserved = legacy_grant.clone();
        reserved.as_object_mut().unwrap().remove("grant");
        let hs = encode(&Header::default(), &reserved, &EncodingKey::from_secret(secret.as_bytes())).unwrap();
        assert_eq!(principal_for_thread(&mode, Some(&bearer(&hs)), thread, ThreadOperation::Read).unwrap_err(), "grant_token_required");
        // Principal must be the enrollment's derived identity, and lifetime is bounded.
        for (pointer, value) in [
            ("/principal/id", serde_json::json!("enrollment:other")),
            ("/principal/kind", serde_json::json!("human")),
            ("/exp", serde_json::json!(chrono::Utc::now().timestamp() + 301 + 60)),
        ] {
            let mut bad = claims.clone();
            *bad.pointer_mut(pointer).unwrap() = value;
            if pointer == "/exp" { bad["iat"] = serde_json::json!(chrono::Utc::now().timestamp()); }
            assert!(principal_for_thread(&mode, Some(&bearer(&signed(K1_PEM, "fixture-k1", &bad))), thread, ThreadOperation::Read).is_err(), "{pointer}");
        }
        let mut no_iat = claims.clone();
        no_iat.as_object_mut().unwrap().remove("iat");
        assert!(principal_for_thread(&mode, Some(&bearer(&signed(K1_PEM, "fixture-k1", &no_iat))), thread, ThreadOperation::Read).is_err());
        // Signed credentials must carry the backend issuer.
        let mut wrong_iss = claims.clone();
        wrong_iss["iss"] = serde_json::json!(LEGACY_ISSUER);
        assert!(principal_for_thread(&mode, Some(&bearer(&signed(K1_PEM, "fixture-k1", &wrong_iss))), thread, ThreadOperation::Read).is_err());
    }

    #[test]
    fn legacy_hs256_requires_configured_secret_and_rotation_removes_kids() {
        let secret = "fixture-secret-at-least-32-bytes-long";
        let owner = serde_json::json!({"iss":LEGACY_ISSUER,"aud":AUDIENCE,"exp":chrono::Utc::now().timestamp()+60,
            "jti":"fixture","principal":{"kind":"human","id":"u","org_id":"org"}});
        let hs = encode(&Header::default(), &owner, &EncodingKey::from_secret(secret.as_bytes())).unwrap();
        assert!(principal_from_authorization(&keyset(Some(secret)), Some(&bearer(&hs))).is_ok());
        assert_eq!(principal_from_authorization(&keyset(None), Some(&bearer(&hs))).unwrap_err(), "legacy_hs256_disabled");

        let mut signed_owner = owner.clone();
        signed_owner["iss"] = serde_json::json!(SIGNED_ISSUER);
        let t1 = signed(K1_PEM, "fixture-k1", &signed_owner);
        let t2 = signed(K2_PEM, "fixture-k2", &signed_owner);
        let verifier = Arc::new(Verifier::new(&both_jwks(), None).unwrap());
        let mode = AuthMode::Keyset(verifier.clone());
        assert!(principal_from_authorization(&mode, Some(&bearer(&t1))).is_ok());
        assert!(principal_from_authorization(&mode, Some(&bearer(&t2))).is_ok());
        verifier.replace_keys(K2_JWKS).unwrap();
        assert_eq!(principal_from_authorization(&mode, Some(&bearer(&t1))).unwrap_err(), "unknown_kid");
        assert!(principal_from_authorization(&mode, Some(&bearer(&t2))).is_ok());
        assert!(verifier.replace_keys("not json").is_err());
        assert_eq!(verifier.kids(), vec!["fixture-k2".to_string()]);
    }

    #[test]
    fn jwks_file_reload_applies_rotation_and_keeps_keys_on_bad_content() {
        let path = std::env::temp_dir().join(format!("mq-jwks-reload-{}.json", uuid::Uuid::new_v4()));
        std::fs::write(&path, both_jwks()).unwrap();
        let verifier = Arc::new(Verifier::from_file(&path, None).unwrap());
        let mode = AuthMode::Keyset(verifier.clone());
        assert!(verifier.has_file_source());
        assert_eq!(verifier.kids(), vec!["fixture-k1".to_string(), "fixture-k2".to_string()]);
        assert_eq!(verifier.reload_if_changed(), Ok(false), "unchanged content is a no-op");
        let owner = serde_json::json!({"iss":SIGNED_ISSUER,"aud":AUDIENCE,"exp":chrono::Utc::now().timestamp()+60,
            "jti":"fixture","principal":{"kind":"human","id":"u","org_id":"org"}});
        let t1 = signed(K1_PEM, "fixture-k1", &owner);
        let t2 = signed(K2_PEM, "fixture-k2", &owner);
        // Rotation completes by rewriting the file: k1 is removed.
        std::fs::write(&path, K2_JWKS).unwrap();
        assert_eq!(verifier.reload_if_changed(), Ok(true));
        assert_eq!(verifier.kids(), vec!["fixture-k2".to_string()]);
        assert_eq!(principal_from_authorization(&mode, Some(&bearer(&t1))).unwrap_err(), "unknown_kid");
        assert!(principal_from_authorization(&mode, Some(&bearer(&t2))).is_ok());
        // Invalid, empty-keyset and missing files keep the current keys.
        for bad in ["not json", r#"{"keys":[]}"#] {
            std::fs::write(&path, bad).unwrap();
            assert!(verifier.reload_if_changed().is_err());
            assert_eq!(verifier.kids(), vec!["fixture-k2".to_string()]);
        }
        std::fs::remove_file(&path).unwrap();
        assert!(verifier.reload_if_changed().is_err());
        assert!(principal_from_authorization(&mode, Some(&bearer(&t2))).is_ok());
        // Restoring valid content applies again; inline keysets never reload.
        std::fs::write(&path, both_jwks()).unwrap();
        assert_eq!(verifier.reload_if_changed(), Ok(true));
        assert!(principal_from_authorization(&mode, Some(&bearer(&t1))).is_ok());
        std::fs::remove_file(&path).unwrap();
        assert_eq!(Verifier::new(&both_jwks(), None).unwrap().reload_if_changed(), Ok(false));
        assert!(Verifier::from_file(&path, None).is_err());
    }
}
