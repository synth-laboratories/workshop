//! Authoritative account summary for the desktop shell.
//!
//! Identity and plan truth live on the Rust side: the renderer only renders
//! what `account_get_summary` returns. When the device is paired, the Synth
//! Cloud Account Snapshot is that truth — plan, allowance, usage, and the
//! hosted billing links all come from the backend.
//!
//! Two rules hold everywhere below:
//!
//! * every cloud profile — `prod`, `staging`, `production`, or any other
//!   name that isn't an explicit local-only lane — never invents dollars. If
//!   the snapshot is missing or the backend does not meter this account in
//!   dollars, the shell shows no plan figure rather than a plausible one;
//! * only the explicit local-only profiles (`local`, `local-slot1`) keep a
//!   seeded `Synth Dev` $200 stand-in — charged from the device
//!   `usage_records` ledger — so the shell is exercisable offline, and it is
//!   always labelled as a dev stand-in, never as cloud truth. Gating is on
//!   the *profile* (`is_local_only_profile`), not on the origin heuristic
//!   used for the display-only `environment` field: a stand-in seeded while
//!   on a local profile must not leak into a later summary read under a
//!   cloud profile.

use anyhow::Result;
use chrono::{DateTime, Datelike, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::account_cloud::{CloudSnapshot, SnapshotRead};
use crate::storage::Storage;

const PLAN_SETTINGS_KEY: &str = "account.dev_plan.v1";
const PAIRED_ONCE_KEY: &str = "account.paired_once.v1";
const DEV_MONTHLY_ALLOWANCE_CENTS: i64 = 20_000;
const DEV_PLAN_NAME: &str = "Synth Dev";
const DEV_DISPLAY_NAME: &str = "Synth Dev";
const DEV_ACCOUNT_ID: &str = "dev-local";

/// Shell states from `AUTH_BILLING_FLOW.md`. `pairing` is renderer-owned (it
/// knows a sign-in is in flight); everything else is decided here.
pub const STATE_LOCAL_ONLY: &str = "local_only";
pub const STATE_SIGNED_OUT: &str = "signed_out";
pub const STATE_ACTIVE: &str = "active";
pub const STATE_LIMITED: &str = "limited";
pub const STATE_PAST_DUE: &str = "past_due";
pub const STATE_CANCELED: &str = "canceled";
pub const STATE_ERROR: &str = "error";
pub const STATE_UNKNOWN: &str = "unknown";

pub const SESSION_LOCAL_ONLY: &str = "local_only";
pub const SESSION_SIGNED_OUT: &str = "signed_out";
pub const SESSION_ACTIVE: &str = "active";
pub const SESSION_REVOKED: &str = "revoked";
pub const SESSION_OFFLINE: &str = "offline";
pub const SESSION_MALFORMED: &str = "malformed";

pub const FAILURE_NONE: &str = "none";
pub const FAILURE_AUTH: &str = "auth";
pub const FAILURE_ENTITLEMENT: &str = "entitlement";
pub const FAILURE_QUOTA: &str = "quota";
pub const FAILURE_OUTAGE: &str = "outage";
pub const FAILURE_MALFORMED: &str = "malformed";

pub const RECONCILIATION_OK: &str = "ok";
pub const RECONCILIATION_STALE: &str = "stale";
pub const RECONCILIATION_FAILED: &str = "failed";

pub const SOURCE_CLOUD: &str = "cloud";
pub const SOURCE_DEV_SEED: &str = "dev_seed";
pub const SOURCE_NONE: &str = "none";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountPlan {
    pub name: String,
    #[serde(default)]
    pub tier: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    /// False when the backend reports no dollar limit for this account: the UI
    /// must then omit allowance figures instead of showing zeros.
    pub metered: bool,
    #[serde(default)]
    pub monthly_allowance_usd: Option<f64>,
    pub used_usd: Option<f64>,
    #[serde(default)]
    pub remaining_usd: Option<f64>,
    #[serde(default)]
    pub resets_at: Option<String>,
    #[serde(default)]
    pub renews_at: Option<String>,
    /// `cloud` or `dev_seed`; the UI labels the stand-in explicitly.
    pub source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountOrganization {
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountUsageWindow {
    #[specta(type = specta_typescript::Number)]
    pub events: i64,
    /// Finalized billed dollars. Never the sum of pending + billed.
    pub cost_usd: f64,
    pub finalized_usd: f64,
    /// Nominal minus billed for this window. Live estimates, not ledger truth.
    pub pending_usd: f64,
    #[specta(type = specta_typescript::Number)]
    pub tokens: i64,
    #[specta(type = specta_typescript::Number)]
    pub runtime_seconds: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountCloudUsage {
    pub today: AccountUsageWindow,
    pub seven_days: AccountUsageWindow,
    pub thirty_days: AccountUsageWindow,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountBilling {
    #[serde(default)]
    pub checkout_url: Option<String>,
    #[serde(default)]
    pub portal_url: Option<String>,
    #[serde(default)]
    pub upgrade_tier: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountPlanOption {
    pub tier: String,
    pub display_name: String,
    pub price_usd: f64,
    pub monthly_allowance_usd: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    pub signed_in: bool,
    pub state: String,
    pub environment: String,
    /// Where the rendered plan came from, so the UI can label a dev stand-in.
    pub source: String,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub organization: Option<AccountOrganization>,
    #[serde(default)]
    pub plan: Option<AccountPlan>,
    #[serde(default)]
    pub cloud_usage: Option<AccountCloudUsage>,
    pub billing: AccountBilling,
    pub catalog: Vec<AccountPlanOption>,
    /// When the rendered cloud facts were fetched, for a `Last updated` line.
    #[serde(default)]
    pub last_updated: Option<String>,
    pub stale: bool,
    #[serde(default)]
    pub error: Option<String>,
    /// `local_only` | `signed_out` | `active` | `revoked` | `offline` | `malformed`
    pub session_health: String,
    /// `none` | `auth` | `entitlement` | `quota` | `outage` | `malformed`
    pub failure_kind: String,
    pub quota_exhausted: bool,
    /// `ok` | `stale` | `failed` — hosted usage vs the last successful snapshot.
    pub reconciliation: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StoredPlan {
    name: String,
    monthly_allowance_cents: i64,
    seeded_at: String,
}

pub fn environment_from_origin(origin: &str) -> &'static str {
    if origin.contains("127.0.0.1") || origin.contains("localhost") {
        "local"
    } else if origin.contains("usesynth.ai") {
        "prod"
    } else {
        "dev"
    }
}

/// The only profiles allowed to seed the `Synth Dev` $200 stand-in. Every
/// other profile name — `staging`, `production`, `prod`, or anything else —
/// is a cloud lane and must show an error state instead of inventing
/// dollars when the signed-in snapshot is missing.
///
/// This is intentionally an allow-list, not a deny-list: a new profile name
/// added later defaults to "never seed" until explicitly added here.
const LOCAL_ONLY_PROFILES: &[&str] = &["local", "local-slot1"];

fn is_local_only_profile(profile: &str) -> bool {
    LOCAL_ONLY_PROFILES.contains(&profile.trim().to_ascii_lowercase().as_str())
}

/// First instant of the month after `now`, in UTC. UTC keeps the boundary
/// timezone-safe: it never jumps backwards for the user's local offset.
pub fn next_monthly_reset(now: DateTime<Utc>) -> DateTime<Utc> {
    let (year, month) = if now.month() == 12 {
        (now.year() + 1, 1)
    } else {
        (now.year(), now.month() + 1)
    };
    Utc.with_ymd_and_hms(year, month, 1, 0, 0, 0).unwrap()
}

fn month_start(now: DateTime<Utc>) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .unwrap()
}

fn usd(cents: i64) -> f64 {
    cents as f64 / 100.0
}

fn used_cents_since(storage: &Storage, since: DateTime<Utc>) -> Result<i64> {
    let floor = since.to_rfc3339();
    // One ledger: count authoritative settled charges, or a Backend-owned
    // Synth Cloud estimate when settlement has not arrived yet. Legacy local
    // tariff estimates are deliberately excluded.
    let used_usd: f64 = storage.database().with_conn(|conn| {
        let mut statement = conn.prepare(
            "SELECT COALESCE(SUM(CASE
                         WHEN cost_source IN ('provider_reported', 'synth_cloud')
                              AND billed_cost_usd IS NOT NULL THEN billed_cost_usd
                         WHEN cost_source = 'synth_cloud' THEN estimated_cost_usd
                         ELSE NULL
                     END), 0)
             FROM usage_records
             WHERE created_at >= ?1",
        )?;
        let value: f64 = statement.query_row([&floor], |row| row.get(0))?;
        Ok(value)
    })?;
    Ok((used_usd * 100.0).round() as i64)
}

fn read_setting(storage: &Storage, key: &str) -> Result<Option<String>> {
    storage.database().with_conn(|conn| {
        let mut statement =
            conn.prepare("SELECT value_json FROM runtime_settings WHERE key = ?1")?;
        let mut rows = statement.query([key])?;
        Ok(match rows.next()? {
            Some(row) => Some(row.get::<_, String>(0)?),
            None => None,
        })
    })
}

fn write_setting_once(storage: &Storage, key: &str, value: &str, at: &str) -> Result<()> {
    storage.database().with_conn(|conn| {
        conn.execute(
            "INSERT INTO runtime_settings(key, value_json, updated_at) VALUES(?1, ?2, ?3)
             ON CONFLICT(key) DO NOTHING",
            [key, value, at],
        )?;
        Ok(())
    })
}

/// Record that this device has completed pairing at least once, so a later
/// signed-out read says `Signed out` instead of pretending it is a fresh
/// local-only install.
pub fn mark_paired(storage: &Storage, now: DateTime<Utc>) -> Result<()> {
    let stamp = now.to_rfc3339();
    write_setting_once(storage, PAIRED_ONCE_KEY, &format!("\"{stamp}\""), &stamp)
}

fn has_paired_before(storage: &Storage) -> bool {
    read_setting(storage, PAIRED_ONCE_KEY)
        .ok()
        .flatten()
        .is_some()
}

/// Load the seeded dev plan, seeding it on first read. Only ever seeds — or
/// returns a previously seeded value — for an explicit local-only profile;
/// every cloud profile gets `None` even if a stand-in was seeded earlier
/// under a different (local) profile on this same device.
fn load_or_seed_plan(
    storage: &Storage,
    profile: &str,
    now: DateTime<Utc>,
) -> Result<Option<StoredPlan>> {
    if !is_local_only_profile(profile) {
        return Ok(None);
    }
    if let Some(raw) = read_setting(storage, PLAN_SETTINGS_KEY)? {
        return Ok(serde_json::from_str(&raw).ok());
    }
    let seeded = StoredPlan {
        name: DEV_PLAN_NAME.into(),
        monthly_allowance_cents: DEV_MONTHLY_ALLOWANCE_CENTS,
        seeded_at: now.to_rfc3339(),
    };
    let payload = serde_json::to_string(&seeded)?;
    write_setting_once(storage, PLAN_SETTINGS_KEY, &payload, &seeded.seeded_at)?;
    Ok(Some(seeded))
}

fn dev_seed_plan(
    storage: &Storage,
    profile: &str,
    now: DateTime<Utc>,
) -> Result<Option<AccountPlan>> {
    let Some(stored) = load_or_seed_plan(storage, profile, now)? else {
        return Ok(None);
    };
    let used_cents = used_cents_since(storage, month_start(now))?;
    let remaining_cents = (stored.monthly_allowance_cents - used_cents).max(0);
    Ok(Some(AccountPlan {
        name: stored.name,
        tier: Some("dev".into()),
        state: Some("active".into()),
        metered: true,
        monthly_allowance_usd: Some(usd(stored.monthly_allowance_cents)),
        used_usd: Some(usd(used_cents)),
        remaining_usd: Some(usd(remaining_cents)),
        resets_at: Some(next_monthly_reset(now).to_rfc3339()),
        renews_at: None,
        source: SOURCE_DEV_SEED.into(),
    }))
}

fn plan_from_snapshot(snapshot: &CloudSnapshot) -> AccountPlan {
    let allowance = &snapshot.allowance;
    let metered = allowance.limit_cents.is_some();
    AccountPlan {
        name: snapshot.plan.display_name.clone(),
        tier: Some(snapshot.plan.tier.clone()),
        state: Some(snapshot.plan.state.clone()),
        metered,
        monthly_allowance_usd: allowance.limit_cents.map(usd),
        used_usd: allowance.used_cents.map(usd),
        remaining_usd: allowance.remaining_cents.map(usd),
        resets_at: allowance.resets_at.clone(),
        renews_at: snapshot.plan.renews_at.clone(),
        source: SOURCE_CLOUD.into(),
    }
}

fn usage_from_snapshot(snapshot: &CloudSnapshot) -> AccountCloudUsage {
    let window = |source: &crate::account_cloud::CloudUsageWindow| {
        let finalized = usd(source.billed_cents);
        let pending = usd((source.nominal_cents - source.billed_cents).max(0));
        AccountUsageWindow {
            events: source.events,
            cost_usd: finalized,
            finalized_usd: finalized,
            pending_usd: pending,
            tokens: source.tokens,
            runtime_seconds: source.runtime_seconds,
        }
    };
    AccountCloudUsage {
        today: window(&snapshot.usage.today),
        seven_days: window(&snapshot.usage.seven_days),
        thirty_days: window(&snapshot.usage.thirty_days),
    }
}

fn state_from_snapshot(status: &str) -> String {
    match status {
        "active" => STATE_ACTIVE,
        "limited" => STATE_LIMITED,
        "past_due" => STATE_PAST_DUE,
        "canceled" => STATE_CANCELED,
        _ => STATE_UNKNOWN,
    }
    .into()
}

fn signed_out_summary(
    storage: &Storage,
    environment: &str,
    cloud: &SnapshotRead,
) -> AccountSummary {
    let paired = has_paired_before(storage);
    let auth_failure = cloud.failure_kind.as_deref() == Some(FAILURE_AUTH);
    let (state, session_health) = if !paired {
        (STATE_LOCAL_ONLY, SESSION_LOCAL_ONLY)
    } else if auth_failure {
        (STATE_SIGNED_OUT, SESSION_REVOKED)
    } else {
        (STATE_SIGNED_OUT, SESSION_SIGNED_OUT)
    };
    AccountSummary {
        signed_in: false,
        state: state.into(),
        environment: environment.into(),
        source: SOURCE_NONE.into(),
        account_id: None,
        display_name: None,
        email: None,
        organization: None,
        plan: None,
        cloud_usage: None,
        billing: AccountBilling::default(),
        catalog: Vec::new(),
        last_updated: None,
        stale: false,
        error: cloud.error.clone(),
        session_health: session_health.into(),
        failure_kind: cloud
            .failure_kind
            .clone()
            .unwrap_or_else(|| FAILURE_NONE.into()),
        quota_exhausted: false,
        reconciliation: if cloud.error.is_some() {
            RECONCILIATION_FAILED.into()
        } else {
            RECONCILIATION_OK.into()
        },
    }
}

/// Compose what the shell renders. `cloud` is the result of the last Account
/// Snapshot read; the caller owns fetching and caching it. `profile` is the
/// resolved `[intern]` profile (`local`, `local-slot1`, `staging`,
/// `production`, legacy `prod`, …) and is what gates the dev $200 stand-in —
/// never the display-only `environment` derived from `origin`.
pub fn summary(
    storage: &Storage,
    origin: &str,
    profile: &str,
    signed_in: bool,
    now: DateTime<Utc>,
    cloud: &SnapshotRead,
) -> Result<AccountSummary> {
    let environment = environment_from_origin(origin);
    if !signed_in {
        return Ok(signed_out_summary(storage, environment, cloud));
    }

    if let Some(snapshot) = cloud.snapshot.as_ref() {
        let identity = &snapshot.account;
        let state = state_from_snapshot(&snapshot.status);
        let quota_exhausted = state == STATE_LIMITED
            || snapshot
                .allowance
                .remaining_cents
                .is_some_and(|cents| cents <= 0);
        let failure_kind = if quota_exhausted {
            FAILURE_QUOTA
        } else if cloud.failure_kind.as_deref() == Some("malformed") {
            FAILURE_MALFORMED
        } else if cloud.stale {
            FAILURE_OUTAGE
        } else if snapshot.status == "canceled" || snapshot.status == "past_due" {
            FAILURE_ENTITLEMENT
        } else {
            FAILURE_NONE
        };
        let session_health = if cloud.stale {
            SESSION_OFFLINE
        } else if cloud.failure_kind.as_deref() == Some("malformed") {
            SESSION_MALFORMED
        } else {
            SESSION_ACTIVE
        };
        let reconciliation = if cloud.stale {
            RECONCILIATION_STALE
        } else if cloud.error.is_some() && !cloud.stale {
            RECONCILIATION_FAILED
        } else {
            RECONCILIATION_OK
        };
        return Ok(AccountSummary {
            signed_in: true,
            state,
            environment: environment.into(),
            source: SOURCE_CLOUD.into(),
            account_id: Some(identity.id.clone()),
            display_name: identity
                .display_name
                .clone()
                .or_else(|| identity.email.clone()),
            email: identity.email.clone(),
            organization: snapshot
                .organization
                .as_ref()
                .map(|org| AccountOrganization {
                    id: org.id.clone(),
                    display_name: org.display_name.clone(),
                    role: org.role.clone(),
                }),
            plan: Some(plan_from_snapshot(snapshot)),
            cloud_usage: Some(usage_from_snapshot(snapshot)),
            billing: AccountBilling {
                checkout_url: snapshot.billing_actions.checkout_url.clone(),
                portal_url: snapshot.billing_actions.portal_url.clone(),
                upgrade_tier: snapshot.billing_actions.upgrade_tier.clone(),
            },
            catalog: snapshot
                .catalog
                .iter()
                .map(|option| AccountPlanOption {
                    tier: option.tier.clone(),
                    display_name: option.display_name.clone(),
                    price_usd: usd(option.price_cents),
                    monthly_allowance_usd: usd(option.monthly_allowance_cents),
                })
                .collect(),
            last_updated: cloud.fetched_at.map(|at| at.to_rfc3339()),
            stale: cloud.stale,
            error: cloud.error.clone(),
            session_health: session_health.into(),
            failure_kind: failure_kind.into(),
            quota_exhausted,
            reconciliation: reconciliation.into(),
        });
    }

    // No snapshot: only an explicit local-only profile falls back to the
    // labelled stand-in so the shell stays exercisable offline; every cloud
    // profile — staging, production, legacy prod — shows an error and no
    // dollars at all.
    let seed_eligible = is_local_only_profile(profile);
    let plan = dev_seed_plan(storage, profile, now)?;
    let malformed = cloud.failure_kind.as_deref() == Some("malformed");
    let session_health = if malformed {
        SESSION_MALFORMED
    } else if cloud.failure_kind.as_deref() == Some("outage") || cloud.error.is_some() {
        SESSION_OFFLINE
    } else {
        SESSION_ACTIVE
    };
    Ok(AccountSummary {
        signed_in: true,
        state: if plan.is_some() {
            STATE_ACTIVE.into()
        } else {
            STATE_ERROR.into()
        },
        environment: environment.into(),
        source: if plan.is_some() {
            SOURCE_DEV_SEED.into()
        } else {
            SOURCE_NONE.into()
        },
        account_id: seed_eligible.then(|| DEV_ACCOUNT_ID.into()),
        display_name: seed_eligible.then(|| DEV_DISPLAY_NAME.into()),
        email: None,
        organization: None,
        plan,
        cloud_usage: None,
        billing: AccountBilling::default(),
        catalog: Vec::new(),
        last_updated: None,
        stale: false,
        error: cloud.error.clone(),
        session_health: session_health.into(),
        failure_kind: cloud
            .failure_kind
            .clone()
            .unwrap_or_else(|| FAILURE_NONE.into()),
        quota_exhausted: false,
        reconciliation: if cloud.error.is_some() {
            RECONCILIATION_FAILED.into()
        } else {
            RECONCILIATION_OK.into()
        },
    })
}

