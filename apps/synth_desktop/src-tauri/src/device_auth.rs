//! Browser device-pairing sign-in against the Workshop web app.
//!
//! Interim alpha lane: the flow terminates in a Synth API key stored through
//! `synth_config` (0600 env file). The OAuth 2.1 promotion replaces the
//! plumbing under the same commands without renderer changes.
//!
//! The renderer never sees the device code or the API key — commands return
//! only display-safe state.

use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

const INIT_PATH: &str = "/api/auth/device/init";
const TOKEN_PATH: &str = "/api/auth/device/token";
const PROD_WORKSHOP_URL: &str = "https://www.usesynth.ai";
const LOCAL_WORKSHOP_URL: &str = "http://localhost:3000";
const DEFAULT_POLL_INTERVAL_S: u64 = 4;
const MAX_POLL_INTERVAL_S: u64 = 30;

#[derive(Clone)]
struct PendingPair {
    issuer: String,
    device_code: String,
    verification_uri: String,
    user_code: Option<String>,
    interval_s: u64,
    expires_at_epoch_s: u64,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct SignInBegin {
    pub verification_uri: String,
    pub user_code: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub interval_s: u64,
    #[specta(type = specta_typescript::Number)]
    pub expires_at_epoch_s: u64,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "status"
)]
pub enum SignInPoll {
    /// Browser approval not observed yet; keep polling.
    Pending {
        #[specta(type = specta_typescript::Number)]
        retry_in_s: u64,
    },
    /// Key received, stored, and runtime reloaded.
    Active,
    /// Code expired or consumed; a fresh begin is required.
    Expired { reason: String },
}

#[derive(Deserialize)]
struct InitResponse {
    device_code: String,
    verification_uri: String,
    #[serde(default)]
    verification_uri_complete: Option<String>,
    #[serde(default)]
    user_code: Option<String>,
    #[serde(default)]
    interval: Option<u64>,
    expires_in: u64,
}

#[derive(Deserialize)]
struct TokenResponse {
    synth_api_key: String,
    #[serde(default)]
    key_scope: Option<String>,
}

pub struct DeviceAuthManager {
    pub operation: tokio::sync::Mutex<()>,
    pending: Mutex<Option<PendingPair>>,
    http: reqwest::Client,
}

fn now_epoch_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Workshop web origin: env override, else local when the backend is local,
/// else production. The device endpoints live on the web app, not the API.
pub fn workshop_origin() -> String {
    if let Ok(value) = std::env::var("SYNTH_WORKSHOP_URL") {
        let trimmed = value.trim().trim_end_matches('/').to_owned();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }
    let backend_is_local = crate::synth_config::resolve()
        .map(|resolved| {
            resolved.backend_url.contains("127.0.0.1") || resolved.backend_url.contains("localhost")
        })
        .unwrap_or(false);
    if backend_is_local {
        LOCAL_WORKSHOP_URL.into()
    } else {
        PROD_WORKSHOP_URL.into()
    }
}

impl DeviceAuthManager {
    pub fn new() -> Self {
        Self {
            operation: tokio::sync::Mutex::new(()),
            pending: Mutex::new(None),
            http: crate::http::http_client_builder()
                // A redirect here means the pairing routes are auth-gated
                // (misconfigured deploy); surface that instead of HTML.
                .redirect(reqwest::redirect::Policy::none())
                .timeout(crate::limits::DEVICE_AUTH_TIMEOUT)
                .build()
                .expect("device-auth HTTP client"),
        }
    }

    /// Start (or resume) a pairing. Idempotent: an unexpired pending pairing
    /// is returned as-is so "Reopen browser" is just begin() again.
    pub async fn begin(&self, origin: &str) -> Result<SignInBegin> {
        if let Some(pending) = self.pending.lock().unwrap().clone() {
            if pending.issuer == origin && pending.expires_at_epoch_s > now_epoch_s() + 10 {
                return Ok(SignInBegin {
                    verification_uri: pending.verification_uri,
                    user_code: pending.user_code,
                    interval_s: pending.interval_s,
                    expires_at_epoch_s: pending.expires_at_epoch_s,
                });
            }
        }
        let response = self
            .http
            .post(format!("{origin}{INIT_PATH}"))
            .header("content-type", "application/json")
            .send()
            .await
            .context("reach the Workshop sign-in service")?;
        if response.status().is_redirection() {
            return Err(anyhow!(
                "sign-in service redirected the pairing request; the Workshop deploy is missing the public device routes"
            ));
        }
        if !response.status().is_success() {
            return Err(anyhow!(
                "sign-in service refused pairing start ({})",
                response.status()
            ));
        }
        let init: InitResponse = response.json().await.context("parse pairing start")?;
        let raw_uri = init
            .verification_uri_complete
            .unwrap_or(init.verification_uri);
        let verification_uri = if raw_uri.starts_with("http") {
            raw_uri
        } else {
            format!("{origin}{raw_uri}")
        };
        ensure_same_origin(origin, &verification_uri)?;
        let pending = PendingPair {
            issuer: origin.to_owned(),
            device_code: init.device_code,
            verification_uri: verification_uri.clone(),
            user_code: init.user_code.clone(),
            interval_s: init
                .interval
                .unwrap_or(DEFAULT_POLL_INTERVAL_S)
                .clamp(1, MAX_POLL_INTERVAL_S),
            expires_at_epoch_s: now_epoch_s() + init.expires_in,
        };
        let expires_at_epoch_s = pending.expires_at_epoch_s;
        let interval_s = pending.interval_s;
        *self.pending.lock().unwrap() = Some(pending);
        Ok(SignInBegin {
            verification_uri,
            user_code: init.user_code,
            interval_s,
            expires_at_epoch_s,
        })
    }

    /// One poll step. On success the key is handed to `store`; the caller owns
    /// runtime reload. The key never leaves the closure.
    pub async fn poll(
        &self,
        origin: &str,
        store: impl FnOnce(&str, bool) -> Result<()>,
    ) -> Result<SignInPoll> {
        let Some(pending) = self.pending.lock().unwrap().clone() else {
            return Ok(SignInPoll::Expired {
                reason: "no sign-in in progress".into(),
            });
        };
        if pending.issuer != origin {
            return Err(anyhow!(
                "Workshop server changed during pairing; cancel and start again"
            ));
        }
        if pending.expires_at_epoch_s <= now_epoch_s() {
            *self.pending.lock().unwrap() = None;
            return Ok(SignInPoll::Expired {
                reason: "the browser link expired; start sign-in again".into(),
            });
        }
        let response = self
            .http
            .post(format!("{origin}{TOKEN_PATH}"))
            .json(&serde_json::json!({ "device_code": pending.device_code }))
            .send()
            .await
            .context("reach the Workshop sign-in service")?;
        match response.status().as_u16() {
            200 => {
                let token: TokenResponse = response.json().await.context("parse pairing result")?;
                store(
                    &token.synth_api_key,
                    token.key_scope.as_deref() == Some("device"),
                )?;
                *self.pending.lock().unwrap() = None;
                Ok(SignInPoll::Active)
            }
            428 => Ok(SignInPoll::Pending {
                retry_in_s: pending.interval_s,
            }),
            429 => {
                let retry_in_s = response
                    .headers()
                    .get("retry-after")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.trim().parse::<u64>().ok())
                    .unwrap_or(pending.interval_s.saturating_mul(2))
                    .clamp(pending.interval_s, MAX_POLL_INTERVAL_S);
                if let Some(current) = self.pending.lock().unwrap().as_mut() {
                    if current.device_code == pending.device_code
                        && current.issuer == pending.issuer
                    {
                        current.interval_s = retry_in_s;
                    }
                }
                Ok(SignInPoll::Pending { retry_in_s })
            }
            404 | 410 => {
                *self.pending.lock().unwrap() = None;
                Ok(SignInPoll::Expired {
                    reason: "the browser link expired; start sign-in again".into(),
                })
            }
            409 => {
                *self.pending.lock().unwrap() = None;
                Ok(SignInPoll::Expired {
                    reason: "this link was already used; start sign-in again".into(),
                })
            }
            other => Err(anyhow!("sign-in service error ({other})")),
        }
    }

    /// Revoke at the recorded issuer; failures leave the local credential intact
    /// so sign-out can be retried and never reports a false server-side success.
    pub async fn revoke(&self, origin: &str, key: &str) -> Result<()> {
        let url = reqwest::Url::parse(origin).context("invalid pairing issuer")?;
        let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        if url.scheme() != "https" && !(url.scheme() == "http" && local) {
            return Err(anyhow!("pairing issuer requires HTTPS outside localhost"));
        }
        let response = self
            .http
            .post(format!(
                "{}/api/auth/device/revoke",
                origin.trim_end_matches('/')
            ))
            .bearer_auth(key)
            .send()
            .await
            .context("could not revoke this device; retry sign-out when connected")?;
        if response.status() != reqwest::StatusCode::NO_CONTENT {
            return Err(anyhow!(
                "device revocation failed ({}); retry sign-out",
                response.status()
            ));
        }
        Ok(())
    }

    pub fn cancel(&self) {
        *self.pending.lock().unwrap() = None;
    }
}

/// Opening a link carries the desktop's authority. An issuer must not redirect
/// pairing to another origin, even if the response came from the right server.
fn ensure_same_origin(origin: &str, uri: &str) -> Result<()> {
    let origin = reqwest::Url::parse(origin).context("parse Workshop origin")?;
    let target = reqwest::Url::parse(uri).context("parse verification link")?;
    if !matches!(target.scheme(), "https" | "http")
        || target.scheme() != origin.scheme()
        || target.host_str() != origin.host_str()
        || target.port_or_known_default() != origin.port_or_known_default()
        || !target.username().is_empty()
        || target.password().is_some()
    {
        return Err(anyhow!(
            "sign-in service returned an untrusted verification link; refusing to open it"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn spawn_fake_workshop(
        responses: Vec<(u16, String)>,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = [0u8; 4096];
                let n = stream.read(&mut buf).unwrap();
                seen.push(String::from_utf8_lossy(&buf[..n]).into_owned());
                let reason = match status {
                    200 => "OK",
                    428 => "Precondition Required",
                    410 => "Gone",
                    _ => "X",
                };
                let payload = format!(
                    "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(payload.as_bytes()).unwrap();
            }
            seen
        });
        (origin, handle)
    }

    #[tokio::test]
    async fn begin_then_poll_to_active_stores_key_exactly_once() {
        let init_body = r#"{"device_code":"abc123","verification_uri":"/signin?redirect_to=x","expires_in":600}"#;
        let (origin, handle) = spawn_fake_workshop(vec![
            (200, init_body.into()),
            (428, r#"{"error":"AUTH_PENDING"}"#.into()),
            (
                200,
                r#"{"synth_api_key":"sk_live_devicepair_secret"}"#.into(),
            ),
        ]);
        let manager = DeviceAuthManager::new();
        let begin = manager.begin(&origin).await.unwrap();
        assert!(begin.verification_uri.starts_with(&origin));
        // idempotent begin returns the same pending link without HTTP
        assert_eq!(manager.begin(&origin).await.unwrap(), begin);

        let stored = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let s1 = stored.clone();
        let first = manager
            .poll(&origin, move |k, _| {
                s1.lock().unwrap().push(k.into());
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(
            first,
            SignInPoll::Pending {
                retry_in_s: DEFAULT_POLL_INTERVAL_S
            }
        );
        let s2 = stored.clone();
        let second = manager
            .poll(&origin, move |k, _| {
                s2.lock().unwrap().push(k.into());
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(second, SignInPoll::Active);
        assert_eq!(*stored.lock().unwrap(), vec!["sk_live_devicepair_secret"]);
        let seen = handle.join().unwrap();
        assert!(seen[1].contains("abc123") && seen[2].contains("abc123"));
        // pairing state is cleared after success
        assert!(matches!(
            manager.poll(&origin, |_, _| Ok(())).await.unwrap(),
            SignInPoll::Expired { .. }
        ));
    }

    #[tokio::test]
    async fn polling_slowdown_is_pending_and_persists_the_backoff() {
        let init = r#"{"device_code":"abc123","verification_uri":"/signin","user_code":"ABCD-1234","interval":5,"expires_in":600}"#;
        let (origin, handle) = spawn_fake_workshop(vec![
            (200, init.into()),
            (429, "{}".into()),
            (428, "{}".into()),
        ]);
        let manager = DeviceAuthManager::new();
        let begin = manager.begin(&origin).await.unwrap();
        assert_eq!(begin.user_code.as_deref(), Some("ABCD-1234"));
        assert_eq!(begin.interval_s, 5);
        assert_eq!(
            manager.poll(&origin, |_, _| Ok(())).await.unwrap(),
            SignInPoll::Pending { retry_in_s: 10 }
        );
        assert_eq!(
            manager.poll(&origin, |_, _| Ok(())).await.unwrap(),
            SignInPoll::Pending { retry_in_s: 10 }
        );
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn begin_refuses_verification_links_outside_the_issuer() {
        let init = r#"{"device_code":"abc123","verification_uri":"https://other.example/signin","expires_in":600}"#;
        let (origin, handle) = spawn_fake_workshop(vec![(200, init.into())]);
        let manager = DeviceAuthManager::new();
        assert!(manager
            .begin(&origin)
            .await
            .unwrap_err()
            .to_string()
            .contains("refusing to open"));
        assert!(manager.pending.lock().unwrap().is_none());
        handle.join().unwrap();
    }

    #[tokio::test]
    async fn begin_prefers_the_complete_same_origin_verification_link() {
        let init = r#"{"device_code":"abc123","verification_uri":"/signin","verification_uri_complete":"/signin?code=ABCD","expires_in":600}"#;
        let (origin, handle) = spawn_fake_workshop(vec![(200, init.into())]);
        let begin = DeviceAuthManager::new().begin(&origin).await.unwrap();
        assert_eq!(begin.verification_uri, format!("{origin}/signin?code=ABCD"));
        handle.join().unwrap();
    }

    #[test]
    fn verification_link_rejects_scheme_port_and_userinfo_changes() {
        for uri in [
            "http://example.com/signin",
            "https://example.com:444/signin",
            "https://user@example.com/signin",
            "file:///signin",
        ] {
            assert!(ensure_same_origin("https://example.com", uri).is_err());
        }
        assert!(ensure_same_origin("https://example.com", "https://example.com/signin").is_ok());
    }

    #[tokio::test]
    async fn revoke_requires_confirmation_and_never_follows_redirects() {
        for status in [204, 302, 500] {
            let (origin, handle) = spawn_fake_workshop(vec![(status, String::new())]);
            let manager = DeviceAuthManager::new();
            assert_eq!(
                manager.revoke(&origin, "device-test-key").await.is_ok(),
                status == 204
            );
            let seen = handle.join().unwrap();
            assert!(seen[0].starts_with("POST /api/auth/device/revoke "));
            assert!(seen[0]
                .to_lowercase()
                .contains("authorization: bearer device-test-key"));
        }
        assert!(DeviceAuthManager::new()
            .revoke("http://example.com", "never-send")
            .await
            .is_err());
    }

    #[test]
    fn legacy_token_does_not_claim_device_ownership() {
        let legacy: TokenResponse = serde_json::from_str(r#"{"synth_api_key":"legacy"}"#).unwrap();
        assert!(legacy.key_scope.is_none());
        let device: TokenResponse =
            serde_json::from_str(r#"{"synth_api_key":"device","key_scope":"device"}"#).unwrap();
        assert_eq!(device.key_scope.as_deref(), Some("device"));
    }

    #[tokio::test]
    async fn expired_code_clears_pending() {
        let init_body = r#"{"device_code":"gone","verification_uri":"/signin","expires_in":600}"#;
        let (origin, handle) = spawn_fake_workshop(vec![
            (200, init_body.into()),
            (410, r#"{"error":"DEVICE_CODE_EXPIRED"}"#.into()),
        ]);
        let manager = DeviceAuthManager::new();
        manager.begin(&origin).await.unwrap();
        let result = manager.poll(&origin, |_, _| Ok(())).await.unwrap();
        assert!(matches!(result, SignInPoll::Expired { .. }));
        handle.join().unwrap();
    }
}
