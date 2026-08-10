//! Authoritative account summary for the desktop shell.
//!
//! Identity and plan truth live on the Rust side: the renderer only renders
//! what `account_get_summary` returns. When the device is paired, the Synth
//! Cloud Account Snapshot is that truth — plan, allowance, usage, and the
//! hosted billing links all come from the backend.
//!
//! Two rules hold everywhere below:
//!
//! * production never invents dollars. If the snapshot is missing or the
//!   backend does not meter this account in dollars, the shell shows no plan
//!   figure rather than a plausible one;
//! * local/dev keeps its seeded `Synth Dev` $200 stand-in — charged from the
//!   device `usage_ledger` — so the shell is exercisable offline, and it is
//!   always labelled as a dev stand-in, never as cloud truth.

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

pub const SOURCE_CLOUD: &str = "cloud";
pub const SOURCE_DEV_SEED: &str = "dev_seed";
pub const SOURCE_NONE: &str = "none";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountPlan {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// False when the backend reports no dollar limit for this account: the UI
    /// must then omit allowance figures instead of showing zeros.
    pub metered: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub monthly_allowance_usd: Option<f64>,
    pub used_usd: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remaining_usd: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub renews_at: Option<String>,
    /// `cloud` or `dev_seed`; the UI labels the stand-in explicitly.
    pub source: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountOrganization {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountUsageWindow {
    pub events: i64,
    pub cost_usd: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountCloudUsage {
    pub today: AccountUsageWindow,
    pub seven_days: AccountUsageWindow,
    pub thirty_days: AccountUsageWindow,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountBilling {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkout_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub portal_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upgrade_tier: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountPlanOption {
    pub tier: String,
    pub display_name: String,
    pub price_usd: f64,
    pub monthly_allowance_usd: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    pub signed_in: bool,
    pub state: String,
    pub environment: String,
    /// Where the rendered plan came from, so the UI can label a dev stand-in.
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization: Option<AccountOrganization>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<AccountPlan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cloud_usage: Option<AccountCloudUsage>,
    pub billing: AccountBilling,
    pub catalog: Vec<AccountPlanOption>,
    /// When the rendered cloud facts were fetched, for a `Last updated` line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_updated: Option<String>,
    pub stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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
    let used_usd: f64 = storage.database().with_conn(|conn| {
        let mut statement = conn.prepare(
            "SELECT COALESCE(SUM(cost_usd), 0) FROM usage_ledger WHERE cost_usd IS NOT NULL AND created_at >= ?1",
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

/// Load the seeded dev plan, seeding it on first read. Never seeds for prod.
fn load_or_seed_plan(
    storage: &Storage,
    environment: &str,
    now: DateTime<Utc>,
) -> Result<Option<StoredPlan>> {
    if let Some(raw) = read_setting(storage, PLAN_SETTINGS_KEY)? {
        return Ok(serde_json::from_str(&raw).ok());
    }
    if environment == "prod" {
        return Ok(None);
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
    environment: &str,
    now: DateTime<Utc>,
) -> Result<Option<AccountPlan>> {
    let Some(stored) = load_or_seed_plan(storage, environment, now)? else {
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
        used_usd: usd(used_cents),
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
        used_usd: usd(allowance.used_cents),
        remaining_usd: allowance.remaining_cents.map(usd),
        resets_at: allowance.resets_at.clone(),
        renews_at: snapshot.plan.renews_at.clone(),
        source: SOURCE_CLOUD.into(),
    }
}

fn usage_from_snapshot(snapshot: &CloudSnapshot) -> AccountCloudUsage {
    let window = |source: &crate::account_cloud::CloudUsageWindow| AccountUsageWindow {
        events: source.events,
        cost_usd: usd(source.billed_cents),
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

fn signed_out_summary(storage: &Storage, environment: &str) -> AccountSummary {
    AccountSummary {
        signed_in: false,
        state: if has_paired_before(storage) {
            STATE_SIGNED_OUT.into()
        } else {
            STATE_LOCAL_ONLY.into()
        },
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
        error: None,
    }
}

/// Compose what the shell renders. `cloud` is the result of the last Account
/// Snapshot read; the caller owns fetching and caching it.
pub fn summary(
    storage: &Storage,
    origin: &str,
    signed_in: bool,
    now: DateTime<Utc>,
    cloud: &SnapshotRead,
) -> Result<AccountSummary> {
    let environment = environment_from_origin(origin);
    if !signed_in {
        return Ok(signed_out_summary(storage, environment));
    }

    if let Some(snapshot) = cloud.snapshot.as_ref() {
        let identity = &snapshot.account;
        return Ok(AccountSummary {
            signed_in: true,
            state: state_from_snapshot(&snapshot.status),
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
        });
    }

    // No snapshot: local/dev falls back to the labelled stand-in so the shell
    // stays exercisable offline; prod shows an error and no dollars at all.
    let plan = dev_seed_plan(storage, environment, now)?;
    let is_dev_env = environment != "prod";
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
        account_id: is_dev_env.then(|| DEV_ACCOUNT_ID.into()),
        display_name: is_dev_env.then(|| DEV_DISPLAY_NAME.into()),
        email: None,
        organization: None,
        plan,
        cloud_usage: None,
        billing: AccountBilling::default(),
        catalog: Vec::new(),
        last_updated: None,
        stale: false,
        error: cloud.error.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account_cloud::{
        CloudAccount, CloudAllowance, CloudBillingActions, CloudOrganization, CloudPlan,
        CloudPlanOption, CloudUsageWindow, CloudUsageWindows,
    };

    fn open_storage() -> (tempfile::TempDir, Storage) {
        let root = tempfile::tempdir().unwrap();
        let storage = Storage::open(root.path()).unwrap();
        (root, storage)
    }

    fn charge(storage: &Storage, id: &str, cost_usd: f64, created_at: &str) {
        storage
            .database()
            .with_conn(|conn| {
                conn.execute(
                    "INSERT INTO usage_ledger(id,provider,model,prompt_tokens,completion_tokens,total_tokens,cost_usd,created_at)
                     VALUES(?1,'synth','laguna-s',10,10,20,?2,?3)",
                    rusqlite::params![id, cost_usd, created_at],
                )?;
                Ok(())
            })
            .unwrap();
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 8, 10, 12, 0, 0).unwrap()
    }

    fn offline() -> SnapshotRead {
        SnapshotRead::default()
    }

    fn cloud_snapshot(status: &str, limit_cents: Option<i64>, used_cents: i64) -> SnapshotRead {
        let snapshot = CloudSnapshot {
            schema_version: crate::account_cloud::SCHEMA_VERSION.into(),
            status: status.into(),
            account: CloudAccount {
                id: "acct_1".into(),
                display_name: Some("ada".into()),
                email: Some("ada@example.com".into()),
                avatar_url: None,
            },
            organization: Some(CloudOrganization {
                id: "org_1".into(),
                display_name: Some("Ada Labs".into()),
                role: Some("owner".into()),
            }),
            plan: CloudPlan {
                tier: "pro".into(),
                display_name: "Pro".into(),
                state: "active".into(),
                price_cents: 20_000,
                renews_at: Some("2026-09-01T00:00:00+00:00".into()),
                is_paid: true,
            },
            allowance: CloudAllowance {
                limit_cents,
                used_cents,
                remaining_cents: limit_cents.map(|limit| (limit - used_cents).max(0)),
                resets_at: Some("2026-09-01T00:00:00+00:00".into()),
                source: "entitlement".into(),
            },
            usage: CloudUsageWindows {
                today: CloudUsageWindow {
                    events: 2,
                    billed_cents: 15,
                    nominal_cents: 15,
                },
                seven_days: CloudUsageWindow {
                    events: 9,
                    billed_cents: 120,
                    nominal_cents: 120,
                },
                thirty_days: CloudUsageWindow {
                    events: 40,
                    billed_cents: 1_300,
                    nominal_cents: 1_300,
                },
            },
            billing_actions: CloudBillingActions {
                checkout_url: Some("https://example.test/usage?upgrade=pro".into()),
                portal_url: Some("https://example.test/usage".into()),
                upgrade_tier: Some("pro".into()),
            },
            catalog: vec![CloudPlanOption {
                tier: "starter".into(),
                display_name: "Starter".into(),
                price_cents: 2_000,
                monthly_allowance_cents: 2_000,
                interval: "month".into(),
            }],
            generated_at: "2026-08-10T12:00:00+00:00".into(),
            degraded: Vec::new(),
        };
        SnapshotRead {
            snapshot: Some(snapshot),
            stale: false,
            error: None,
            fetched_at: Some(now()),
            unauthenticated: false,
        }
    }

    #[test]
    fn a_fresh_install_reads_as_local_only_not_signed_out() {
        let (_root, storage) = open_storage();
        let summary = summary(&storage, "http://localhost:3000", false, now(), &offline()).unwrap();
        assert!(!summary.signed_in);
        assert_eq!(summary.state, STATE_LOCAL_ONLY);
        assert_eq!(summary.environment, "local");
        assert!(summary.account_id.is_none());
        assert!(summary.plan.is_none());
    }

    #[test]
    fn a_device_that_has_paired_before_reads_as_signed_out() {
        let (_root, storage) = open_storage();
        mark_paired(&storage, now()).unwrap();
        let summary = summary(&storage, "http://localhost:3000", false, now(), &offline()).unwrap();
        assert_eq!(summary.state, STATE_SIGNED_OUT);
        assert!(summary.plan.is_none(), "signed out shows no plan dollars");
    }

    #[test]
    fn the_cloud_snapshot_is_rendered_verbatim_when_present() {
        let (_root, storage) = open_storage();
        // A local ledger charge must not touch the cloud numbers.
        charge(&storage, "local", 42.0, "2026-08-05T00:00:00+00:00");
        let summary = summary(
            &storage,
            "https://www.usesynth.ai",
            true,
            now(),
            &cloud_snapshot("active", Some(20_000), 4_250),
        )
        .unwrap();
        assert_eq!(summary.state, STATE_ACTIVE);
        assert_eq!(summary.source, SOURCE_CLOUD);
        assert_eq!(summary.display_name.as_deref(), Some("ada"));
        assert_eq!(summary.email.as_deref(), Some("ada@example.com"));
        assert_eq!(
            summary.organization.as_ref().map(|org| org.id.as_str()),
            Some("org_1")
        );
        let plan = summary.plan.expect("cloud plan");
        assert_eq!(plan.name, "Pro");
        assert_eq!(plan.tier.as_deref(), Some("pro"));
        assert_eq!(plan.monthly_allowance_usd, Some(200.0));
        assert_eq!(plan.used_usd, 42.50);
        assert_eq!(plan.remaining_usd, Some(157.50));
        assert_eq!(plan.source, SOURCE_CLOUD);
        let usage = summary.cloud_usage.expect("cloud usage");
        assert_eq!(usage.today.cost_usd, 0.15);
        assert_eq!(usage.thirty_days.events, 40);
        assert_eq!(
            summary.billing.portal_url.as_deref(),
            Some("https://example.test/usage")
        );
        assert_eq!(summary.catalog.len(), 1);
        assert_eq!(summary.catalog[0].price_usd, 20.0);
    }

    #[test]
    fn an_exhausted_cloud_allowance_reports_limited() {
        let (_root, storage) = open_storage();
        let summary = summary(
            &storage,
            "https://www.usesynth.ai",
            true,
            now(),
            &cloud_snapshot("limited", Some(2_000), 2_500),
        )
        .unwrap();
        assert_eq!(summary.state, STATE_LIMITED);
        let plan = summary.plan.unwrap();
        assert_eq!(plan.remaining_usd, Some(0.0));
        assert!(plan.metered);
    }

    #[test]
    fn an_unmetered_cloud_account_reports_no_dollar_figures() {
        let (_root, storage) = open_storage();
        let summary = summary(
            &storage,
            "https://www.usesynth.ai",
            true,
            now(),
            &cloud_snapshot("active", None, 0),
        )
        .unwrap();
        let plan = summary.plan.unwrap();
        assert!(!plan.metered);
        assert!(plan.monthly_allowance_usd.is_none());
        assert!(plan.remaining_usd.is_none());
    }

    #[test]
    fn a_stale_snapshot_still_renders_and_says_so() {
        let (_root, storage) = open_storage();
        let mut cloud = cloud_snapshot("active", Some(20_000), 0);
        cloud.stale = true;
        cloud.error = Some("Synth Cloud is unavailable right now".into());
        let summary = summary(&storage, "https://www.usesynth.ai", true, now(), &cloud).unwrap();
        assert!(summary.stale);
        assert_eq!(summary.state, STATE_ACTIVE);
        assert!(summary.error.is_some());
        assert!(summary.last_updated.is_some());
    }

    #[test]
    fn prod_without_a_snapshot_shows_an_error_and_never_seeds_dollars() {
        let (_root, storage) = open_storage();
        let cloud = SnapshotRead {
            error: Some("Synth Cloud is unavailable right now".into()),
            ..SnapshotRead::default()
        };
        let summary = summary(&storage, "https://www.usesynth.ai", true, now(), &cloud).unwrap();
        assert!(summary.signed_in);
        assert_eq!(summary.state, STATE_ERROR);
        assert!(summary.plan.is_none(), "prod must not be seeded");
        assert_eq!(
            summary.error.as_deref(),
            Some("Synth Cloud is unavailable right now")
        );
        let count: i64 = storage
            .database()
            .with_conn(|conn| {
                let mut statement = conn.prepare(
                    "SELECT COUNT(*) FROM runtime_settings WHERE key = 'account.dev_plan.v1'",
                )?;
                let value: i64 = statement.query_row([], |row| row.get(0))?;
                Ok(value)
            })
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn dev_without_a_snapshot_seeds_the_200_dollar_stand_in_once() {
        let (_root, storage) = open_storage();
        let first = summary(&storage, "http://localhost:3000", true, now(), &offline()).unwrap();
        let plan = first.plan.expect("dev plan seeded");
        assert_eq!(plan.name, "Synth Dev");
        assert_eq!(plan.source, SOURCE_DEV_SEED);
        assert_eq!(plan.monthly_allowance_usd, Some(200.0));
        assert_eq!(plan.used_usd, 0.0);
        assert_eq!(plan.remaining_usd, Some(200.0));
        assert_eq!(first.source, SOURCE_DEV_SEED);
        assert_eq!(first.display_name.as_deref(), Some("Synth Dev"));
        assert_eq!(first.account_id.as_deref(), Some("dev-local"));

        let second = summary(&storage, "http://localhost:3000", true, now(), &offline()).unwrap();
        assert_eq!(second.plan.unwrap(), plan);
    }

    #[test]
    fn the_dev_stand_in_charges_the_device_ledger_for_the_current_month_only() {
        let (_root, storage) = open_storage();
        charge(&storage, "u1", 12.34, "2026-08-05T00:00:00+00:00");
        charge(&storage, "u2", 0.66, "2026-08-09T00:00:00+00:00");
        charge(&storage, "old", 99.0, "2026-07-31T23:59:59+00:00");
        charge(&storage, "untracked", 5.0, "2026-08-06T00:00:00+00:00");
        storage
            .database()
            .with_conn(|conn| {
                conn.execute(
                    "UPDATE usage_ledger SET cost_usd = NULL WHERE id = 'untracked'",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        let plan = summary(&storage, "http://localhost:3000", true, now(), &offline())
            .unwrap()
            .plan
            .unwrap();
        assert_eq!(plan.used_usd, 13.0);
        assert_eq!(plan.remaining_usd, Some(187.0));
    }

    #[test]
    fn the_dev_stand_in_clamps_at_zero_when_usage_exceeds_the_allowance() {
        let (_root, storage) = open_storage();
        charge(&storage, "big", 250.0, "2026-08-02T00:00:00+00:00");
        let plan = summary(&storage, "http://localhost:3000", true, now(), &offline())
            .unwrap()
            .plan
            .unwrap();
        assert_eq!(plan.used_usd, 250.0);
        assert_eq!(plan.remaining_usd, Some(0.0));
    }

    #[test]
    fn reset_boundary_advances_monthly_and_wraps_the_year() {
        let december = Utc.with_ymd_and_hms(2026, 12, 15, 8, 30, 0).unwrap();
        assert_eq!(
            next_monthly_reset(december),
            Utc.with_ymd_and_hms(2027, 1, 1, 0, 0, 0).unwrap()
        );
        assert_eq!(
            next_monthly_reset(now()),
            Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap()
        );
    }
}
