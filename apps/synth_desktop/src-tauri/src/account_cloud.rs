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
use std::time::Duration;

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
    #[specta(type = specta_typescript::Unknown)]
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
    #[specta(type = specta_typescript::Unknown)]
    pub limit_cents: Option<i64>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub used_cents: Option<i64>,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub remaining_cents: Option<i64>,
    #[serde(default)]
    pub resets_at: Option<String>,
    #[serde(default)]
    pub source: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, specta::Type)]
pub struct CloudUsageWindow {
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub events: i64,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub billed_cents: i64,
    #[serde(default)]
    #[specta(type = specta_typescript::Unknown)]
    pub nominal_cents: i64,
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
    #[specta(type = specta_typescript::Unknown)]
    pub price_cents: i64,
    #[specta(type = specta_typescript::Unknown)]
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
                }
            }
            Err(error) => {
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
            Ok(response) => eprintln!(
                "credits_unknown observability report failed with status {}",
                response.status()
            ),
            Err(error) => eprintln!("credits_unknown observability report failed: {error}"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// A fake Synth backend that answers a scripted sequence and records the
    /// requests it saw.
    fn spawn_backend(responses: Vec<(u16, String)>) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let served = Arc::new(AtomicUsize::new(0));
        let counter = served.clone();
        std::thread::spawn(move || {
            for (status, body) in responses {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let mut buf = [0u8; 8192];
                let _ = stream.read(&mut buf);
                counter.fetch_add(1, Ordering::SeqCst);
                let payload = format!(
                    "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(payload.as_bytes());
            }
        });
        (origin, served)
    }

    fn snapshot_body(tier: &str, remaining_cents: i64) -> String {
        format!(
            r#"{{
              "schema_version": "synth.desktop-account.v1",
              "status": "active",
              "account": {{"id": "acct_1", "display_name": "ada", "email": "ada@example.com"}},
              "organization": {{"id": "org_1", "display_name": "Ada Labs", "role": "owner"}},
              "plan": {{"tier": "{tier}", "display_name": "Pro", "state": "active", "price_cents": 20000, "renews_at": "2026-09-01T00:00:00+00:00", "is_paid": true}},
              "allowance": {{"limit_cents": 20000, "used_cents": {used}, "remaining_cents": {remaining_cents}, "resets_at": "2026-09-01T00:00:00+00:00", "source": "entitlement"}},
              "usage": {{"today": {{"events": 2, "billed_cents": 15, "nominal_cents": 15}}, "seven_days": {{"events": 9, "billed_cents": 120, "nominal_cents": 120}}, "thirty_days": {{"events": 40, "billed_cents": 1300, "nominal_cents": 1300}}}},
              "billing_actions": {{"checkout_url": "https://example.test/usage?upgrade=pro", "portal_url": "https://example.test/usage", "upgrade_tier": "pro"}},
              "catalog": [{{"tier": "pro", "display_name": "Pro", "price_cents": 20000, "monthly_allowance_cents": 20000, "interval": "month"}}],
              "generated_at": "2026-08-10T12:00:00+00:00",
              "degraded": []
            }}"#,
            used = 20000 - remaining_cents
        )
    }

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-08-10T12:00:00+00:00")
            .unwrap()
            .with_timezone(&Utc)
    }

    #[tokio::test]
    async fn no_key_reads_as_unauthenticated_without_touching_the_network() {
        let client = AccountCloudClient::open();
        let read = client.read("http://127.0.0.1:1", None, false, now()).await;
        assert!(read.unauthenticated);
        assert!(read.snapshot.is_none());
        assert!(read.error.is_none());
    }

    #[tokio::test]
    async fn a_fresh_snapshot_is_parsed_and_then_served_from_cache() {
        let (origin, served) = spawn_backend(vec![(200, snapshot_body("pro", 15_750))]);
        let client = AccountCloudClient::open();
        let first = client.read(&origin, Some("sk_test"), false, now()).await;
        let snapshot = first.snapshot.expect("snapshot parsed");
        assert_eq!(snapshot.plan.tier, "pro");
        assert_eq!(snapshot.allowance.remaining_cents, Some(15_750));
        assert_eq!(snapshot.usage.seven_days.billed_cents, 120);
        assert_eq!(snapshot.account.email.as_deref(), Some("ada@example.com"));
        assert!(!first.stale);

        // Within the TTL the menu re-reads for free: no second request is made
        // (the fake backend would have nothing to answer with).
        let second = client
            .read(
                &origin,
                Some("sk_test"),
                false,
                now() + chrono::Duration::seconds(30),
            )
            .await;
        assert_eq!(second.snapshot.map(|s| s.plan.tier), Some("pro".into()));
        assert!(!second.stale);
        assert_eq!(served.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn an_expired_cache_refetches_and_a_failure_serves_the_last_good_copy_as_stale() {
        let (origin, served) = spawn_backend(vec![
            (200, snapshot_body("pro", 20_000)),
            (503, r#"{"detail":"down"}"#.into()),
        ]);
        let client = AccountCloudClient::open();
        client.read(&origin, Some("sk_test"), false, now()).await;
        let stale = client
            .read(
                &origin,
                Some("sk_test"),
                false,
                now() + chrono::Duration::seconds(CACHE_TTL_SECONDS + 1),
            )
            .await;
        assert!(
            stale.stale,
            "a failed refresh must keep showing the last plan"
        );
        assert_eq!(stale.snapshot.map(|s| s.plan.tier), Some("pro".into()));
        assert_eq!(
            stale.error.as_deref(),
            Some("Synth Cloud is unavailable right now.")
        );
        assert_eq!(served.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn a_different_key_never_reads_another_accounts_cache() {
        let (origin, _) = spawn_backend(vec![(200, snapshot_body("pro", 20_000))]);
        let client = AccountCloudClient::open();
        client.read(&origin, Some("sk_one"), false, now()).await;
        // The second key has no cache entry and the fake backend is exhausted,
        // so the read must fail rather than return the first account's plan.
        let other = client.read(&origin, Some("sk_two"), false, now()).await;
        assert!(other.snapshot.is_none());
        assert!(!other.stale);
        assert!(other.error.is_some());
    }

    #[tokio::test]
    async fn an_unauthorized_snapshot_explains_itself_without_leaking_the_key() {
        let (origin, _) = spawn_backend(vec![(401, r#"{"detail":"bad key"}"#.into())]);
        let client = AccountCloudClient::open();
        let read = client.read(&origin, Some("sk_dead"), false, now()).await;
        let error = read.error.expect("error surfaced");
        assert_eq!(
            error,
            "Synth Cloud rejected this device's key. Sign in again to continue."
        );
        assert!(!error.contains("sk_dead"));
    }

    #[tokio::test]
    async fn a_schema_the_desktop_does_not_speak_is_refused() {
        let body = snapshot_body("pro", 20_000)
            .replace("synth.desktop-account.v1", "synth.desktop-account.v2");
        let (origin, _) = spawn_backend(vec![(200, body)]);
        let client = AccountCloudClient::open();
        let read = client.read(&origin, Some("sk_test"), false, now()).await;
        assert!(read.snapshot.is_none());
        assert_eq!(
            read.error.as_deref(),
            Some(
                "Synth Cloud sent an account format this version of Synth Desktop cannot read. Update Synth Desktop."
            )
        );
    }

    #[tokio::test]
    async fn a_different_backend_never_reads_another_origins_cache_for_the_same_key() {
        let (first_origin, _) = spawn_backend(vec![(200, snapshot_body("pro", 20_000))]);
        let (second_origin, _) = spawn_backend(vec![(503, r#"{"detail":"down"}"#.into())]);
        let client = AccountCloudClient::open();
        client
            .read(&first_origin, Some("sk_same"), false, now())
            .await;

        let other = client
            .read(&second_origin, Some("sk_same"), false, now())
            .await;
        assert!(other.snapshot.is_none());
        assert!(!other.stale);
        assert_eq!(
            other.error.as_deref(),
            Some("Synth Cloud is unavailable right now.")
        );
    }

    #[tokio::test]
    async fn a_backend_without_the_snapshot_endpoint_gets_the_specific_update_copy() {
        let (origin, _) = spawn_backend(vec![(404, r#"{"detail":"missing"}"#.into())]);
        let client = AccountCloudClient::open();
        let read = client.read(&origin, Some("sk_test"), false, now()).await;
        assert!(read.snapshot.is_none());
        assert_eq!(
            read.error.as_deref(),
            Some("This Synth backend does not serve the desktop account snapshot yet.")
        );
    }

    /// The outage the QA run hit: the local proxy was stopped mid-session. The
    /// user must get a complete sentence, not an anyhow context fragment.
    #[tokio::test]
    async fn a_refused_connection_reads_as_one_complete_public_sentence() {
        // Bind then drop, so the port is closed but routable.
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", closed.local_addr().unwrap());
        drop(closed);
        let client = AccountCloudClient::open();
        let read = client.read(&origin, Some("sk_test"), false, now()).await;
        assert_eq!(
            read.error.as_deref(),
            Some("Synth Cloud is unavailable right now.")
        );
        assert!(!read.stale, "there is no cached plan to fall back to yet");
    }

    /// The same outage after a good read: the plan stays on screen, marked stale,
    /// and the note reads exactly as the product contract specifies.
    #[tokio::test]
    async fn an_outage_after_a_good_read_keeps_the_plan_and_marks_it_stale() {
        let (origin, _) = spawn_backend(vec![(200, snapshot_body("pro", 15_000))]);
        let client = AccountCloudClient::open();
        client.read(&origin, Some("sk_test"), false, now()).await;
        // The scripted backend is exhausted and its listener is closed, so the
        // refresh fails at the transport layer.
        let read = client
            .read(
                &origin,
                Some("sk_test"),
                true,
                now() + chrono::Duration::seconds(CACHE_TTL_SECONDS + 1),
            )
            .await;
        assert!(read.stale);
        assert_eq!(read.snapshot.map(|s| s.plan.tier), Some("pro".into()));
        let note = format!("Showing the last known plan — {}", read.error.unwrap());
        assert_eq!(
            note,
            "Showing the last known plan — Synth Cloud is unavailable right now."
        );
    }

    #[tokio::test]
    async fn a_timeout_reads_as_the_same_public_sentence_as_a_refusal() {
        // A listener that accepts and then never answers.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let mut held = Vec::new();
            while let Ok((stream, _)) = listener.accept() {
                held.push(stream);
            }
        });
        let client = AccountCloudClient {
            http: crate::http::http_client_with_timeout(Duration::from_millis(150)),
            cache: Mutex::new(None),
        };
        let read = client.read(&origin, Some("sk_test"), false, now()).await;
        assert_eq!(
            read.error.as_deref(),
            Some("Synth Cloud is unavailable right now.")
        );
    }

    #[tokio::test]
    async fn a_two_hundred_that_is_not_a_snapshot_is_refused_with_stable_copy() {
        let (origin, _) = spawn_backend(vec![(200, r#"{"unexpected": true}"#.into())]);
        let client = AccountCloudClient::open();
        let read = client.read(&origin, Some("sk_test"), false, now()).await;
        assert!(read.snapshot.is_none());
        assert_eq!(
            read.error.as_deref(),
            Some("Synth Cloud sent an account snapshot Synth Desktop could not read.")
        );
    }

    /// Public copy is a closed set of complete sentences. Backend- and
    /// transport-controlled text must never reach it.
    #[test]
    fn every_public_message_is_a_complete_secret_free_sentence() {
        let cases = [
            AccountError::Unreachable {
                detail: "error sending request for url (http://127.0.0.1:1/x): connection refused"
                    .into(),
            },
            AccountError::Unauthorized,
            AccountError::MissingRoute,
            AccountError::RateLimited,
            AccountError::Unavailable,
            AccountError::UnexpectedStatus(418),
            AccountError::UnsupportedSchema {
                reported: "synth.desktop-account.v2".into(),
            },
            AccountError::Malformed {
                detail: "expected value at line 1 column 1".into(),
            },
        ];
        for case in cases {
            let message = case.public_message();
            assert!(message.ends_with('.'), "not a complete sentence: {message}");
            assert!(
                message.starts_with(|c: char| c.is_ascii_uppercase()),
                "not a complete sentence: {message}"
            );
            if let Some(detail) = case.detail() {
                assert!(
                    !message.contains(detail),
                    "native diagnostics leaked into public copy: {message}"
                );
            }
            assert_eq!(message, case.to_string());
        }
    }

    #[tokio::test]
    async fn a_refused_checkout_session_is_reported_not_papered_over_with_a_cached_url() {
        // The backend owns authorization for a purchase. Substituting the URL
        // from an earlier snapshot would open a checkout the server just
        // declined, so the refusal has to reach the caller.
        let (origin, _) = spawn_backend(vec![
            (200, snapshot_body("starter", 2_000)),
            (500, r#"{"detail":"no provider"}"#.into()),
        ]);
        let client = AccountCloudClient::open();
        client.read(&origin, Some("sk_test"), false, now()).await;
        let error = client
            .billing_url(&origin, Some("sk_test"), BillingAction::Upgrade, None)
            .await
            .expect_err("a 500 from the session endpoint must not yield a URL")
            .to_string();
        assert_eq!(error, "Synth Cloud is unavailable right now.");
    }

    #[tokio::test]
    async fn an_unreadable_billing_session_is_reported_rather_than_replaced() {
        let (origin, _) = spawn_backend(vec![
            (200, snapshot_body("starter", 2_000)),
            (200, r#"{"not_a_url": true}"#.into()),
        ]);
        let client = AccountCloudClient::open();
        client.read(&origin, Some("sk_test"), false, now()).await;
        let error = client
            .billing_url(&origin, Some("sk_test"), BillingAction::Manage, None)
            .await
            .expect_err("an unparseable session must not fall back to a cached link")
            .to_string();
        assert_eq!(
            error,
            "Synth Cloud sent an account snapshot Synth Desktop could not read."
        );
    }

    #[tokio::test]
    async fn billing_prefers_the_backend_issued_session_url() {
        let (origin, _) = spawn_backend(vec![
            (200, snapshot_body("starter", 2_000)),
            (
                200,
                r#"{"url":"https://checkout.test/session/abc","mode":"provider"}"#.into(),
            ),
        ]);
        let client = AccountCloudClient::open();
        client.read(&origin, Some("sk_test"), false, now()).await;
        let url = client
            .billing_url(&origin, Some("sk_test"), BillingAction::Manage, None)
            .await
            .unwrap();
        assert_eq!(url, "https://checkout.test/session/abc");
    }

    #[tokio::test]
    async fn upgrade_refuses_hosted_web_fallback() {
        let (origin, _) = spawn_backend(vec![
            (200, snapshot_body("free", 0)),
            (
                200,
                r#"{"url":"https://app.test/usage?upgrade=starter","mode":"hosted_web"}"#.into(),
            ),
        ]);
        let client = AccountCloudClient::open();
        client.read(&origin, Some("sk_test"), false, now()).await;
        let error = client
            .billing_url(
                &origin,
                Some("sk_test"),
                BillingAction::Upgrade,
                Some("starter"),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("provider checkout"),
            "unexpected copy: {error}"
        );
    }

    #[tokio::test]
    async fn upgrade_accepts_provider_session_with_id() {
        let (origin, _) = spawn_backend(vec![
            (200, snapshot_body("free", 0)),
            (
                200,
                r#"{"url":"https://checkout.test/session/abc","mode":"provider","session_id":"cs_test_1"}"#.into(),
            ),
        ]);
        let client = AccountCloudClient::open();
        client.read(&origin, Some("sk_test"), false, now()).await;
        let url = client
            .billing_url(
                &origin,
                Some("sk_test"),
                BillingAction::Upgrade,
                Some("starter"),
            )
            .await
            .unwrap();
        assert_eq!(url, "https://checkout.test/session/abc");
    }

    #[tokio::test]
    async fn upgrade_rejects_provider_session_without_id() {
        let (origin, _) = spawn_backend(vec![
            (200, snapshot_body("free", 0)),
            (
                200,
                r#"{"url":"https://checkout.test/session/abc","mode":"provider"}"#.into(),
            ),
        ]);
        let client = AccountCloudClient::open();
        client.read(&origin, Some("sk_test"), false, now()).await;
        let error = client
            .billing_url(
                &origin,
                Some("sk_test"),
                BillingAction::Upgrade,
                Some("starter"),
            )
            .await
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("without a session id"),
            "unexpected copy: {error}"
        );
    }

    #[tokio::test]
    async fn billing_requires_a_signed_in_device() {
        let client = AccountCloudClient::open();
        let error = client
            .billing_url("http://127.0.0.1:1", None, BillingAction::Manage, None)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("sign in"));
    }
}
