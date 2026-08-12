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

#[derive(Clone)]
struct PendingPair {
    device_code: String,
    verification_uri: String,
    expires_at_epoch_s: u64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SignInBegin {
    pub verification_uri: String,
    pub expires_at_epoch_s: u64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum SignInPoll {
    /// Browser approval not observed yet; keep polling.
    Pending,
    /// Key received, stored, and runtime reloaded.
    Active,
    /// Code expired or consumed; a fresh begin is required.
    Expired { reason: String },
}

#[derive(Deserialize)]
struct InitResponse {
    device_code: String,
    verification_uri: String,
    expires_in: u64,
}

#[derive(Deserialize)]
struct TokenResponse {
    synth_api_key: String,
}

pub struct DeviceAuthManager {
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
            if pending.expires_at_epoch_s > now_epoch_s() + 10 {
                return Ok(SignInBegin {
                    verification_uri: pending.verification_uri,
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
        let verification_uri = if init.verification_uri.starts_with("http") {
            init.verification_uri
        } else {
            format!("{origin}{}", init.verification_uri)
        };
        let pending = PendingPair {
            device_code: init.device_code,
            verification_uri: verification_uri.clone(),
            expires_at_epoch_s: now_epoch_s() + init.expires_in,
        };
        let expires_at_epoch_s = pending.expires_at_epoch_s;
        *self.pending.lock().unwrap() = Some(pending);
        Ok(SignInBegin {
            verification_uri,
            expires_at_epoch_s,
        })
    }

    /// One poll step. On success the key is handed to `store`; the caller owns
    /// runtime reload. The key never leaves the closure.
    pub async fn poll(
        &self,
        origin: &str,
        store: impl FnOnce(&str) -> Result<()>,
    ) -> Result<SignInPoll> {
        let Some(pending) = self.pending.lock().unwrap().clone() else {
            return Ok(SignInPoll::Expired {
                reason: "no sign-in in progress".into(),
            });
        };
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
                store(&token.synth_api_key)?;
                *self.pending.lock().unwrap() = None;
                Ok(SignInPoll::Active)
            }
            428 => Ok(SignInPoll::Pending),
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

    pub fn cancel(&self) {
        *self.pending.lock().unwrap() = None;
    }
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
            .poll(&origin, move |k| {
                s1.lock().unwrap().push(k.into());
                Ok(())
            })
            .await
            .unwrap();
        assert_eq!(first, SignInPoll::Pending);
        let s2 = stored.clone();
        let second = manager
            .poll(&origin, move |k| {
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
            manager.poll(&origin, |_| Ok(())).await.unwrap(),
            SignInPoll::Expired { .. }
        ));
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
        let result = manager.poll(&origin, |_| Ok(())).await.unwrap();
        assert!(matches!(result, SignInPoll::Expired { .. }));
        handle.join().unwrap();
    }
}
