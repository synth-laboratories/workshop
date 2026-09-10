//! Consent state for product telemetry.
//!
//! Honest bookkeeping: `Unset` (never asked) is distinct from a recorded
//! choice, every choice pins the policy version it was made under, and a
//! policy-version bump re-triggers the ask. Telemetry consumes this state;
//! it does not own it — the same shape can lift out for other features.

use anyhow::Result;
use chrono::Utc;
use serde::Serialize;

use super::contract;
use super::store::TelemetryStore;

const CHOICE_KEY: &str = "telemetry.consent.choice";
const VERSION_KEY: &str = "telemetry.consent.version";
const AT_KEY: &str = "telemetry.consent.at";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ConsentChoice {
    Granted,
    Declined,
}

#[derive(Clone, Debug, PartialEq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum ConsentState {
    /// Never asked (or the stored record is unreadable — treated as unasked).
    Unset,
    Granted { version: String, at: String },
    Declined { version: String, at: String },
}

pub fn state(store: &TelemetryStore) -> Result<ConsentState> {
    let choice = store.read_setting(CHOICE_KEY)?;
    let version = store.read_setting(VERSION_KEY)?;
    let at = store.read_setting(AT_KEY)?;
    Ok(match (choice.as_deref(), version, at) {
        (Some("granted"), Some(version), Some(at)) => ConsentState::Granted { version, at },
        (Some("declined"), Some(version), Some(at)) => ConsentState::Declined { version, at },
        _ => ConsentState::Unset,
    })
}

/// Whether the consent ask must be shown: never answered, or answered under
/// an older collection policy.
pub fn needs_ask(state: &ConsentState) -> bool {
    match state {
        ConsentState::Unset => true,
        ConsentState::Granted { version, .. } | ConsentState::Declined { version, .. } => {
            version != contract::collection_policy_version()
        }
    }
}

/// Whether the flusher may ship sync-eligible events right now.
pub fn sync_allowed(state: &ConsentState) -> bool {
    matches!(
        state,
        ConsentState::Granted { version, .. }
            if version == contract::collection_policy_version()
    )
}

pub fn record_choice(store: &TelemetryStore, choice: ConsentChoice) -> Result<ConsentState> {
    store.write_setting(
        CHOICE_KEY,
        match choice {
            ConsentChoice::Granted => "granted",
            ConsentChoice::Declined => "declined",
        },
    )?;
    store.write_setting(VERSION_KEY, contract::collection_policy_version())?;
    store.write_setting(AT_KEY, &Utc::now().to_rfc3339())?;
    state(store)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unset_needs_ask_and_never_syncs() {
        assert!(needs_ask(&ConsentState::Unset));
        assert!(!sync_allowed(&ConsentState::Unset));
    }

    #[test]
    fn stale_policy_version_reasks_and_stops_sync() {
        let stale = ConsentState::Granted {
            version: "workshop.product-telemetry.policy.v1".into(),
            at: "2026-01-01T00:00:00Z".into(),
        };
        assert!(needs_ask(&stale));
        assert!(!sync_allowed(&stale));
        let current = ConsentState::Granted {
            version: contract::collection_policy_version().into(),
            at: "2026-08-28T00:00:00Z".into(),
        };
        assert!(!needs_ask(&current));
        assert!(sync_allowed(&current));
    }

    #[test]
    fn declined_never_syncs_even_when_current() {
        let declined = ConsentState::Declined {
            version: contract::collection_policy_version().into(),
            at: "2026-08-28T00:00:00Z".into(),
        };
        assert!(!needs_ask(&declined));
        assert!(!sync_allowed(&declined));
    }
}
