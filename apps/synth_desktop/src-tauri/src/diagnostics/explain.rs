//! `diagnostics_explain` — the causal neighborhood around a set of identities.
//!
//! This is the operation the whole system exists for: one typed call with a
//! visual id (or a task, rollout, stream, or trace id) that comes back with the
//! failure, what it was correlated with, which of those was the cause and which
//! were symptoms, and what to do about it.
//!
//! It is deterministic. No model call is involved, and the same inputs over the
//! same journal always produce the same answer — an explanation you cannot
//! reproduce is not evidence.

use super::codes;
use super::event::{Correlation, Severity, CORRELATION_FIELDS};
use super::store::{group_by_code, DiagnosticRecord};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// One hop of identity expansion. A visual id finds its projection failure,
/// that failure names a rollout, and the rollout's own errors come back in the
/// same answer — without letting the neighborhood grow without bound.
pub const EXPANSION_HOPS: usize = 1;

/// Correlated identities discovered while explaining.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IdentitySet {
    pub values: BTreeMap<String, BTreeSet<String>>,
}

impl IdentitySet {
    pub fn from_correlation(correlation: &Correlation) -> Self {
        let mut set = Self::default();
        for (field, value) in correlation.present() {
            set.insert(field, value);
        }
        set
    }

    pub fn insert(&mut self, field: &str, value: String) {
        self.values
            .entry(field.to_owned())
            .or_default()
            .insert(value);
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Every `(field, value)` pair, so the caller can run one query per pair.
    pub fn pairs(&self) -> Vec<(String, String)> {
        self.values
            .iter()
            .flat_map(|(field, values)| {
                values
                    .iter()
                    .map(move |value| (field.clone(), value.clone()))
            })
            .collect()
    }

    /// Identities named by the records themselves — the next hop.
    pub fn absorb(&mut self, records: &[DiagnosticRecord]) -> usize {
        let before = self.pairs().len();
        for record in records {
            for field in CORRELATION_FIELDS {
                if let Some(value) = record.event.correlation.get(field) {
                    self.insert(field, value.to_owned());
                }
            }
        }
        self.pairs().len() - before
    }

    pub fn to_json(&self) -> Value {
        Value::Object(
            self.values
                .iter()
                .map(|(field, values)| {
                    (
                        field.clone(),
                        Value::Array(values.iter().cloned().map(Value::String).collect()),
                    )
                })
                .collect(),
        )
    }
}

/// Order a correlated result set into cause and symptoms.
///
/// Rank first, then time: the earliest event of the most upstream rank is the
/// cause. Sorting on time alone would nominate whichever surface happened to
/// notice first, which for the failure that motivated this system is the
/// renderer — the one component guaranteed *not* to be the cause.
pub fn build(records: &[DiagnosticRecord], identities: &IdentitySet) -> Value {
    let mut errors: Vec<&DiagnosticRecord> = records
        .iter()
        .filter(|record| record.event.severity >= Severity::Warn)
        .collect();
    errors.sort_by(|left, right| {
        codes::rank(&left.event.code)
            .cmp(&codes::rank(&right.event.code))
            .then_with(|| left.event.timestamp.cmp(&right.event.timestamp))
            .then_with(|| left.sequence.cmp(&right.sequence))
    });

    let cause = errors.first().copied();
    let symptoms: Vec<Value> = errors
        .iter()
        .skip(1)
        .take(20)
        .map(|record| summarize(record))
        .collect();

    let remediation = cause.and_then(|record| {
        record
            .event
            .details
            .get("remediation")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| codes::remediation(&record.event.code).map(str::to_owned))
    });

    json!({
        "identities": identities.to_json(),
        "matched": records.len(),
        "cause": cause.map(summarize),
        "symptoms": symptoms,
        "groups": group_by_code(records),
        "remediation": remediation,
        "evidence": records
            .iter()
            .take(50)
            .map(DiagnosticRecord::to_json)
            .collect::<Vec<_>>(),
    })
}

fn summarize(record: &DiagnosticRecord) -> Value {
    json!({
        "journal_sequence": record.sequence,
        "event_id": record.event.event_id,
        "timestamp": record.event.timestamp,
        "severity": record.event.severity.as_str(),
        "component": record.event.component,
        "event": record.event.event,
        "code": record.event.code,
        "message": record.event.message,
        "retryable": record.event.retryable,
        "rank": codes::rank(&record.event.code),
        "correlation": record.event.correlation.present(),
        "details": record.event.details,
    })
}

