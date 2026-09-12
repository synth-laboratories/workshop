//! Auth: `MQ_AUTH=dev` spoof bearer, or `MQ_AUTH=jwt` Synth-signed tokens.

use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use mq_core::{Principal, PrincipalKind};
use serde::Deserialize;

#[derive(Debug, Clone)]
pub enum AuthMode {
    /// `Bearer {kind}:{org_id}:{id}` — local/dev only.
    Dev,
    /// HS256 JWT with Synth claims (`MQ_JWT_SECRET`).
    Jwt { secret: String },
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
                let secret = std::env::var("MQ_JWT_SECRET")
                    .map_err(|_| "MQ_AUTH=jwt requires MQ_JWT_SECRET".to_string())?;
                if secret.as_bytes().len() < 32 {
                    return Err("MQ_JWT_SECRET requires at least 32 bytes".into());
                }
                Ok(Self::Jwt { secret })
            }
            other => Err(format!("unknown MQ_AUTH={other} (use dev|jwt)")),
        }
    }
}

#[derive(Debug, Deserialize)]
struct JwtClaims {
    /// Optional attenuation; it never replaces persisted thread membership.
    #[serde(default)]
    thread_scope: Option<ThreadScope>,
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
    exp: i64,
    jti: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ThreadScope {
    thread_id: uuid::Uuid,
    operations: Vec<ThreadOperation>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThreadOperation {
    Read,
    Publish,
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
    Ok(Principal {
        kind: parse_kind(kind)?,
        id: id.into(),
        org_id: org_id.into(),
    })
}

#[cfg(test)]
fn principal_from_jwt(token: &str, secret: &str) -> Result<Principal, &'static str> {
    principal_from_jwt_for_access(token, secret, None)
}

fn principal_from_jwt_for_access(
    token: &str,
    secret: &str,
    access: Option<(uuid::Uuid, ThreadOperation)>,
) -> Result<Principal, &'static str> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_audience(&["manderqueue"]);
    validation.set_issuer(&["manderqueue"]);
    validation.leeway = 0;
    validation.set_required_spec_claims(&["exp", "iss", "aud"]);

    let data = decode::<JwtClaims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|_| "invalid_jwt")?;

    let claims = data.claims;
    if let Some(scope) = &claims.thread_scope {
        if scope.operations.is_empty() || scope.operations.len() > 2
            || (scope.operations.len() == 2 && scope.operations[0] == scope.operations[1])
        {
            return Err("invalid_thread_scope");
        }
        let (thread_id, operation) = access.ok_or("thread_scope_required")?;
        if scope.thread_id != thread_id || !scope.operations.contains(&operation) {
            return Err("thread_scope_denied");
        }
    }
    if claims.jti.trim().is_empty() || claims.exp <= chrono::Utc::now().timestamp() {
        return Err("invalid_jwt");
    }
    if let Some(iss) = &claims.iss {
        if iss != "manderqueue" {
            return Err("bad_issuer");
        }
    }
    if let Some(aud) = &claims.aud {
        let ok = match aud {
            serde_json::Value::String(s) => s == "manderqueue",
            serde_json::Value::Array(arr) => arr.iter().any(|v| v.as_str() == Some("manderqueue")),
            _ => false,
        };
        if !ok {
            return Err("bad_audience");
        }
    }

    if let Some(p) = claims.principal {
        if p.id.trim().is_empty() || p.org_id.trim().is_empty() {
            return Err("missing_principal");
        }
        return Ok(Principal {
            kind: parse_kind(&p.kind)?,
            id: p.id,
            org_id: p.org_id,
        });
    }
    let kind = claims.kind.as_deref().ok_or("missing_principal")?;
    let id = claims.id.or(claims.sub).ok_or("missing_principal")?;
    let org_id = claims.org_id.ok_or("missing_principal")?;
    if id.trim().is_empty() || org_id.trim().is_empty() {
        return Err("missing_principal");
    }
    Ok(Principal {
        kind: parse_kind(kind)?,
        id,
        org_id,
    })
}

/// Resolve principal from `Authorization` header according to [`AuthMode`].
pub fn principal_from_authorization(
    mode: &AuthMode,
    header: Option<&str>,
) -> Result<Principal, &'static str> {
    principal_for_access(mode, header, None)
}

/// Enforce signed attenuation before the caller checks persisted membership.
pub fn principal_for_thread(
    mode: &AuthMode,
    header: Option<&str>,
    thread: uuid::Uuid,
    operation: ThreadOperation,
) -> Result<Principal, &'static str> {
    principal_for_access(mode, header, Some((thread, operation)))
}

fn principal_for_access(
    mode: &AuthMode,
    header: Option<&str>,
    access: Option<(uuid::Uuid, ThreadOperation)>,
) -> Result<Principal, &'static str> {
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
                        return principal_from_jwt_for_access(token, &secret, access);
                    }
                }
            }
            principal_from_dev_token(token)
        }
        AuthMode::Jwt { secret } => principal_from_jwt_for_access(token, secret, access),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};

    #[test]
    fn signed_scope_cannot_escape_thread_operation_or_be_discarded() {
        let secret = "fixture-secret-at-least-32-bytes-long";
        let thread = uuid::Uuid::new_v4();
        let mut claims = serde_json::json!({"iss":"manderqueue", "aud":"manderqueue",
            "exp":chrono::Utc::now().timestamp()+300, "jti":"fixture",
            "principal":{"kind":"actor","id":"a1","org_id":"org"},
            "thread_scope":{"thread_id":thread,"operations":["read"]}});
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
            let token = encode(
                &Header::default(),
                &claims,
                &EncodingKey::from_secret(secret.as_bytes()),
            )
            .unwrap();
            assert!(
                principal_from_jwt(&token, secret).is_err(),
                "missing {field}"
            );
        }
        for (field, value) in [
            ("iss", serde_json::json!("other")),
            ("aud", serde_json::json!("other")),
            ("exp", serde_json::json!(1)),
            ("jti", serde_json::json!("")),
        ] {
            let mut claims = valid.clone();
            claims[field] = value;
            let token = encode(
                &Header::default(),
                &claims,
                &EncodingKey::from_secret(secret.as_bytes()),
            )
            .unwrap();
            assert!(principal_from_jwt(&token, secret).is_err());
        }
    }

    #[test]
    fn dev_bearer_parses() {
        let p =
            principal_from_authorization(&AuthMode::Dev, Some("Bearer human:org-1:u1")).unwrap();
        assert_eq!(p.id, "u1");
    }

    #[test]
    fn jwt_hs256_parses() {
        #[derive(serde::Serialize)]
        struct Claims {
            iss: &'static str,
            aud: &'static str,
            exp: i64,
            jti: &'static str,
            principal: JwtPrincipalSer,
        }
        #[derive(serde::Serialize)]
        struct JwtPrincipalSer {
            kind: &'static str,
            id: &'static str,
            org_id: &'static str,
        }
        let secret = "test-secret-for-mq";
        let token = encode(
            &Header::default(),
            &Claims {
                iss: "manderqueue",
                aud: "manderqueue",
                exp: chrono::Utc::now().timestamp() + 3600,
                jti: "test-token",
                principal: JwtPrincipalSer {
                    kind: "intern_async",
                    id: "i1",
                    org_id: "org-1",
                },
            },
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap();
        let mode = AuthMode::Jwt {
            secret: secret.into(),
        };
        let p = principal_from_authorization(&mode, Some(&format!("Bearer {token}"))).unwrap();
        assert_eq!(p.kind, PrincipalKind::InternAsync);
        assert_eq!(p.id, "i1");
    }
}
