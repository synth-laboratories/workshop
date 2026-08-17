//! `run_progress_history.v1` — the shape a finished run traced through its own
//! work, sealed into the run's summary so later runs of the same recipe can be
//! estimated from it.
//!
//! ## Why this exists
//!
//! A run-progress card cannot tell you when a run will finish by watching how
//! fast work completes. Measured on ten real Banking77 GEPA runs, rollouts
//! arrive in bursts 13ms apart with proposer gaps of up to 150 seconds between
//! them, and extrapolating from rollout throughput missed the true remaining
//! time by a median of 4.7× — see `runtime/runProgress/eta.ts` in the renderer.
//!
//! What does work is the recipe's own history. Leave-one-out over those ten
//! runs, predicting remaining time at nineteen points in each run:
//!
//! | estimator                        | median error | p90  | median absolute |
//! |----------------------------------|--------------|------|-----------------|
//! | prior (median total) alone       | 100%         | 389% | —               |
//! | rollout throughput               | 74%          | 594% | —               |
//! | elapsed ÷ progress               | 205%         | 520% | 79s             |
//! | **prior + this progress curve**  | **30%**      | 57%  | **10s**         |
//!
//! Every cheap approximation of the curve was tried and rejected: `elapsed / fᵃ`
//! for a fitted exponent lands at 85% median error at best, because progress is
//! not a power law. GEPA burns its seed rollouts in bursts and then stalls on
//! proposer calls, so only a measured curve captures the shape.
//!
//! ## What is sealed
//!
//! Nineteen numbers: the fraction of the run's wall time that had elapsed when
//! each 5% of its work had completed. Plus the totals needed to decide whether a
//! later run is comparable at all. It goes into the existing free-form
//! `summary_json`, so there is no migration.
//!
//! Only a *completed* run seals a curve. A failed or cancelled run stopped early
//! for a reason that says nothing about how long a healthy run takes, and
//! averaging one in would drag every future estimate toward the failure.

use super::models::{OptimizerEventEnvelope, OptimizerRunRecord};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const PROGRESS_HISTORY_SCHEMA: &str = "run_progress_history.v1";

/// Progress is sampled at 5% intervals: 5%, 10%, … 95%.
pub const CURVE_POINTS: usize = 19;

/// The summary key the curve is sealed under.
pub const SUMMARY_KEY: &str = "progressHistory";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProgressHistory {
    pub schema_version: String,
    /// What was counted: `rollouts`, `trials`, `steps`.
    pub unit: String,
    /// Units the run completed in total. Comparability is judged on this.
    pub total_units: u64,
    /// Wall time from first to last counted completion.
    pub wall_time_ms: u64,
    /// Elapsed fraction (0–1) at each 5% of unit progress. `CURVE_POINTS` long.
    pub curve: Vec<f64>,
}

/// The completion event types that count as one unit of work, per algorithm.
///
/// A producer may emit its completion twice — a real GEPA run emits
/// `optimizer.evaluation_result.received` 480 times for 240 rollouts — so the
/// caller de-duplicates on the unit id before building a curve.
fn completion_types(algorithm_id: &str) -> &'static [&'static str] {
    match algorithm_id {
        "gepa" => &["optimizer.evaluation_result.received"],
        "eval" => &["eval.trial.terminal"],
        "sft" => &["sft.checkpoint_rollout.completed"],
        _ => &[],
    }
}

fn unit_for(algorithm_id: &str) -> &'static str {
    match algorithm_id {
        "eval" => "trials",
        "sft" => "rollouts",
        _ => "rollouts",
    }
}

/// The delta fields that identify one unit of work.
const UNIT_ID_KEYS: &[&str] = &["rollout_id", "rolloutId", "trial_id", "trialId"];

fn unit_id(event: &OptimizerEventEnvelope) -> Option<String> {
    for key in UNIT_ID_KEYS {
        if let Some(value) = event.delta.get(*key).and_then(Value::as_str) {
            return Some(value.to_string());
        }
    }
    // `eval.trial.terminal` carries its identity on the item, not the delta.
    event
        .item
        .as_ref()
        .and_then(|item| item.get("id"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn parse_ms(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|parsed| parsed.timestamp_millis())
}

/// Build the curve a completed run traced, or `None` when the run cannot teach
/// anything: too little work, or no measurable span.
///
/// `MIN_UNITS` is deliberately low. A curve from a short run is still a curve;
/// what protects a later estimate is the requirement that several *comparable*
/// runs agree, which the renderer enforces.
pub fn build(
    run: &OptimizerRunRecord,
    events: &[OptimizerEventEnvelope],
) -> Option<ProgressHistory> {
    const MIN_UNITS: usize = 8;
    if run.status != "completed" {
        return None;
    }
    let wanted = completion_types(&run.algorithm_id);
    if wanted.is_empty() {
        return None;
    }
    let mut seen: Vec<String> = Vec::new();
    let mut completions: Vec<i64> = Vec::new();
    for event in events {
        if !wanted.contains(&event.event_type.as_str()) {
            continue;
        }
        let at = match parse_ms(&event.occurred_at) {
            Some(at) => at,
            None => continue,
        };
        if let Some(id) = unit_id(event) {
            if seen.contains(&id) {
                continue;
            }
            seen.push(id);
        }
        completions.push(at);
    }
    completions.sort_unstable();
    if completions.len() < MIN_UNITS {
        return None;
    }
    let first = *completions.first()?;
    let last = *completions.last()?;
    let span = last - first;
    if span <= 0 {
        return None;
    }
    let total = completions.len();
    let curve = (1..=CURVE_POINTS)
        .map(|step| {
            let fraction = step as f64 / (CURVE_POINTS + 1) as f64;
            // The completion that carried progress past this fraction.
            let index = ((fraction * total as f64).ceil() as usize).clamp(1, total) - 1;
            (completions[index] - first) as f64 / span as f64
        })
        .collect();
    Some(ProgressHistory {
        schema_version: PROGRESS_HISTORY_SCHEMA.to_string(),
        unit: unit_for(&run.algorithm_id).to_string(),
        total_units: total as u64,
        wall_time_ms: span as u64,
        curve,
    })
}

/// Seal the curve into the run's summary. A run that cannot teach anything, or
/// that already carries a curve for the same work, is left alone.
pub fn seal(run: &mut OptimizerRunRecord, events: &[OptimizerEventEnvelope]) -> bool {
    let history = match build(run, events) {
        Some(history) => history,
        None => return false,
    };
    let existing = run
        .summary
        .get(SUMMARY_KEY)
        .and_then(|value| serde_json::from_value::<ProgressHistory>(value.clone()).ok());
    if existing
        .as_ref()
        .is_some_and(|previous| previous.total_units == history.total_units)
    {
        return false;
    }
    let mut summary = run.summary.as_object().cloned().unwrap_or_default();
    summary.insert(SUMMARY_KEY.into(), json!(history));
    run.summary = Value::Object(summary);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Map;

    fn run(algorithm_id: &str, status: &str) -> OptimizerRunRecord {
        OptimizerRunRecord {
            schema_version: super::super::models::OPTIMIZER_RUN_SCHEMA_VERSION.into(),
            id: "run-a".into(),
            algorithm_id: algorithm_id.into(),
            algorithm_version: None,
            status: status.into(),
            source: "local".into(),
            objective: None,
            project_ref: None,
            session_ref: None,
            created_at: "2026-08-17T12:00:00Z".into(),
            started_at: Some("2026-08-17T12:00:00Z".into()),
            finished_at: Some("2026-08-17T12:10:00Z".into()),
            cursor_seq: 0,
            capabilities: Default::default(),
            execution_bindings: Vec::new(),
            input_refs: Vec::new(),
            output_refs: Vec::new(),
            visual_refs: Vec::new(),
            summary: json!({}),
            usage: Default::default(),
            error: None,
        }
    }

    fn completion(sequence: u64, second: i64, rollout: &str) -> OptimizerEventEnvelope {
        let mut delta = Map::new();
        delta.insert("rollout_id".into(), json!(rollout));
        OptimizerEventEnvelope {
            schema_version: super::super::models::OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
            event_id: None,
            event_type: "optimizer.evaluation_result.received".into(),
            sequence_number: sequence,
            occurred_at: format!("2026-08-17T12:{:02}:{:02}Z", second / 60, second % 60),
            optimizer_run_id: "run-a".into(),
            algorithm_id: "gepa".into(),
            level: None,
            item: None,
            delta,
            snapshot: None,
            usage_delta: None,
            artifact_refs: Vec::new(),
            error: None,
            raw: json!({}),
        }
    }

    /// One rollout per second, twenty of them, each reported twice.
    fn steady_events() -> Vec<OptimizerEventEnvelope> {
        let mut events = Vec::new();
        let mut sequence = 0;
        for index in 0..20 {
            for _ in 0..2 {
                sequence += 1;
                events.push(completion(sequence, index, &format!("rollout_{index}")));
            }
        }
        events
    }

    #[test]
    fn a_steady_run_traces_a_straight_curve() {
        let mut record = run("gepa", "completed");
        let history = build(&record, &steady_events()).expect("a completed run seals a curve");
        assert_eq!(history.schema_version, PROGRESS_HISTORY_SCHEMA);
        assert_eq!(history.curve.len(), CURVE_POINTS);
        // Duplicate reports must not double the work: 20 rollouts, not 40.
        assert_eq!(history.total_units, 20);
        assert_eq!(history.wall_time_ms, 19_000);
        // Uniform work traces the identity line, within one sample of quantisation.
        for (index, value) in history.curve.iter().enumerate() {
            let expected = (index + 1) as f64 / (CURVE_POINTS + 1) as f64;
            assert!(
                (value - expected).abs() < 0.08,
                "point {index} was {value}, expected about {expected}"
            );
        }
        assert!(seal(&mut record, &steady_events()));
        assert!(record.summary.get(SUMMARY_KEY).is_some());
    }

    #[test]
    fn a_front_loaded_run_traces_a_curve_that_bends() {
        // Ten rollouts in the first second, ten more spread over two minutes:
        // the real GEPA shape, where a burst is followed by a proposer stall.
        let mut events = Vec::new();
        let mut sequence = 0;
        for index in 0..10 {
            sequence += 1;
            events.push(completion(sequence, 0, &format!("burst_{index}")));
        }
        for index in 0..10 {
            sequence += 1;
            events.push(completion(sequence, 12 * (index + 1), &format!("slow_{index}")));
        }
        let history = build(&run("gepa", "completed"), &events).expect("curve");
        // Half the work is done in the first moment, so the curve starts near zero
        // and only then climbs — which is exactly what a naive linear model misses.
        assert!(history.curve[0] < 0.05, "{:?}", history.curve);
        assert!(history.curve[CURVE_POINTS / 2] < 0.35, "{:?}", history.curve);
        assert!(history.curve[CURVE_POINTS - 1] > 0.8, "{:?}", history.curve);
    }

    #[test]
    fn only_a_completed_run_teaches_anything() {
        for status in ["failed", "cancelled", "running", "queued"] {
            assert!(
                build(&run("gepa", status), &steady_events()).is_none(),
                "a {status} run must not seal a curve"
            );
        }
    }

    #[test]
    fn a_run_with_too_little_work_seals_nothing() {
        let events: Vec<_> = (0..4)
            .map(|index| completion(index + 1, index as i64, &format!("r{index}")))
            .collect();
        assert!(build(&run("gepa", "completed"), &events).is_none());
    }

    #[test]
    fn an_algorithm_with_no_counted_unit_seals_nothing() {
        assert!(build(&run("go-ex", "completed"), &steady_events()).is_none());
        assert!(build(&run("dag.behavior", "completed"), &steady_events()).is_none());
    }

    #[test]
    fn resealing_the_same_work_is_a_no_op() {
        let mut record = run("gepa", "completed");
        assert!(seal(&mut record, &steady_events()));
        let first = record.summary.clone();
        assert!(!seal(&mut record, &steady_events()), "no second write");
        assert_eq!(record.summary, first);
    }

    #[test]
    fn a_curve_never_regresses() {
        let history = build(&run("gepa", "completed"), &steady_events()).expect("curve");
        for pair in history.curve.windows(2) {
            assert!(
                pair[1] >= pair[0],
                "progress cannot move backwards: {:?}",
                history.curve
            );
        }
        assert!(history.curve.iter().all(|value| (0.0..=1.0).contains(value)));
    }
}
