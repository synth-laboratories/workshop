//! Opaque run capabilities. Token claims are ceilings; remaining budget is
//! tracked in host memory and SQLite, never in the handle itself.

use anyhow::{anyhow, Result};
use chrono::{Duration as ChronoDuration, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;

use super::audit::{self, SecretAuditEvent};

pub(crate) const PROVIDER_USAGE_RECEIPT_SCHEMA: &str = "workshop.provider-usage-receipt.v1";

/// Non-secret identity retained for every capability included in a run receipt.
/// Handles and secret ids are deliberately excluded from this optimizer-facing
/// surface.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderUsageCapability {
    pub capability_id: String,
    pub provider: String,
    pub status: String,
}

/// Durable provider usage aggregated across every capability issued to a run.
/// A missing receipt means no capability rows exist. `cost_usd: None` means at
/// least one included capability has an unknown charge, never a measured zero.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProviderUsageReceipt {
    pub schema_version: String,
    pub run_id: String,
    pub capabilities: Vec<ProviderUsageCapability>,
    pub calls: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: Option<f64>,
    pub digest: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CanonicalProviderUsageReceipt<'a> {
    schema_version: &'a str,
    run_id: &'a str,
    capabilities: &'a [ProviderUsageCapability],
    calls: u64,
    input_tokens: u64,
    output_tokens: u64,
    cost_usd_micros: Option<u64>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStatus {
    Granted,
    Active,
    Exhausted,
    Expired,
    Revoked,
}

impl CapabilityStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Granted => "granted",
            Self::Active => "active",
            Self::Exhausted => "exhausted",
            Self::Expired => "expired",
            Self::Revoked => "revoked",
        }
    }

    fn can_be_revoked(self) -> bool {
        matches!(self, Self::Granted | Self::Active)
    }
}

impl TryFrom<&str> for CapabilityStatus {
    type Error = anyhow::Error;

    fn try_from(value: &str) -> Result<Self> {
        match value {
            "granted" => Ok(Self::Granted),
            "active" => Ok(Self::Active),
            "exhausted" => Ok(Self::Exhausted),
            "expired" => Ok(Self::Expired),
            "revoked" => Ok(Self::Revoked),
            other => Err(anyhow!("unknown capability status {other}")),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsePolicy {
    pub operations: Vec<String>,
    pub models: Vec<String>,
    pub reasoning_efforts: Vec<String>,
    pub max_calls: u32,
    #[specta(type = specta_typescript::Number)]
    pub max_input_tokens: u64,
    #[specta(type = specta_typescript::Number)]
    pub max_output_tokens: u64,
    pub max_cost_usd: f64,
    #[specta(type = specta_typescript::Number)]
    pub lifetime_seconds: u64,
}

impl Default for ProviderUsePolicy {
    fn default() -> Self {
        Self {
            operations: vec!["chat.completions.create".into()],
            models: Vec::new(),
            reasoning_efforts: vec!["medium".into()],
            max_calls: 40,
            max_input_tokens: 200_000,
            max_output_tokens: 40_000,
            max_cost_usd: 0.60,
            lifetime_seconds: crate::limits::SECRETS_CAPABILITY_TTL.as_secs(),
        }
    }
}

impl ProviderUsePolicy {
    pub fn intersect(&self, other: &Self) -> Self {
        Self {
            operations: intersect_list(&self.operations, &other.operations),
            models: intersect_list(&self.models, &other.models),
            reasoning_efforts: intersect_list(&self.reasoning_efforts, &other.reasoning_efforts),
            max_calls: self.max_calls.min(other.max_calls),
            max_input_tokens: self.max_input_tokens.min(other.max_input_tokens),
            max_output_tokens: self.max_output_tokens.min(other.max_output_tokens),
            max_cost_usd: self.max_cost_usd.min(other.max_cost_usd),
            lifetime_seconds: self.lifetime_seconds.min(other.lifetime_seconds),
        }
    }
}

fn intersect_list(left: &[String], right: &[String]) -> Vec<String> {
    if left.is_empty() {
        return right.to_vec();
    }
    if right.is_empty() {
        return left.to_vec();
    }
    left.iter()
        .filter(|item| right.iter().any(|other| other.eq_ignore_ascii_case(item)))
        .cloned()
        .collect()
}

#[derive(Clone, Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CapabilitySummary {
    pub id: String,
    pub secret_id: String,
    pub run_id: String,
    pub recipe_id: String,
    pub provider: String,
    pub status: String,
    pub max_calls: u32,
    pub used_calls: u32,
    pub max_cost_usd: f64,
    pub used_cost_usd: Option<f64>,
    #[specta(type = specta_typescript::Number)]
    pub used_input_tokens: u64,
    #[specta(type = specta_typescript::Number)]
    pub used_output_tokens: u64,
    pub expires_at: String,
    pub display_suffix: Option<String>,
}

#[derive(Clone, Debug)]
pub struct IssuedCapability {
    pub id: String,
    pub handle: String,
    pub proxy_origin: String,
    pub summary: CapabilitySummary,
}

#[derive(Clone, Debug)]
pub struct LiveCapability {
    pub id: String,
    pub handle: String,
    pub secret_id: String,
    pub run_id: String,
    pub recipe_id: String,
    pub provider: String,
    pub operations: Vec<String>,
    pub models: Vec<String>,
    pub reasoning_efforts: Vec<String>,
    pub max_calls: u32,
    pub max_input_tokens: u64,
    pub max_output_tokens: u64,
    pub max_cost_usd_micros: u64,
    pub used_calls: u32,
    pub used_input_tokens: u64,
    pub used_output_tokens: u64,
    /// `Some(0)` is a genuine measured zero. `None` means at least one
    /// completed provider call did not report a settled charge.
    pub used_cost_usd_micros: Option<u64>,
    pub status: String,
    pub expires_at_ms: i64,
}

impl LiveCapability {
    pub fn remaining_calls(&self) -> u32 {
        self.max_calls.saturating_sub(self.used_calls)
    }

    fn cost_usd(&self) -> Option<f64> {
        self.used_cost_usd_micros
            .map(|micros| micros as f64 / 1_000_000.0)
    }

    fn max_cost_usd(&self) -> f64 {
        self.max_cost_usd_micros as f64 / 1_000_000.0
    }

    /// Whether this already-issued grant is at least as broad as the admitted
    /// workload envelope asking to reuse it. Empty model/reasoning lists mean
    /// unrestricted in the proxy contract; operations are always explicit.
    pub(crate) fn covers(&self, requested: &ProviderUsePolicy) -> bool {
        list_covers(&self.operations, &requested.operations)
            && list_covers(&self.models, &requested.models)
            && list_covers(&self.reasoning_efforts, &requested.reasoning_efforts)
            && self.max_calls >= requested.max_calls
            && self.max_input_tokens >= requested.max_input_tokens
            && self.max_output_tokens >= requested.max_output_tokens
            && self.max_cost_usd() + f64::EPSILON >= requested.max_cost_usd
    }
}

fn list_covers(granted: &[String], requested: &[String]) -> bool {
    granted.is_empty()
        || requested.iter().all(|requested_item| {
            granted
                .iter()
                .any(|granted_item| granted_item.eq_ignore_ascii_case(requested_item))
        })
}

#[derive(Clone, Debug)]
pub struct MeasuredUsage {
    pub calls: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// Exact provider-reported charge. `None` means the provider did not
    /// report a settled cost; it must not be replaced by a tariff estimate.
    pub cost_usd: Option<f64>,
}

/// Trusted provider accounting summed across every capability issued to one
/// optimizer run.
#[derive(Clone, Debug, Default, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct CapabilityLedger {
    pub capabilities: u32,
    pub status: Option<String>,
    pub used_calls: u32,
    pub max_calls: u32,
    #[specta(type = specta_typescript::Number)]
    pub used_input_tokens: u64,
    #[specta(type = specta_typescript::Number)]
    pub used_output_tokens: u64,
    pub used_cost_usd: Option<f64>,
    pub max_cost_usd: f64,
}

impl CapabilityLedger {
    pub fn from_capabilities(rows: &[LiveCapability]) -> Self {
        let mut ledger = Self {
            capabilities: rows.len() as u32,
            ..Self::default()
        };
        let mut billed_micros = 0u64;
        for row in rows {
            ledger.used_calls = ledger.used_calls.saturating_add(row.used_calls);
            ledger.max_calls = ledger.max_calls.saturating_add(row.max_calls);
            ledger.used_input_tokens = ledger
                .used_input_tokens
                .saturating_add(row.used_input_tokens);
            ledger.used_output_tokens = ledger
                .used_output_tokens
                .saturating_add(row.used_output_tokens);
            ledger.max_cost_usd += row.max_cost_usd_micros as f64 / 1_000_000.0;
            billed_micros = billed_micros.saturating_add(row.used_cost_usd_micros.unwrap_or(0));
            ledger.status = Some(row.status.clone());
        }
        ledger.used_cost_usd = (billed_micros > 0).then(|| billed_micros as f64 / 1_000_000.0);
        ledger
    }

    pub fn has_accounting(&self) -> bool {
        self.used_calls > 0
            || self.used_input_tokens > 0
            || self.used_output_tokens > 0
            || self.used_cost_usd.is_some()
    }
}

#[derive(Default)]
pub struct CapabilityStore {
    by_handle: Mutex<HashMap<String, LiveCapability>>,
    next_request_at: Mutex<HashMap<String, Instant>>,
    adaptive_interval: Mutex<HashMap<String, Duration>>,
}

impl CapabilityStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn lookup(&self, handle: &str) -> Option<LiveCapability> {
        self.by_handle
            .lock()
            .expect("capability store")
            .get(handle)
            .cloned()
    }

    pub fn insert(&self, live: LiveCapability) {
        self.by_handle
            .lock()
            .expect("capability store")
            .insert(live.handle.clone(), live);
    }

    pub fn revoke_handle(&self, handle: &str) {
        if let Some(live) = self
            .by_handle
            .lock()
            .expect("capability store")
            .get_mut(handle)
        {
            let status = CapabilityStatus::try_from(live.status.as_str());
            if status.is_ok_and(CapabilityStatus::can_be_revoked) {
                live.status = CapabilityStatus::Revoked.as_str().into();
            }
        }
        self.next_request_at
            .lock()
            .expect("capability request pacer")
            .remove(handle);
    }

    pub fn revoke_run(&self, run_id: &str) -> Vec<String> {
        let mut store = self.by_handle.lock().expect("capability store");
        let mut handles = Vec::new();
        for live in store.values_mut() {
            let status = CapabilityStatus::try_from(live.status.as_str());
            if live.run_id == run_id && status.is_ok_and(CapabilityStatus::can_be_revoked) {
                live.status = CapabilityStatus::Revoked.as_str().into();
                handles.push(live.handle.clone());
            }
        }
        handles
    }

    pub fn revoke_secret(&self, secret_id: &str) -> Vec<String> {
        let mut store = self.by_handle.lock().expect("capability store");
        let mut handles = Vec::new();
        for live in store.values_mut() {
            let status = CapabilityStatus::try_from(live.status.as_str());
            if live.secret_id == secret_id && status.is_ok_and(CapabilityStatus::can_be_revoked) {
                live.status = CapabilityStatus::Revoked.as_str().into();
                handles.push(live.handle.clone());
            }
        }
        let mut pacing = self
            .next_request_at
            .lock()
            .expect("capability request pacer");
        for handle in &handles {
            pacing.remove(handle);
        }
        handles
    }

    /// Claim the provider request-start boundary only when it is currently
    /// eligible. Waiters observe the same boundary without pushing later
    /// requests into speculative future slots.
    fn request_start_delay_at(&self, handle: &str, now: Instant, interval: Duration) -> Duration {
        let mut next = self
            .next_request_at
            .lock()
            .expect("capability request pacer");
        let eligible_at = next.get(handle).copied().unwrap_or(now);
        let delay = eligible_at.saturating_duration_since(now);
        if delay.is_zero() {
            next.insert(handle.to_owned(), now + interval);
        }
        delay
    }

    pub async fn pace_request_start(&self, handle: &str) {
        loop {
            let delay = self.request_start_delay_at(
                handle,
                Instant::now(),
                crate::limits::CREDENTIAL_UPSTREAM_MIN_INTERVAL,
            );
            if delay.is_zero() {
                return;
            }
            tokio::time::sleep(delay).await;
        }
    }

    pub fn request_interval(&self, handle: &str) -> Duration {
        self.adaptive_interval
            .lock()
            .expect("capability request pacer")
            .get(handle)
            .copied()
            .unwrap_or(crate::limits::CREDENTIAL_UPSTREAM_MIN_INTERVAL)
    }

    pub fn observe_rate_limit(&self, handle: &str, reset: Option<Duration>) -> Duration {
        let floor = crate::limits::CREDENTIAL_UPSTREAM_MIN_INTERVAL;
        let ceiling = crate::limits::CREDENTIAL_UPSTREAM_MAX_INTERVAL;
        let mut intervals = self
            .adaptive_interval
            .lock()
            .expect("capability request pacer");
        let current = intervals.get(handle).copied().unwrap_or(floor);
        let raised = reset
            .unwrap_or(current.saturating_mul(2))
            .max(current)
            .max(floor)
            .min(ceiling);
        intervals.insert(handle.to_owned(), raised);
        raised
    }

    pub fn observe_admitted(&self, handle: &str) -> Duration {
        let floor = crate::limits::CREDENTIAL_UPSTREAM_MIN_INTERVAL;
        let mut intervals = self
            .adaptive_interval
            .lock()
            .expect("capability request pacer");
        let current = intervals.get(handle).copied().unwrap_or(floor);
        let decayed = Duration::from_millis(current.as_millis() as u64 / 2).max(floor);
        intervals.insert(handle.to_owned(), decayed);
        decayed
    }

    pub fn list_active(&self) -> Vec<LiveCapability> {
        self.by_handle
            .lock()
            .expect("capability store")
            .values()
            .filter(|live| live.status == "granted" || live.status == "active")
            .cloned()
            .collect()
    }

    /// Return every capability issued for a run, including terminal entries.
    /// Revocation deliberately leaves the metering counters in memory so the
    /// run's sealed receipt can preserve the authoritative proxy totals.
    pub fn list_for_run(&self, run_id: &str) -> Vec<LiveCapability> {
        self.by_handle
            .lock()
            .expect("capability store")
            .values()
            .filter(|live| live.run_id == run_id)
            .cloned()
            .collect()
    }

    pub fn find_active(&self, secret_id: &str, run_id: &str) -> Option<LiveCapability> {
        self.list_active()
            .into_iter()
            .find(|live| live.secret_id == secret_id && live.run_id == run_id)
    }

    /// Reserve one call. Fail closed at the call ceiling.
    pub fn reserve_call(&self, handle: &str) -> Result<LiveCapability> {
        let mut store = self.by_handle.lock().expect("capability store");
        let live = store
            .get_mut(handle)
            .ok_or_else(|| anyhow!("capability is not valid"))?;
        let now = Utc::now().timestamp_millis();
        if live.status == "revoked" {
            anyhow::bail!("capability was revoked");
        }
        if live.status == "exhausted" {
            anyhow::bail!("capability budget is exhausted");
        }
        if now >= live.expires_at_ms {
            live.status = "expired".into();
            anyhow::bail!("capability expired");
        }
        if live.used_calls >= live.max_calls {
            live.status = "exhausted".into();
            anyhow::bail!("capability call ceiling reached");
        }
        if live
            .used_cost_usd_micros
            .is_some_and(|used| used >= live.max_cost_usd_micros)
            && live.max_cost_usd_micros > 0
        {
            live.status = "exhausted".into();
            anyhow::bail!("capability spend ceiling reached");
        }
        live.used_calls += 1;
        if live.status == "granted" {
            live.status = "active".into();
        }
        Ok(live.clone())
    }

    pub fn debit_usage(&self, handle: &str, usage: &MeasuredUsage) -> Result<LiveCapability> {
        let mut store = self.by_handle.lock().expect("capability store");
        let live = store
            .get_mut(handle)
            .ok_or_else(|| anyhow!("capability is not valid"))?;
        live.used_input_tokens = live.used_input_tokens.saturating_add(usage.input_tokens);
        live.used_output_tokens = live.used_output_tokens.saturating_add(usage.output_tokens);
        if let Some(cost_usd) = usage
            .cost_usd
            .filter(|cost| cost.is_finite() && *cost >= 0.0)
        {
            let micros = (cost_usd * 1_000_000.0).round() as u64;
            if let Some(used) = live.used_cost_usd_micros.as_mut() {
                *used = used.saturating_add(micros);
            }
        } else {
            // Once any billed call is missing cost the aggregate is unknown;
            // later reported charges cannot make that missing amount vanish.
            live.used_cost_usd_micros = None;
        }
        if live.used_input_tokens > live.max_input_tokens
            || live.used_output_tokens > live.max_output_tokens
            || (live.max_cost_usd_micros > 0
                && live
                    .used_cost_usd_micros
                    .is_some_and(|used| used > live.max_cost_usd_micros))
            || live.used_calls >= live.max_calls
        {
            live.status = "exhausted".into();
        }
        Ok(live.clone())
    }
}

pub fn issue(
    conn: &Connection,
    store: &CapabilityStore,
    secret_id: &str,
    run_id: &str,
    recipe_id: &str,
    provider: &str,
    policy: &ProviderUsePolicy,
    actor: &str,
) -> Result<IssuedCapability> {
    let id = format!("cap_{}", Uuid::new_v4().simple());
    let handle = format!("wcap_{}", Uuid::new_v4().simple());
    let now = Utc::now();
    let ttl = ChronoDuration::seconds(policy.lifetime_seconds.max(30) as i64);
    let expires = now + ttl;
    let max_cost_usd_micros = (policy.max_cost_usd.max(0.0) * 1_000_000.0).round() as u64;
    conn.execute(
        "INSERT INTO secret_capabilities(
            id, handle, secret_id, run_id, recipe_id, provider, operations_json, models_json,
            reasoning_efforts_json, max_calls, max_input_tokens, max_output_tokens,
            max_cost_usd_micros, used_calls, used_input_tokens, used_output_tokens,
            used_cost_usd_micros, status, created_at, expires_at, revoked_at,
            used_cost_known
        ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,0,0,0,0,'granted',?14,?15,NULL,1)",
        params![
            id,
            handle,
            secret_id,
            run_id,
            recipe_id,
            provider,
            serde_json::to_string(&policy.operations)?,
            serde_json::to_string(&policy.models)?,
            serde_json::to_string(&policy.reasoning_efforts)?,
            policy.max_calls as i64,
            policy.max_input_tokens as i64,
            policy.max_output_tokens as i64,
            max_cost_usd_micros as i64,
            now.to_rfc3339(),
            expires.to_rfc3339(),
        ],
    )?;
    let live = LiveCapability {
        id: id.clone(),
        handle: handle.clone(),
        secret_id: secret_id.into(),
        run_id: run_id.into(),
        recipe_id: recipe_id.into(),
        provider: provider.into(),
        operations: policy.operations.clone(),
        models: policy.models.clone(),
        reasoning_efforts: policy.reasoning_efforts.clone(),
        max_calls: policy.max_calls,
        max_input_tokens: policy.max_input_tokens,
        max_output_tokens: policy.max_output_tokens,
        max_cost_usd_micros,
        used_calls: 0,
        used_input_tokens: 0,
        used_output_tokens: 0,
        used_cost_usd_micros: Some(0),
        status: "granted".into(),
        expires_at_ms: expires.timestamp_millis(),
    };
    store.insert(live.clone());
    let mut event = SecretAuditEvent::new("user", actor, "capability.issue", "granted");
    event.secret_id = Some(secret_id.into());
    event.provider = Some(provider.into());
    event.capability_id = Some(id.clone());
    audit::append(conn, &event)?;
    Ok(IssuedCapability {
        id: id.clone(),
        handle,
        proxy_origin: String::new(),
        summary: summary_from_live(&live, None),
    })
}

pub fn persist_usage(conn: &Connection, live: &LiveCapability) -> Result<()> {
    conn.execute(
        "UPDATE secret_capabilities SET used_calls=?1, used_input_tokens=?2,
         used_output_tokens=?3, used_cost_usd_micros=?4, used_cost_known=?5,
         revoked_at=CASE WHEN status='revoked' THEN revoked_at ELSE NULL END,
         status=CASE WHEN status IN ('exhausted','expired','revoked') THEN status ELSE ?6 END
         WHERE id=?7",
        params![
            live.used_calls as i64,
            live.used_input_tokens as i64,
            live.used_output_tokens as i64,
            live.used_cost_usd_micros.unwrap_or(0) as i64,
            i64::from(live.used_cost_usd_micros.is_some()),
            live.status,
            live.id
        ],
    )?;
    Ok(())
}

pub fn revoke(
    conn: &Connection,
    store: &CapabilityStore,
    capability_id: &str,
    actor: &str,
) -> Result<()> {
    let handle: Option<String> = conn
        .query_row(
            "SELECT handle FROM secret_capabilities WHERE id=?1",
            [capability_id],
            |row| row.get(0),
        )
        .optional()?;
    let revoked = conn.execute(
        "UPDATE secret_capabilities SET status='revoked', revoked_at=?1
         WHERE id=?2 AND status IN ('granted','active')",
        params![Utc::now().to_rfc3339(), capability_id],
    )?;
    if revoked > 0 {
        if let Some(handle) = handle {
            store.revoke_handle(&handle);
        }
        let mut event = SecretAuditEvent::new("user", actor, "capability.revoke", "revoked");
        event.capability_id = Some(capability_id.into());
        audit::append(conn, &event)?;
    }
    Ok(())
}

pub fn run_id_for_capability(conn: &Connection, capability_id: &str) -> Result<Option<String>> {
    conn.query_row(
        "SELECT run_id FROM secret_capabilities WHERE id=?1",
        [capability_id],
        |row| row.get(0),
    )
    .optional()
    .map_err(Into::into)
}

/// Read the durable provider usage for `run_id` without consulting the live
/// capability store. Exhausted, expired, and revoked rows remain evidence and
/// are included alongside active rows. No secret-bearing columns are selected.
pub(crate) fn provider_usage_receipt(
    conn: &Connection,
    run_id: &str,
) -> Result<Option<ProviderUsageReceipt>> {
    let mut statement = conn.prepare(
        "SELECT id, provider, status, used_calls, used_input_tokens,
                used_output_tokens, used_cost_usd_micros, used_cost_known
         FROM secret_capabilities
         WHERE run_id=?1
         ORDER BY id ASC",
    )?;
    let rows = statement
        .query_map([run_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if rows.is_empty() {
        return Ok(None);
    }

    let mut capabilities = Vec::with_capacity(rows.len());
    let mut calls = 0u64;
    let mut input_tokens = 0u64;
    let mut output_tokens = 0u64;
    let mut cost_usd_micros = 0u64;
    let mut cost_known = true;
    for (id, provider, status, row_calls, input, output, cost, known) in rows {
        CapabilityStatus::try_from(status.as_str())?;
        let row_calls = u64::try_from(row_calls)
            .map_err(|_| anyhow!("capability {id} has negative used_calls"))?;
        let input = u64::try_from(input)
            .map_err(|_| anyhow!("capability {id} has negative used_input_tokens"))?;
        let output = u64::try_from(output)
            .map_err(|_| anyhow!("capability {id} has negative used_output_tokens"))?;
        let cost = u64::try_from(cost)
            .map_err(|_| anyhow!("capability {id} has negative used_cost_usd_micros"))?;
        calls = calls
            .checked_add(row_calls)
            .ok_or_else(|| anyhow!("capability usage calls overflow for run {run_id}"))?;
        input_tokens = input_tokens
            .checked_add(input)
            .ok_or_else(|| anyhow!("capability input tokens overflow for run {run_id}"))?;
        output_tokens = output_tokens
            .checked_add(output)
            .ok_or_else(|| anyhow!("capability output tokens overflow for run {run_id}"))?;
        cost_usd_micros = cost_usd_micros
            .checked_add(cost)
            .ok_or_else(|| anyhow!("capability cost overflow for run {run_id}"))?;
        cost_known &= known == 1;
        capabilities.push(ProviderUsageCapability {
            capability_id: id,
            provider,
            status,
        });
    }
    let cost_usd_micros = cost_known.then_some(cost_usd_micros);
    let canonical = CanonicalProviderUsageReceipt {
        schema_version: PROVIDER_USAGE_RECEIPT_SCHEMA,
        run_id,
        capabilities: &capabilities,
        calls,
        input_tokens,
        output_tokens,
        cost_usd_micros,
    };
    let canonical = serde_json::to_vec(&canonical)?;
    let digest = format!("sha256:{:x}", Sha256::digest(canonical));
    Ok(Some(ProviderUsageReceipt {
        schema_version: PROVIDER_USAGE_RECEIPT_SCHEMA.into(),
        run_id: run_id.into(),
        capabilities,
        calls,
        input_tokens,
        output_tokens,
        cost_usd: cost_usd_micros.map(|micros| micros as f64 / 1_000_000.0),
        digest,
    }))
}

/// Revoke every live capability for `run_id`, returning the revoked
/// capability ids so settlement can journal exactly what it revoked.
pub fn revoke_run(conn: &Connection, store: &CapabilityStore, run_id: &str) -> Result<Vec<String>> {
    let rows = {
        let mut stmt = conn.prepare(
            "SELECT id, handle FROM secret_capabilities
             WHERE run_id=?1 AND status IN ('granted','active')",
        )?;
        let rows = stmt
            .query_map([run_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    let (ids, handles): (Vec<String>, Vec<String>) = rows.into_iter().unzip();
    conn.execute(
        "UPDATE secret_capabilities SET status='revoked', revoked_at=?1
         WHERE run_id=?2 AND status IN ('granted','active')",
        params![Utc::now().to_rfc3339(), run_id],
    )?;
    conn.execute(
        "UPDATE secret_capabilities SET revoked_at=NULL
         WHERE run_id=?1 AND status IN ('exhausted','expired')",
        [run_id],
    )?;
    for handle in &handles {
        store.revoke_handle(handle);
    }
    if !handles.is_empty() {
        let mut event = SecretAuditEvent::new("run", run_id, "capability.revoke", "revoked");
        event.detail = Some("run ended".into());
        audit::append(conn, &event)?;
    }
    Ok(ids)
}

pub fn summary_from_live(
    live: &LiveCapability,
    display_suffix: Option<String>,
) -> CapabilitySummary {
    CapabilitySummary {
        id: live.id.clone(),
        secret_id: live.secret_id.clone(),
        run_id: live.run_id.clone(),
        recipe_id: live.recipe_id.clone(),
        provider: live.provider.clone(),
        status: live.status.clone(),
        max_calls: live.max_calls,
        used_calls: live.used_calls,
        max_cost_usd: live.max_cost_usd(),
        used_cost_usd: live.cost_usd(),
        used_input_tokens: live.used_input_tokens,
        used_output_tokens: live.used_output_tokens,
        expires_at: chrono::DateTime::from_timestamp_millis(live.expires_at_ms)
            .map(|value| value.to_rfc3339())
            .unwrap_or_default(),
        display_suffix,
    }
}

pub fn authorize_request(
    live: &LiveCapability,
    operation: &str,
    model: Option<&str>,
    effort: Option<&str>,
) -> Result<()> {
    if !live.operations.is_empty()
        && !live
            .operations
            .iter()
            .any(|allowed| allowed.eq_ignore_ascii_case(operation))
    {
        anyhow::bail!("operation {operation} is not allowed for this capability");
    }
    if let Some(model) = model {
        if !live.models.is_empty()
            && !live.models.iter().any(|allowed| {
                allowed.eq_ignore_ascii_case(model)
                    || (live.provider.eq_ignore_ascii_case("openrouter")
                        && !model.contains('/')
                        && allowed
                            .strip_prefix("openai/")
                            .is_some_and(|bare| bare.eq_ignore_ascii_case(model)))
            })
        {
            anyhow::bail!("model {model} is not allowed for this capability");
        }
    }
    if let Some(effort) = effort {
        if !live.reasoning_efforts.is_empty()
            && !live
                .reasoning_efforts
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(effort))
        {
            anyhow::bail!("reasoning effort {effort} is not allowed for this capability");
        }
    }
    Ok(())
}

pub fn persist_status(conn: &Connection, live: &LiveCapability) -> Result<()> {
    persist_usage(conn, live)
}

/// Shared store used by the proxy and the issuer.
pub type SharedCapabilityStore = Arc<CapabilityStore>;

#[cfg(test)]
mod tests {
    use super::*;

    fn live() -> LiveCapability {
        LiveCapability {
            id: "cap-1".into(),
            handle: "handle-1".into(),
            secret_id: "secret-1".into(),
            run_id: "run-1".into(),
            recipe_id: "recipe-1".into(),
            provider: "provider-1".into(),
            operations: Vec::new(),
            models: Vec::new(),
            reasoning_efforts: Vec::new(),
            max_calls: 5,
            max_input_tokens: 1_000,
            max_output_tokens: 1_000,
            max_cost_usd_micros: 1_000_000,
            used_calls: 0,
            used_input_tokens: 0,
            used_output_tokens: 0,
            used_cost_usd_micros: Some(0),
            status: "granted".into(),
            expires_at_ms: Utc::now().timestamp_millis() + 60_000,
        }
    }

    #[test]
    fn reported_zero_and_unknown_cost_are_never_interchangeable() {
        let store = CapabilityStore::new();
        store.insert(live());
        assert_eq!(
            summary_from_live(&store.lookup("handle-1").unwrap(), None).used_cost_usd,
            Some(0.0),
            "a newly granted capability has a genuine known zero"
        );
        let unknown = store
            .debit_usage(
                "handle-1",
                &MeasuredUsage {
                    calls: 1,
                    input_tokens: 10,
                    output_tokens: 2,
                    cost_usd: None,
                },
            )
            .unwrap();
        assert_eq!(summary_from_live(&unknown, None).used_cost_usd, None);
        let still_unknown = store
            .debit_usage(
                "handle-1",
                &MeasuredUsage {
                    calls: 1,
                    input_tokens: 10,
                    output_tokens: 2,
                    cost_usd: Some(0.25),
                },
            )
            .unwrap();
        assert_eq!(summary_from_live(&still_unknown, None).used_cost_usd, None);
    }

    #[test]
    fn request_pacer_does_not_stack_waiters_into_future_windows() {
        let store = CapabilityStore::new();
        let now = Instant::now();
        let interval = Duration::from_secs(6);

        assert_eq!(
            store.request_start_delay_at("wcap_test", now, interval),
            Duration::ZERO
        );
        assert_eq!(
            store.request_start_delay_at("wcap_test", now, interval),
            interval
        );
        assert_eq!(
            store.request_start_delay_at("wcap_test", now + interval, interval),
            Duration::ZERO
        );
        assert_eq!(
            store.request_start_delay_at("wcap_other", now, interval),
            Duration::ZERO
        );
    }
}
