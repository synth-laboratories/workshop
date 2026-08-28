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
const REVOKE_PATH: &str = "/api/auth/device/revoke";
const PROD_WORKSHOP_URL: &str = "https://www.usesynth.ai";
const LOCAL_WORKSHOP_URL: &str = "http://localhost:3000";
/// Poll cadence when the server predates the RFC 8628 `interval` field.
const DEFAULT_POLL_INTERVAL_S: u64 = 4;
/// Ceiling for server-directed backoff so a slow-down can never look like a hang.
const MAX_POLL_INTERVAL_S: u64 = 30;

#[derive(Clone)]
struct PendingPair {
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
    /// Human-comparable pairing code; the browser approval page shows the
    /// same code so the user can confirm the request came from this desktop.
    pub user_code: Option<String>,
    #[specta(type = specta_typescript::Number)]
    pub expires_at_epoch_s: u64,
    #[specta(type = specta_typescript::Number)]
    pub interval_s: u64,
}

#[derive(Serialize, Clone, Debug, PartialEq, specta::Type)]
#[serde(
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    tag = "status"
)]
pub enum SignInPoll {
    /// Browser approval not observed yet; poll again after `retry_in_s`.
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
                    user_code: pending.user_code,
                    expires_at_epoch_s: pending.expires_at_epoch_s,
                    interval_s: pending.interval_s,
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
        // This URL is handed to the system browser; never open one that
        // points anywhere but the origin the pairing started against.
        ensure_same_origin(origin, &verification_uri)?;
        let pending = PendingPair {
            device_code: init.device_code,
            verification_uri: verification_uri.clone(),
            user_code: init.user_code.clone(),
            interval_s: init
                .interval
                .unwrap_or(DEFAULT_POLL_INTERVAL_S)
                .clamp(1, MAX_POLL_INTERVAL_S),
            expires_at_epoch_s: now_epoch_s() + init.expires_in,
        };
        let begin = SignInBegin {
            verification_uri,
            user_code: pending.user_code.clone(),
            expires_at_epoch_s: pending.expires_at_epoch_s,
            interval_s: pending.interval_s,
        };
        *self.pending.lock().unwrap() = Some(pending);
        Ok(begin)
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
            428 => Ok(SignInPoll::Pending {
                retry_in_s: pending.interval_s,
            }),
            // Rate-limited is a pacing signal, not a failure: honor the
            // server's Retry-After (RFC 8628 slow_down), capped so the UI
            // never looks stalled.
            429 => {
                let retry_in_s = response
                    .headers()
                    .get("retry-after")
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.trim().parse::<u64>().ok())
                    .unwrap_or(pending.interval_s * 2)
                    .clamp(pending.interval_s, MAX_POLL_INTERVAL_S);
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

    /// Server-side revocation of a desktop-managed key at sign-out. The key
    /// itself is the authority; the caller treats failure as best-effort —
    /// local deletion must never be blocked on reaching the service.
    pub async fn revoke_key(&self, origin: &str, api_key: &str) -> Result<()> {
        let response = self
            .http
            .post(format!("{origin}{REVOKE_PATH}"))
            .bearer_auth(api_key)
            .send()
            .await
            .context("reach the Workshop sign-in service")?;
        if response.status() != reqwest::StatusCode::NO_CONTENT {
            return Err(anyhow!(
                "sign-in service refused key revocation ({})",
                response.status()
            ));
        }
        Ok(())
    }

    pub fn cancel(&self) {
        *self.pending.lock().unwrap() = None;
    }
}

/// The verification URL is opened in the system browser on the host's
/// authority; a compromised or misconfigured service must not be able to
/// point it at another site.
fn ensure_same_origin(origin: &str, uri: &str) -> Result<()> {
    let origin = reqwest::Url::parse(origin).context("parse Workshop origin")?;
    let target = reqwest::Url::parse(uri).context("parse verification link")?;
    if target.scheme() != origin.scheme()
        || target.host_str() != origin.host_str()
        || target.port_or_known_default() != origin.port_or_known_default()
    {
        return Err(anyhow!(
            "sign-in service returned a verification link outside {origin}; refusing to open it"
        ));
    }
    Ok(())
}

