//! Authoritative account summary for the desktop shell.
//!
//! Identity and plan truth live on the Rust side: the renderer only renders
//! what `account_get_summary` returns. For local/dev profiles the plan is
//! seeded once into `runtime_settings` (never for prod), and usage is charged
//! from the durable `usage_ledger` — cents arithmetic, clamped at zero.

use anyhow::Result;
use chrono::{DateTime, Datelike, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::storage::Storage;

const PLAN_SETTINGS_KEY: &str = "account.dev_plan.v1";
const DEV_MONTHLY_ALLOWANCE_CENTS: i64 = 20_000;
const DEV_PLAN_NAME: &str = "Synth Dev";
const DEV_DISPLAY_NAME: &str = "Synth Dev";
const DEV_ACCOUNT_ID: &str = "dev-local";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountPlan {
    pub name: String,
    pub monthly_allowance_usd: f64,
    pub used_usd: f64,
    pub remaining_usd: f64,
    pub resets_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummary {
    pub signed_in: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub environment: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub plan: Option<AccountPlan>,
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

/// Load the seeded dev plan, seeding it on first read. Never seeds for prod.
fn load_or_seed_plan(
    storage: &Storage,
    environment: &str,
    now: DateTime<Utc>,
) -> Result<Option<StoredPlan>> {
    let existing: Option<String> = storage.database().with_conn(|conn| {
        let mut statement =
            conn.prepare("SELECT value_json FROM runtime_settings WHERE key = ?1")?;
        let mut rows = statement.query([PLAN_SETTINGS_KEY])?;
        Ok(match rows.next()? {
            Some(row) => Some(row.get::<_, String>(0)?),
            None => None,
        })
    })?;
    if let Some(raw) = existing {
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
    storage.database().with_conn(|conn| {
        conn.execute(
            "INSERT INTO runtime_settings(key, value_json, updated_at) VALUES(?1, ?2, ?3)
             ON CONFLICT(key) DO NOTHING",
            [PLAN_SETTINGS_KEY, &payload, &seeded.seeded_at],
        )?;
        Ok(())
    })?;
    Ok(Some(seeded))
}

pub fn summary(
    storage: &Storage,
    origin: &str,
    signed_in: bool,
    now: DateTime<Utc>,
) -> Result<AccountSummary> {
    let environment = environment_from_origin(origin);
    if !signed_in {
        return Ok(AccountSummary {
            signed_in: false,
            account_id: None,
            display_name: None,
            environment: environment.into(),
            plan: None,
        });
    }
    let plan = match load_or_seed_plan(storage, environment, now)? {
        Some(stored) => {
            let used_cents = used_cents_since(storage, month_start(now))?;
            let remaining_cents = (stored.monthly_allowance_cents - used_cents).max(0);
            Some(AccountPlan {
                name: stored.name,
                monthly_allowance_usd: stored.monthly_allowance_cents as f64 / 100.0,
                used_usd: used_cents as f64 / 100.0,
                remaining_usd: remaining_cents as f64 / 100.0,
                resets_at: next_monthly_reset(now).to_rfc3339(),
            })
        }
        None => None,
    };
    let is_dev_env = environment != "prod";
    Ok(AccountSummary {
        signed_in: true,
        account_id: is_dev_env.then(|| DEV_ACCOUNT_ID.into()),
        display_name: is_dev_env.then(|| DEV_DISPLAY_NAME.into()),
        environment: environment.into(),
        plan,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn signed_out_summary_reports_no_identity_or_plan() {
        let (_root, storage) = open_storage();
        let summary = summary(&storage, "http://localhost:3000", false, now()).unwrap();
        assert!(!summary.signed_in);
        assert_eq!(summary.environment, "local");
        assert!(summary.account_id.is_none());
        assert!(summary.plan.is_none());
    }

    #[test]
    fn dev_sign_in_seeds_the_200_dollar_plan_once() {
        let (_root, storage) = open_storage();
        let first = summary(&storage, "http://localhost:3000", true, now()).unwrap();
        let plan = first.plan.expect("dev plan seeded");
        assert_eq!(plan.name, "Synth Dev");
        assert_eq!(plan.monthly_allowance_usd, 200.0);
        assert_eq!(plan.used_usd, 0.0);
        assert_eq!(plan.remaining_usd, 200.0);
        // A second read must reuse the stored seed, not create a new one.
        let second = summary(&storage, "http://localhost:3000", true, now()).unwrap();
        assert_eq!(second.plan.unwrap(), {
            let mut expected = plan.clone();
            expected.resets_at = next_monthly_reset(now()).to_rfc3339();
            expected
        });
        assert_eq!(first.display_name.as_deref(), Some("Synth Dev"));
        assert_eq!(first.account_id.as_deref(), Some("dev-local"));
    }

    #[test]
    fn used_and_remaining_are_exact_and_scoped_to_the_current_month() {
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
        let plan = summary(&storage, "http://localhost:3000", true, now())
            .unwrap()
            .plan
            .unwrap();
        assert_eq!(plan.used_usd, 13.0);
        assert_eq!(plan.remaining_usd, 187.0);
    }

    #[test]
    fn remaining_clamps_at_zero_when_usage_exceeds_the_allowance() {
        let (_root, storage) = open_storage();
        charge(&storage, "big", 250.0, "2026-08-02T00:00:00+00:00");
        let plan = summary(&storage, "http://localhost:3000", true, now())
            .unwrap()
            .plan
            .unwrap();
        assert_eq!(plan.used_usd, 250.0);
        assert_eq!(plan.remaining_usd, 0.0);
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

    #[test]
    fn prod_profiles_never_receive_the_dev_seed() {
        let (_root, storage) = open_storage();
        let summary = summary(&storage, "https://www.usesynth.ai", true, now()).unwrap();
        assert!(summary.signed_in);
        assert_eq!(summary.environment, "prod");
        assert!(
            summary.plan.is_none(),
            "prod must not be seeded with the dev plan"
        );
        assert!(summary.account_id.is_none());
        // And the seed row must not exist afterwards.
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
}
