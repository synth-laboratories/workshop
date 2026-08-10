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

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const SNAPSHOT_PATH: &str = "/api/v1/desktop/account-snapshot";
const CHECKOUT_PATH: &str = "/api/v1/billing/checkout-session";
const PORTAL_PATH: &str = "/api/v1/billing/portal-session";
/// Long enough that opening the menu repeatedly costs nothing, short enough
/// that a checkout completed in the browser shows up on the next open.
const CACHE_TTL_SECONDS: i64 = 60;
pub const SCHEMA_VERSION: &str = "synth.desktop-account.v1";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CloudAccount {
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub avatar_url: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CloudOrganization {
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CloudPlan {
    pub tier: String,
    pub display_name: String,
    pub state: String,
    #[serde(default)]
    pub price_cents: i64,
    #[serde(default)]
    pub renews_at: Option<String>,
    #[serde(default)]
    pub is_paid: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CloudAllowance {
    /// `None` means the backend does not meter this account in dollars. The UI
    /// must then show no dollar figure at all.
    #[serde(default)]
    pub limit_cents: Option<i64>,
    #[serde(default)]
    pub used_cents: i64,
    #[serde(default)]
    pub remaining_cents: Option<i64>,
    #[serde(default)]
    pub resets_at: Option<String>,
    #[serde(default)]
    pub source: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct CloudUsageWindow {
    #[serde(default)]
    pub events: i64,
    #[serde(default)]
    pub billed_cents: i64,
    #[serde(default)]
    pub nominal_cents: i64,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct CloudUsageWindows {
    #[serde(default)]
    pub today: CloudUsageWindow,
    #[serde(default)]
    pub seven_days: CloudUsageWindow,
    #[serde(default)]
    pub thirty_days: CloudUsageWindow,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
pub struct CloudBillingActions {
    #[serde(default)]
    pub checkout_url: Option<String>,
    #[serde(default)]
    pub portal_url: Option<String>,
    #[serde(default)]
    pub upgrade_tier: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CloudPlanOption {
    pub tier: String,
    pub display_name: String,
    pub price_cents: i64,
    pub monthly_allowance_cents: i64,
    #[serde(default)]
    pub interval: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
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

#[derive(Clone, Copy, Debug, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum BillingAction {
    Upgrade,
    Manage,
}

#[derive(Deserialize)]
struct HostedBillingSession {
    url: String,
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

/// HTTP failures reach the user; keep them short and free of secrets.
fn describe_status(status: u16) -> String {
    match status {
        401 | 403 => "Synth Cloud rejected this device's key; sign in again".into(),
        404 => "This Synth backend does not serve the desktop account snapshot yet".into(),
        429 => "Synth Cloud is rate limiting account refreshes; try again shortly".into(),
        500..=599 => "Synth Cloud is unavailable right now".into(),
        other => format!("Synth Cloud returned an unexpected status ({other})"),
    }
}

pub struct AccountCloudClient {
    http: reqwest::Client,
    cache: Mutex<Option<CachedSnapshot>>,
}

impl Default for AccountCloudClient {
    fn default() -> Self {
        Self::new()
    }
}

impl AccountCloudClient {
    pub fn new() -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(12))
                .build()
                .expect("account-snapshot HTTP client"),
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
            Err(error) => SnapshotRead {
                snapshot: cached.as_ref().map(|entry| entry.snapshot.clone()),
                stale: cached.is_some(),
                error: Some(error.to_string()),
                fetched_at: cached.as_ref().map(|entry| entry.fetched_at),
                unauthenticated: false,
            },
        }
    }

    async fn fetch(&self, backend_url: &str, api_key: &str) -> Result<CloudSnapshot> {
        let response = self
            .http
            .get(format!(
                "{}{SNAPSHOT_PATH}",
                backend_url.trim_end_matches('/')
            ))
            .bearer_auth(api_key)
            .send()
            .await
            .context("reach Synth Cloud")?;
        let status = response.status();
        if !status.is_success() {
            return Err(anyhow!(describe_status(status.as_u16())));
        }
        let snapshot: CloudSnapshot = response
            .json()
            .await
            .context("read the Synth Cloud account snapshot")?;
        if !snapshot.schema_version.is_empty() && snapshot.schema_version != SCHEMA_VERSION {
            return Err(anyhow!(
                "Synth Cloud sent account schema {} but this desktop speaks {SCHEMA_VERSION}; update Synth Desktop",
                snapshot.schema_version
            ));
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
        let fallback = self.fallback_billing_url(backend_url, api_key, action);
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
        match request.send().await {
            Ok(response) if response.status().is_success() => {
                match response.json::<HostedBillingSession>().await {
                    Ok(session) => Ok(session.url),
                    Err(_) => fallback
                        .ok_or_else(|| anyhow!("Synth Cloud returned an unreadable billing link")),
                }
            }
            Ok(response) => {
                fallback.ok_or_else(|| anyhow!(describe_status(response.status().as_u16())))
            }
            Err(_) => fallback.ok_or_else(|| anyhow!("could not reach Synth Cloud billing")),
        }
    }

    fn fallback_billing_url(
        &self,
        backend_url: &str,
        api_key: &str,
        action: BillingAction,
    ) -> Option<String> {
        let actions = self
            .cached_for(connection_identity(backend_url, api_key))?
            .snapshot
            .billing_actions;
        match action {
            BillingAction::Upgrade => actions.checkout_url.or(actions.portal_url),
            BillingAction::Manage => actions.portal_url,
        }
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
        let client = AccountCloudClient::new();
        let read = client.read("http://127.0.0.1:1", None, false, now()).await;
        assert!(read.unauthenticated);
        assert!(read.snapshot.is_none());
        assert!(read.error.is_none());
    }

    #[tokio::test]
    async fn a_fresh_snapshot_is_parsed_and_then_served_from_cache() {
        let (origin, served) = spawn_backend(vec![(200, snapshot_body("pro", 15_750))]);
        let client = AccountCloudClient::new();
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
        let client = AccountCloudClient::new();
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
        assert!(stale.error.unwrap().contains("unavailable"));
        assert_eq!(served.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn a_different_key_never_reads_another_accounts_cache() {
        let (origin, _) = spawn_backend(vec![(200, snapshot_body("pro", 20_000))]);
        let client = AccountCloudClient::new();
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
        let client = AccountCloudClient::new();
        let read = client.read(&origin, Some("sk_dead"), false, now()).await;
        let error = read.error.expect("error surfaced");
        assert!(error.contains("sign in again"));
        assert!(!error.contains("sk_dead"));
    }

    #[tokio::test]
    async fn a_schema_the_desktop_does_not_speak_is_refused() {
        let body = snapshot_body("pro", 20_000)
            .replace("synth.desktop-account.v1", "synth.desktop-account.v2");
        let (origin, _) = spawn_backend(vec![(200, body)]);
        let client = AccountCloudClient::new();
        let read = client.read(&origin, Some("sk_test"), false, now()).await;
        assert!(read.snapshot.is_none());
        assert!(read.error.unwrap().contains("update Synth Desktop"));
    }

    #[tokio::test]
    async fn billing_falls_back_to_the_snapshot_url_when_the_session_endpoint_fails() {
        let (origin, _) = spawn_backend(vec![
            (200, snapshot_body("starter", 2_000)),
            (500, r#"{"detail":"no provider"}"#.into()),
        ]);
        let client = AccountCloudClient::new();
        client.read(&origin, Some("sk_test"), false, now()).await;
        let url = client
            .billing_url(&origin, Some("sk_test"), BillingAction::Upgrade, None)
            .await
            .unwrap();
        assert_eq!(url, "https://example.test/usage?upgrade=pro");
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
        let client = AccountCloudClient::new();
        client.read(&origin, Some("sk_test"), false, now()).await;
        let url = client
            .billing_url(&origin, Some("sk_test"), BillingAction::Manage, None)
            .await
            .unwrap();
        assert_eq!(url, "https://checkout.test/session/abc");
    }

    #[tokio::test]
    async fn billing_requires_a_signed_in_device() {
        let client = AccountCloudClient::new();
        let error = client
            .billing_url("http://127.0.0.1:1", None, BillingAction::Manage, None)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("sign in"));
    }
}
