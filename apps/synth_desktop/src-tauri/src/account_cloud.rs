//! Synth Cloud account snapshot client.
//!
//! One backend document (`GET /api/v1/desktop/account-snapshot`) is the whole
//! cloud truth for the shell: identity, org, plan, allowance, usage windows,
//! and hosted billing URLs. This module fetches it with the desktop-managed API
//! key, caches it briefly, and — when the network or the backend is down —
//! serves the last good copy marked stale rather than a blank account menu.
//!
//! The key never leaves this process: the renderer receives only the parsed,
//! display-safe snapshot.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const SNAPSHOT_PATH: &str = "/api/v1/desktop/account-snapshot";
const CHECKOUT_PATH: &str = "/api/v1/billing/checkout-session";
const PORTAL_PATH: &str = "/api/v1/billing/portal-session";
const CREDITS_UNKNOWN_PATH: &str = "/api/v1/billing/credits-unknown";
/// Long enough that opening the menu repeatedly costs nothing, short enough
/// that a checkout completed in the browser shows up on the next open.
const CACHE_TTL_SECONDS: i64 = 60;
pub const SCHEMA_VERSION: &str = "synth.desktop-account.v1";

/// Billing destinations are security-sensitive even though the backend issues
/// them: opening an arbitrary origin would turn a compromised backend response
/// into a trusted-looking payment prompt. Production accepts only Synth web
/// properties and Stripe-hosted checkout. Named debug instances may also open
/// the configured development backend's own origin.
pub(crate) fn validate_billing_url(
    value: &str,
    backend_url: &str,
    allow_development_origin: bool,
) -> Result<String> {
    let url = reqwest::Url::parse(value)
        .map_err(|_| anyhow!("Synth Cloud returned an invalid billing URL."))?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return Err(anyhow!("Synth Cloud returned an untrusted billing URL."));
    }
    let host = url
        .host_str()
        .ok_or_else(|| anyhow!("Synth Cloud returned an untrusted billing URL."))?;
    let source_owned = host == "usesynth.ai"
        || host.ends_with(".usesynth.ai")
        || host == "stripe.com"
        || host.ends_with(".stripe.com");
    let development_owned = allow_development_origin
        && reqwest::Url::parse(backend_url)
            .ok()
            .and_then(|backend| backend.host_str().map(str::to_owned))
            .as_deref()
            == Some(host);
    if !source_owned && !development_owned {
        return Err(anyhow!("Synth Cloud returned an untrusted billing URL."));
    }
    Ok(url.to_string())
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, specta::Type)]
pub struct CloudAccount {
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, specta::Type)]
pub struct CloudOrganization {
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, specta::Type)]
pub struct CloudPlan {
    pub tier: String,
    pub display_name: String,
    pub state: String,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub price_cents: i64,
    #[serde(default)]
    pub renews_at: Option<String>,
    #[serde(default)]
    pub is_paid: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, specta::Type)]
pub struct CloudAllowance {
    /// `None` means the backend does not meter this account in dollars. The UI
    /// must then show no dollar figure at all.
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub limit_cents: Option<i64>,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub used_cents: Option<i64>,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub remaining_cents: Option<i64>,
    #[serde(default)]
    pub resets_at: Option<String>,
    #[serde(default)]
    pub source: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, specta::Type)]
pub struct CloudUsageWindow {
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub events: i64,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub billed_cents: i64,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub nominal_cents: i64,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub tokens: i64,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub runtime_seconds: i64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, specta::Type)]
pub struct CloudUsageWindows {
    #[serde(default)]
    pub today: CloudUsageWindow,
    #[serde(default)]
    pub seven_days: CloudUsageWindow,
    #[serde(default)]
    pub thirty_days: CloudUsageWindow,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, specta::Type)]
pub struct CloudBillingActions {
    #[serde(default)]
    pub checkout_url: Option<String>,
    #[serde(default)]
    pub portal_url: Option<String>,
    #[serde(default)]
    pub upgrade_tier: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, specta::Type)]
pub struct CloudPlanOption {
    pub tier: String,
    pub display_name: String,
    #[specta(type = specta_typescript::Number)]
    pub price_cents: i64,
    #[specta(type = specta_typescript::Number)]
    pub monthly_allowance_cents: i64,
    #[serde(default)]
    pub interval: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, specta::Type)]
pub struct CloudSnapshot {
    #[serde(default)]
    pub schema_version: String,
    pub status: String,
    pub account: CloudAccount,
    #[serde(default)]
    pub organization: Option<CloudOrganization>,
    pub plan: CloudPlan,
    pub allowance: CloudAllowance,
    #[serde(default)]
    pub usage: CloudUsageWindows,
    #[serde(default)]
    pub billing_actions: CloudBillingActions,
    #[serde(default)]
    pub catalog: Vec<CloudPlanOption>,
    #[serde(default)]
    pub generated_at: String,
    #[serde(default)]
    pub degraded: Vec<String>,
}

/// What the shell learned about the cloud on this read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SnapshotRead {
    pub snapshot: Option<CloudSnapshot>,
    /// True when `snapshot` is a cached copy served because the refresh failed.
    pub stale: bool,
    /// Display-safe reason the live fetch failed, if it did.
    pub error: Option<String>,
    pub fetched_at: Option<DateTime<Utc>>,
    /// True when the caller is not signed in (no desktop-managed key).
    pub unauthenticated: bool,
    /// `auth` | `outage` | `malformed` when `error` is set; never a secret.
    pub failure_kind: Option<String>,
}

/// Enforce the native spend boundary for a Synth Cloud turn.
///
/// The renderer may disable controls for presentation, but only this host-side
/// decision is authoritative. A stale snapshot is not sufficient: another
/// device or an earlier concurrent request may have consumed its balance.
pub fn validate_turn_admission(read: &SnapshotRead) -> Result<(), String> {
    if read.unauthenticated {
        return Err("Synth Cloud rejected this device's key. Sign in again to continue.".into());
    }
    if read.stale {
        return Err(
            "Synth Cloud balance could not be refreshed. No metered turn was started.".into(),
        );
    }
    let snapshot = read.snapshot.as_ref().ok_or_else(|| {
        read.error.clone().unwrap_or_else(|| {
            "Synth Cloud balance is unknown. No metered turn was started.".into()
        })
    })?;
    if !matches!(snapshot.status.as_str(), "active") {
        return Err(format!(
            "Synth Cloud account state is {}. No metered turn was started.",
            snapshot.status
        ));
    }
    if snapshot
        .allowance
        .remaining_cents
        .is_some_and(|cents| cents <= 0)
    {
        return Err("Synth Cloud balance is exhausted. No metered turn was started.".into());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum BillingAction {
    Upgrade,
    Manage,
}

#[derive(Deserialize)]
struct HostedBillingSession {
    url: String,
    /// `provider` = Autumn/Stripe hosted checkout. `hosted_web` = web-app
    /// fallback. Upgrade must be provider-resolved for Gate F.
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Clone, Debug)]
struct CachedSnapshot {
    snapshot: CloudSnapshot,
    fetched_at: DateTime<Utc>,
    /// Identity of the connection the copy belongs to; a backend or key change
    /// must never be answered from another account's cache.
    identity: u64,
}

fn connection_identity(backend_url: &str, api_key: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    backend_url.hash(&mut hasher);
    api_key.hash(&mut hasher);
    hasher.finish()
}

/// Why an account read failed.
///
/// The renderer composes this into user-visible chrome (`Showing the last known
/// plan — {message}`), so every variant must render one complete, stable, public
/// sentence. Transport and parser text is backend- or network-controlled and
/// never reaches that copy; it is kept beside the variant for native diagnosis.
#[derive(Clone, Debug)]
pub enum AccountError {
    /// Connection refused, DNS failure, TLS failure, or timeout.
    Unreachable {
        detail: String,
    },
    Unauthorized,
    /// The backend answered, but does not serve the account contract.
    MissingRoute,
    RateLimited,
    /// The backend is up but failing (5xx).
    Unavailable,
    UnexpectedStatus(u16),
    /// A schema this build does not speak. The reported version is backend text
    /// and stays out of the copy.
    UnsupportedSchema {
        reported: String,
    },
    /// A 200 whose body is not an account snapshot.
    Malformed {
        detail: String,
    },
}

impl AccountError {
    pub fn from_status(status: u16) -> Self {
        match status {
            401 | 403 => Self::Unauthorized,
            404 => Self::MissingRoute,
            429 => Self::RateLimited,
            500..=599 => Self::Unavailable,
            other => Self::UnexpectedStatus(other),
        }
    }

    /// The exact sentence the shell shows. Stable, complete, secret-free.
    pub fn public_message(&self) -> String {
        match self {
            Self::Unreachable { .. } | Self::Unavailable => {
                "Synth Cloud is unavailable right now.".into()
            }
            Self::Unauthorized => {
                "Synth Cloud rejected this device's key. Sign in again to continue.".into()
            }
            Self::MissingRoute => {
                "This Synth backend does not serve the desktop account snapshot yet.".into()
            }
            Self::RateLimited => {
                "Synth Cloud is refreshing accounts too often right now. Try again shortly.".into()
            }
            Self::UnexpectedStatus(status) => {
                format!("Synth Cloud returned an unexpected response ({status}).")
            }
            Self::UnsupportedSchema { .. } => {
                "Synth Cloud sent an account format this version of Synth Desktop cannot read. Update Synth Desktop.".into()
            }
            Self::Malformed { .. } => {
                "Synth Cloud sent an account snapshot Synth Desktop could not read.".into()
            }
        }
    }

    /// Native-only diagnosis. Never rendered and never logged automatically.
    pub fn detail(&self) -> Option<&str> {
        match self {
            Self::Unreachable { detail } | Self::Malformed { detail } => Some(detail),
            Self::UnsupportedSchema { reported } => Some(reported),
            _ => None,
        }
    }

    fn credits_unknown_reason(&self) -> &'static str {
        match self {
            Self::Malformed { .. } | Self::UnsupportedSchema { .. } => "malformed_snapshot",
            _ => "request_failed",
        }
    }

    /// Stable classifier the shell uses to tell auth, outage, and malformed
    /// snapshots apart. Never includes transport or parser text.
    pub fn failure_kind(&self) -> &'static str {
        match self {
            Self::Unauthorized => "auth",
            Self::Malformed { .. } | Self::UnsupportedSchema { .. } => "malformed",
            _ => "outage",
        }
    }
}

impl std::fmt::Display for AccountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.public_message())
    }
}

pub struct AccountCloudClient {
    http: reqwest::Client,
    cache: Mutex<Option<CachedSnapshot>>,
}

impl Default for AccountCloudClient {
    fn default() -> Self {
        Self::open()
    }
}

impl AccountCloudClient {
    pub fn open() -> Self {
        Self {
            http: crate::http::http_client_with_timeout(crate::limits::ACCOUNT_CLOUD_TIMEOUT),
            cache: Mutex::new(None),
        }
    }

    pub fn clear_cache(&self) {
        *self.cache.lock().unwrap() = None;
    }

    fn cached_for(&self, identity: u64) -> Option<CachedSnapshot> {
        self.cache
            .lock()
            .unwrap()
            .clone()
            .filter(|entry| entry.identity == identity)
    }

    /// Read the snapshot, preferring a fresh cache entry. `force` skips the TTL
    /// but still falls back to the cached copy when the network fails.
    pub async fn read(
        &self,
        backend_url: &str,
        api_key: Option<&str>,
        force: bool,
        now: DateTime<Utc>,
    ) -> SnapshotRead {
        let Some(api_key) = api_key else {
            self.clear_cache();
            return SnapshotRead {
                unauthenticated: true,
                ..SnapshotRead::default()
            };
        };
        let identity = connection_identity(backend_url, api_key);
        let cached = self.cached_for(identity);
        if !force {
            if let Some(entry) = cached.as_ref() {
                let age = now.signed_duration_since(entry.fetched_at).num_seconds();
                if age < CACHE_TTL_SECONDS {
                    return SnapshotRead {
                        snapshot: Some(entry.snapshot.clone()),
                        stale: false,
                        error: None,
                        fetched_at: Some(entry.fetched_at),
                        unauthenticated: false,
                        failure_kind: None,
                    };
                }
            }
        }
        match self.fetch(backend_url, api_key).await {
            Ok(snapshot) => {
                *self.cache.lock().unwrap() = Some(CachedSnapshot {
                    snapshot: snapshot.clone(),
                    fetched_at: now,
                    identity,
                });
                SnapshotRead {
                    snapshot: Some(snapshot),
                    stale: false,
                    error: None,
                    fetched_at: Some(now),
                    unauthenticated: false,
                    failure_kind: None,
                }
            }
            Err(error) => {
                // Authentication failures invalidate the identity, not merely
                // the refresh. Serving a cached paid plan after the backend
                // revoked this key would make the shell claim the device is
                // still signed in and could admit more metered work.
                if matches!(error, AccountError::Unauthorized) {
                    self.clear_cache();
                    return SnapshotRead {
                        snapshot: None,
                        stale: false,
                        error: Some(error.public_message()),
                        fetched_at: None,
                        unauthenticated: true,
                        failure_kind: Some(error.failure_kind().into()),
                    };
                }
                if cached.is_none() {
                    self.report_credits_unknown(
                        backend_url,
                        api_key,
                        error.credits_unknown_reason(),
                    )
                    .await;
                }
                SnapshotRead {
                    snapshot: cached.as_ref().map(|entry| entry.snapshot.clone()),
                    stale: cached.is_some(),
                    error: Some(error.public_message()),
                    fetched_at: cached.as_ref().map(|entry| entry.fetched_at),
                    unauthenticated: false,
                    failure_kind: Some(error.failure_kind().into()),
                }
            }
        }
    }

    async fn report_credits_unknown(&self, backend_url: &str, api_key: &str, reason: &str) {
        let result = self
            .http
            .post(format!(
                "{}{}",
                backend_url.trim_end_matches('/'),
                CREDITS_UNKNOWN_PATH
            ))
            .bearer_auth(api_key)
            .json(&serde_json::json!({
                "surface": "desktop_account",
                "reason": reason,
            }))
            .send()
            .await;
        match result {
            Ok(response) if response.status().is_success() => {}
            Ok(response) => crate::platform::logging::report(
                "account_cloud",
                "eprintln",
                format!(
                    "credits_unknown observability report failed with status {}",
                    response.status()
                ),
            ),
            Err(error) => crate::platform::logging::report(
                "account_cloud",
                "eprintln",
                format!("credits_unknown observability report failed: {error}"),
            ),
        }
    }

    async fn fetch(
        &self,
        backend_url: &str,
        api_key: &str,
    ) -> std::result::Result<CloudSnapshot, AccountError> {
        let response = self
            .http
            .get(format!(
                "{}{SNAPSHOT_PATH}",
                backend_url.trim_end_matches('/')
            ))
            .bearer_auth(api_key)
            .send()
            .await
            .map_err(|error| AccountError::Unreachable {
                detail: error.to_string(),
            })?;
        let status = response.status();
        if !status.is_success() {
            return Err(AccountError::from_status(status.as_u16()));
        }
        let snapshot: CloudSnapshot =
            response
                .json()
                .await
                .map_err(|error| AccountError::Malformed {
                    detail: error.to_string(),
                })?;
        if !snapshot.schema_version.is_empty() && snapshot.schema_version != SCHEMA_VERSION {
            return Err(AccountError::UnsupportedSchema {
                reported: snapshot.schema_version,
            });
        }
        Ok(snapshot)
    }

    /// Ask the backend for a hosted billing URL. Desktop opens URLs; it never
    /// renders a payment form. Falls back to the snapshot's own hosted links so
    /// Upgrade still works when the session endpoints are unavailable.
    pub async fn billing_url(
        &self,
        backend_url: &str,
        api_key: Option<&str>,
        action: BillingAction,
        tier: Option<&str>,
    ) -> Result<String> {
        let api_key = api_key.ok_or_else(|| anyhow!("sign in to manage Synth Cloud billing"))?;
        let base = backend_url.trim_end_matches('/');
        let request = match action {
            BillingAction::Upgrade => {
                let plan = tier
                    .map(str::to_owned)
                    .or_else(|| {
                        self.cached_for(connection_identity(backend_url, api_key))
                            .and_then(|entry| entry.snapshot.billing_actions.upgrade_tier.clone())
                    })
                    .ok_or_else(|| {
                        anyhow!("no upgrade is available for this plan; use Manage billing")
                    })?;
                self.http
                    .post(format!("{base}{CHECKOUT_PATH}"))
                    .bearer_auth(api_key)
                    .json(&serde_json::json!({ "plan": plan }))
            }
            BillingAction::Manage => self
                .http
                .post(format!("{base}{PORTAL_PATH}"))
                .bearer_auth(api_key),
        };
        // The backend issues the hosted URL. Substituting a cached one when it
        // refuses would open a checkout the server just declined to authorize,
        // so every failure is reported as itself.
        let response = request.send().await.map_err(|error| {
            anyhow!(AccountError::Unreachable {
                detail: error.to_string(),
            }
            .public_message())
        })?;
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!(
                AccountError::from_status(status.as_u16()).public_message()
            ));
        }
        let session: HostedBillingSession = response.json().await.map_err(|error| {
            anyhow!(AccountError::Malformed {
                detail: error.to_string(),
            }
            .public_message())
        })?;
        if matches!(action, BillingAction::Upgrade) {
            let mode = session.mode.as_deref().unwrap_or("");
            if mode != "provider" {
                return Err(anyhow!(
                    "Synth Cloud could not open provider checkout for this upgrade. Try again in a moment, or manage billing on the web."
                ));
            }
            if session.url.trim().is_empty() {
                return Err(anyhow!(
                    "Synth Cloud returned an empty checkout URL for this upgrade."
                ));
            }
            if session
                .session_id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .is_none()
            {
                return Err(anyhow!(
                    "Synth Cloud returned a checkout URL without a session id."
                ));
            }
        }
        Ok(session.url)
    }
}

