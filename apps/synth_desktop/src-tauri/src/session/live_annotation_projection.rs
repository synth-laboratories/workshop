//! Provisional live-annotation findings and their post-hoc reconciliation.
//!
//! Lane C streams provisional findings while a rollout runs; they are relayed
//! into the run journal as `eval.trial.event` carriers tagged
//! `delta.stream = "annotation"`. This projection folds those rows per rollout
//! (supersede / retract history intact) and, once the run is sealed, checks
//! each finding's citations against the rollout's own relayed journal -- the
//! evidence the sealed Trace V5 was built from and the relay verified
//! contiguous and closed -- and against the sealed post-hoc findings for the
//! same trace.
//!
//! Reconciliation states:
//!
//! * `resolved` -- every cited rollout sequence exists in the verified journal;
//! * `corroborated` -- resolved, and a sealed post-hoc finding shares the label;
//! * `unresolved` -- a cited sequence is missing from the verified journal
//!   (the citation cannot be trusted);
//! * `unsealed` -- the rollout's journal never closed, so nothing can be
//!   checked against.
//!
//! Provisional rows never become sealed evidence here; they stay in their own
//! table and their own status vocabulary.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Map, Value};

use crate::optimizers::OptimizerEventEnvelope;

pub const PROVISIONAL_SCHEMA: &str = "synth.live-annotation-provisional.v1";

#[derive(Clone, Debug, PartialEq)]
pub struct ProvisionalFinding {
    pub rollout_id: String,
    pub trial_id: Option<String>,
    pub sequence: u64,
    pub finding_id: String,
    pub kind: String,
    pub label: String,
    /// `provisional`, `superseded`, or `retracted` -- the live vocabulary.
    pub status: String,
    pub step: Option<u64>,
    pub confidence: Option<f64>,
    pub protocol_revision_id: Option<String>,
    pub cited_sequences: Vec<u64>,
    pub supersedes: Option<String>,
    pub superseded_by: Option<String>,
    pub retracted_reason: Option<String>,
    pub basis: Option<String>,
    pub occurred_at: Option<String>,
    pub payload: Value,
}

#[derive(Clone, Debug, Default)]
pub struct RolloutFold {
    pub findings: Vec<ProvisionalFinding>,
    /// Rollout-stream sequences the relay durably holds for this rollout.
    pub journal_sequences: BTreeSet<u64>,
    /// The relayed annotation stream reported `capture.closed`.
    pub annotation_closed: bool,
    pub annotation_outcome: Option<String>,
    pub protocol_revision_id: Option<String>,
    pub trial_id: Option<String>,
}

fn container_event(envelope: &OptimizerEventEnvelope) -> Option<&Map<String, Value>> {
    if envelope.event_type != "eval.trial.event" {
        return None;
    }
    envelope
        .delta
        .get("container_event")
        .and_then(Value::as_object)
}

fn is_annotation_row(envelope: &OptimizerEventEnvelope, event: &Map<String, Value>) -> bool {
    envelope.delta.get("stream").and_then(Value::as_str) == Some("annotation")
        || event
            .get("stream_id")
            .and_then(Value::as_str)
            .is_some_and(|id| id.ends_with(":annotations"))
        || event
            .get("kind")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind.starts_with("annotation."))
}

/// Fold a run's relayed events into per-rollout provisional findings and
/// journal coverage. Pure over the event list; order is the journal order.
pub fn fold_run_events(events: &[OptimizerEventEnvelope]) -> BTreeMap<String, RolloutFold> {
    let mut folds: BTreeMap<String, RolloutFold> = BTreeMap::new();
    for envelope in events {
        let Some(event) = container_event(envelope) else {
            continue;
        };
        let Some(rollout_id) = event.get("rollout_id").and_then(Value::as_str) else {
            continue;
        };
        let fold = folds.entry(rollout_id.to_string()).or_default();
        if fold.trial_id.is_none() {
            fold.trial_id = envelope
                .delta
                .get("trial_id")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        let sequence = event.get("sequence").and_then(Value::as_u64).unwrap_or(0);
        let kind = event.get("kind").and_then(Value::as_str).unwrap_or("");
        let payload = event.get("payload").and_then(Value::as_object);
        if !is_annotation_row(envelope, event) {
            if sequence > 0 {
                fold.journal_sequences.insert(sequence);
            }
            continue;
        }
        let Some(payload) = payload else { continue };
        match kind {
            "annotation.protocol.bound" | "annotation.protocol.rebound" => {
                fold.protocol_revision_id = payload
                    .get("protocol_revision_id")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            "annotation.finding" => {
                let finding_id = payload
                    .get("finding_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("finding:{sequence}"));
                let supersedes = payload
                    .get("supersedes")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                if let Some(previous) = supersedes.as_deref() {
                    if let Some(row) = fold
                        .findings
                        .iter_mut()
                        .find(|row| row.finding_id == previous && row.status == "provisional")
                    {
                        row.status = "superseded".into();
                        row.superseded_by = Some(finding_id.clone());
                    }
                }
                let cited = payload
                    .get("evidence")
                    .and_then(|evidence| evidence.get("sequences"))
                    .and_then(Value::as_array)
                    .map(|rows| rows.iter().filter_map(Value::as_u64).collect::<Vec<_>>())
                    .unwrap_or_default();
                fold.findings.push(ProvisionalFinding {
                    rollout_id: rollout_id.to_string(),
                    trial_id: fold.trial_id.clone(),
                    sequence,
                    finding_id,
                    kind: payload
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or("note")
                        .to_string(),
                    label: payload
                        .get("label")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string(),
                    status: "provisional".into(),
                    step: payload.get("step").and_then(Value::as_u64),
                    confidence: payload.get("confidence").and_then(Value::as_f64),
                    protocol_revision_id: payload
                        .get("protocol_revision_id")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    cited_sequences: cited,
                    supersedes,
                    superseded_by: None,
                    retracted_reason: None,
                    basis: payload
                        .get("detail")
                        .and_then(|detail| detail.get("basis"))
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    occurred_at: event
                        .get("occurred_at")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    payload: Value::Object(payload.clone()),
                });
            }
            "annotation.finding.retracted" => {
                let target = payload.get("finding_id").and_then(Value::as_str);
                if let Some(row) = fold
                    .findings
                    .iter_mut()
                    .find(|row| Some(row.finding_id.as_str()) == target)
                {
                    row.status = "retracted".into();
                    row.retracted_reason = payload
                        .get("reason")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                }
            }
            "annotation.closed" => {
                fold.annotation_outcome = payload
                    .get("outcome")
                    .and_then(Value::as_str)
                    .map(str::to_string);
            }
            "capture.closed" => fold.annotation_closed = true,
            _ => {}
        }
    }
    folds
}

/// One rollout's sealed context, from the trial record: which local trace
/// digest holds its seal, and whether the relay verified journal closure.
#[derive(Clone, Debug, Default)]
pub struct SealContext {
    pub trace_digest: Option<String>,
    pub journal_closed: bool,
    pub journal_high_water: u64,
}

pub fn seal_contexts(records: &[Value]) -> BTreeMap<String, SealContext> {
    let mut out = BTreeMap::new();
    for record in records {
        let Some(rollout_id) = record.get("rolloutId").and_then(Value::as_str) else {
            continue;
        };
        let trace_digest = record
            .pointer("/sealedTrace/traces")
            .and_then(Value::as_array)
            .and_then(|traces| traces.first())
            .and_then(|trace| trace.get("digest"))
            .and_then(Value::as_str)
            .filter(|digest| !digest.is_empty())
            .map(str::to_string);
        let relay = record.get("relay").cloned().unwrap_or(Value::Null);
        out.insert(
            rollout_id.to_string(),
            SealContext {
                trace_digest,
                journal_closed: relay
                    .get("journalClosed")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                journal_high_water: relay
                    .get("highWater")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            },
        );
    }
    out
}

/// Decide one finding's reconciliation against the verified journal and the
/// sealed post-hoc labels for the same trace.
pub fn reconcile_one(
    finding: &ProvisionalFinding,
    fold: &RolloutFold,
    seal: &SealContext,
    sealed_labels: &BTreeSet<String>,
) -> &'static str {
    if !seal.journal_closed {
        return "unsealed";
    }
    let resolved = finding.cited_sequences.iter().all(|sequence| {
        *sequence <= seal.journal_high_water && fold.journal_sequences.contains(sequence)
    });
    if !resolved {
        return "unresolved";
    }
    if sealed_labels.contains(&finding.label) || sealed_labels.contains(&format!("{}.{}", finding.kind, finding.label)) {
        return "corroborated";
    }
    "resolved"
}

pub fn sealed_labels_for_trace(conn: &Connection, trace_digest: &str) -> Result<BTreeSet<String>> {
    let rows = super::annotation_projection::list_findings_for_trace(conn, trace_digest)?;
    let mut labels = BTreeSet::new();
    for row in rows {
        if row.get("status").and_then(Value::as_str) != Some("applied") {
            continue;
        }
        if let Some(label) = row.get("label").and_then(Value::as_str) {
            labels.insert(label.to_string());
        }
        // Milestone-shaped sealed payloads name the milestone they verified.
        for key in ["milestone_id", "milestone", "achievement", "label"] {
            if let Some(value) = row.pointer(&format!("/payload/{key}")).and_then(Value::as_str) {
                labels.insert(value.to_string());
            }
        }
    }
    Ok(labels)
}

/// Replace the run's provisional rows with the reconciled fold. Idempotent.
pub fn replace_run_rows(
    conn: &Connection,
    run_id: &str,
    folds: &BTreeMap<String, RolloutFold>,
    seals: &BTreeMap<String, SealContext>,
) -> Result<Value> {
    conn.execute(
        "DELETE FROM annotation_provisional_findings WHERE run_id=?1",
        [run_id],
    )?;
    let now = chrono::Utc::now().to_rfc3339();
    let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
    let mut rollouts = 0usize;
    for (rollout_id, fold) in folds {
        rollouts += 1;
        let seal = seals.get(rollout_id).cloned().unwrap_or_default();
        let sealed_labels = match seal.trace_digest.as_deref() {
            Some(digest) => sealed_labels_for_trace(conn, digest)?,
            None => BTreeSet::new(),
        };
        for finding in &fold.findings {
            let reconciliation = reconcile_one(finding, fold, &seal, &sealed_labels);
            *counts.entry(reconciliation).or_default() += 1;
            *counts.entry(match finding.status.as_str() {
                "retracted" => "retracted",
                "superseded" => "superseded",
                _ => "active",
            }).or_default() += 1;
            conn.execute(
                "INSERT INTO annotation_provisional_findings(
                    run_id, rollout_id, trial_id, sequence, finding_id, kind, label, status,
                    step, confidence, protocol_revision_id, cited_sequences_json, supersedes,
                    superseded_by, retracted_reason, basis, payload_json, occurred_at,
                    reconciliation, reconciled_trace_digest, reconciled_at
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21)",
                params![
                    run_id,
                    rollout_id,
                    finding.trial_id,
                    finding.sequence as i64,
                    finding.finding_id,
                    finding.kind,
                    finding.label,
                    finding.status,
                    finding.step.map(|step| step as i64),
                    finding.confidence,
                    finding.protocol_revision_id,
                    serde_json::to_string(&finding.cited_sequences)?,
                    finding.supersedes,
                    finding.superseded_by,
                    finding.retracted_reason,
                    finding.basis,
                    finding.payload.to_string(),
                    finding.occurred_at,
                    reconciliation,
                    seal.trace_digest,
                    now,
                ],
            )?;
        }
    }
    let total: usize = folds.values().map(|fold| fold.findings.len()).sum();
    Ok(json!({
        "schema": PROVISIONAL_SCHEMA,
        "rollouts": rollouts,
        "findings": total,
        "resolved": counts.get("resolved").copied().unwrap_or(0),
        "corroborated": counts.get("corroborated").copied().unwrap_or(0),
        "unresolved": counts.get("unresolved").copied().unwrap_or(0),
        "unsealed": counts.get("unsealed").copied().unwrap_or(0),
        "active": counts.get("active").copied().unwrap_or(0),
        "superseded": counts.get("superseded").copied().unwrap_or(0),
        "retracted": counts.get("retracted").copied().unwrap_or(0),
        "reconciledAt": now,
    }))
}

pub fn list_run_rows(conn: &Connection, run_id: &str, rollout_id: Option<&str>) -> Result<Vec<Value>> {
    let mut statement = conn.prepare(
        "SELECT rollout_id, trial_id, sequence, finding_id, kind, label, status, step, confidence,
                protocol_revision_id, cited_sequences_json, supersedes, superseded_by, retracted_reason,
                basis, payload_json, occurred_at, reconciliation, reconciled_trace_digest, reconciled_at
         FROM annotation_provisional_findings
         WHERE run_id=?1 AND (?2 IS NULL OR rollout_id=?2)
         ORDER BY rollout_id, sequence",
    )?;
    let rows = statement
        .query_map(params![run_id, rollout_id], |row| {
            let cited: String = row.get(10)?;
            let payload: String = row.get(15)?;
            Ok(json!({
                "rolloutId": row.get::<_, String>(0)?,
                "trialId": row.get::<_, Option<String>>(1)?,
                "sequence": row.get::<_, i64>(2)?,
                "findingId": row.get::<_, String>(3)?,
                "kind": row.get::<_, String>(4)?,
                "label": row.get::<_, String>(5)?,
                "status": row.get::<_, String>(6)?,
                "step": row.get::<_, Option<i64>>(7)?,
                "confidence": row.get::<_, Option<f64>>(8)?,
                "protocolRevisionId": row.get::<_, Option<String>>(9)?,
                "citedSequences": serde_json::from_str::<Value>(&cited).unwrap_or(json!([])),
                "supersedes": row.get::<_, Option<String>>(11)?,
                "supersededBy": row.get::<_, Option<String>>(12)?,
                "retractedReason": row.get::<_, Option<String>>(13)?,
                "basis": row.get::<_, Option<String>>(14)?,
                "payload": serde_json::from_str::<Value>(&payload).unwrap_or(json!({})),
                "occurredAt": row.get::<_, Option<String>>(16)?,
                "reconciliation": row.get::<_, String>(17)?,
                "reconciledTraceDigest": row.get::<_, Option<String>>(18)?,
                "reconciledAt": row.get::<_, String>(19)?,
            }))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn run_summary(conn: &Connection, run_id: &str) -> Result<Option<Value>> {
    let row = conn
        .query_row(
            "SELECT COUNT(*),
                    SUM(CASE WHEN reconciliation='resolved' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN reconciliation='corroborated' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN reconciliation='unresolved' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN reconciliation='unsealed' THEN 1 ELSE 0 END),
                    SUM(CASE WHEN status='retracted' THEN 1 ELSE 0 END),
                    MAX(reconciled_at)
             FROM annotation_provisional_findings WHERE run_id=?1",
            [run_id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            },
        )
        .optional()?;
    Ok(row.filter(|row| row.0 > 0).map(|row| {
        json!({
            "schema": PROVISIONAL_SCHEMA,
            "findings": row.0,
            "resolved": row.1.unwrap_or(0),
            "corroborated": row.2.unwrap_or(0),
            "unresolved": row.3.unwrap_or(0),
            "unsealed": row.4.unwrap_or(0),
            "retracted": row.5.unwrap_or(0),
            "reconciledAt": row.6,
        })
    }))
}

/// Read every relayed event of a run, page by page.
pub async fn run_events(
    service: &crate::optimizers::OptimizerService,
    run_id: &str,
) -> Result<Vec<OptimizerEventEnvelope>> {
    let mut out = Vec::new();
    let mut after = 0u64;
    loop {
        let page = service
            .events_after(run_id.to_string(), after, Some(2000))
            .await
            .context("read relayed run events for live annotation reconciliation")?;
        let Some(last) = page.last() else { break };
        after = last.sequence_number;
        let count = page.len();
        out.extend(page);
        if count < 2000 {
            break;
        }
    }
    Ok(out)
}

/// Reconcile a sealed run: fold the relayed streams, check citations, and
/// replace the run's provisional rows. Returns the summary. Never touches
/// sealed evidence heads or findings.
pub async fn reconcile_run(
    service: &crate::optimizers::OptimizerService,
    database: &std::sync::Arc<crate::storage::Database>,
    run_id: &str,
    records: &[Value],
) -> Result<Value> {
    let events = run_events(service, run_id).await?;
    let folds = fold_run_events(&events);
    if folds.values().all(|fold| fold.findings.is_empty()) {
        return Ok(json!({"schema": PROVISIONAL_SCHEMA, "rollouts": folds.len(), "findings": 0}));
    }
    let seals = seal_contexts(records);
    let owned = run_id.to_string();
    database
        .run_transaction(move |conn| replace_run_rows(conn, &owned, &folds, &seals))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(sequence: u64, stream: Option<&str>, event: Value) -> OptimizerEventEnvelope {
        let mut delta = Map::new();
        delta.insert("trial_id".into(), json!("trial:craftax:0"));
        if let Some(stream) = stream {
            delta.insert("stream".into(), json!(stream));
        }
        delta.insert("container_event".into(), event);
        OptimizerEventEnvelope {
            schema_version: "optimizer_event.v1".into(),
            event_id: Some(format!("opt:eval:{sequence}")),
            event_type: "eval.trial.event".into(),
            sequence_number: sequence,
            occurred_at: "2026-09-01T00:00:00+00:00".into(),
            optimizer_run_id: "opt_eval_1".into(),
            algorithm_id: "eval".into(),
            level: Some("debug".into()),
            item: None,
            delta,
            snapshot: None,
            usage_delta: None,
            artifact_refs: vec![],
            error: None,
            raw: json!({}),
        }
    }

    fn rollout(seq: u64, kind: &str) -> Value {
        json!({"rollout_id": "roll_a", "sequence": seq, "kind": kind, "payload": {}})
    }
    fn annotation(seq: u64, kind: &str, payload: Value) -> Value {
        json!({"rollout_id": "roll_a", "stream_id": "stream:roll_a:annotations", "sequence": seq, "kind": kind, "payload": payload})
    }

    fn sample() -> Vec<OptimizerEventEnvelope> {
        vec![
            envelope(1, None, rollout(1, "trace.opened")),
            envelope(2, None, rollout(2, "observation")),
            envelope(3, Some("annotation"), annotation(1, "annotation.protocol.bound", json!({"protocol_revision_id": "anprev_a"}))),
            envelope(4, None, rollout(3, "action")),
            envelope(5, Some("annotation"), annotation(2, "annotation.finding", json!({"finding_id": "fm:1", "kind": "failure_mode", "label": "feedback_incorporation.repeated_blocked_action", "step": 1, "confidence": 0.5, "evidence": {"sequences": [3]}, "protocol_revision_id": "anprev_a"}))),
            envelope(6, Some("annotation"), annotation(3, "annotation.finding", json!({"finding_id": "fm:2", "kind": "failure_mode", "label": "feedback_incorporation.repeated_blocked_action", "supersedes": "fm:1", "evidence": {"sequences": [3, 4]}}))),
            envelope(7, None, rollout(4, "observation")),
            envelope(8, Some("annotation"), annotation(4, "annotation.finding", json!({"finding_id": "ach:collect_wood", "kind": "achievement", "label": "collect_wood", "step": 2, "evidence": {"sequences": [4]}, "detail": {"basis": "readout"}}))),
            envelope(9, Some("annotation"), annotation(5, "annotation.finding.retracted", json!({"finding_id": "fm:2", "reason": "progress resumed"}))),
            envelope(10, Some("annotation"), annotation(6, "annotation.finding", json!({"finding_id": "ghost", "kind": "note", "label": "cites the future", "evidence": {"sequences": [99]}}))),
            envelope(11, Some("annotation"), annotation(7, "annotation.closed", json!({"outcome": "completed"}))),
            envelope(12, Some("annotation"), annotation(8, "capture.closed", json!({"high_water": 7}))),
        ]
    }

    #[test]
    fn fold_keeps_history_and_journal_coverage() {
        let folds = fold_run_events(&sample());
        let fold = &folds["roll_a"];
        assert_eq!(fold.journal_sequences.iter().copied().collect::<Vec<_>>(), vec![1, 2, 3, 4]);
        assert_eq!(fold.protocol_revision_id.as_deref(), Some("anprev_a"));
        assert!(fold.annotation_closed);
        assert_eq!(fold.annotation_outcome.as_deref(), Some("completed"));
        let by_id: BTreeMap<_, _> = fold.findings.iter().map(|row| (row.finding_id.as_str(), row)).collect();
        assert_eq!(by_id["fm:1"].status, "superseded");
        assert_eq!(by_id["fm:1"].superseded_by.as_deref(), Some("fm:2"));
        assert_eq!(by_id["fm:2"].status, "retracted");
        assert_eq!(by_id["fm:2"].retracted_reason.as_deref(), Some("progress resumed"));
        assert_eq!(by_id["ach:collect_wood"].status, "provisional");
        assert_eq!(by_id["ach:collect_wood"].basis.as_deref(), Some("readout"));
        assert_eq!(by_id["ach:collect_wood"].trial_id.as_deref(), Some("trial:craftax:0"));
    }

    #[test]
    fn reconciliation_resolves_citations_against_the_verified_journal_only() {
        let folds = fold_run_events(&sample());
        let fold = &folds["roll_a"];
        let sealed = SealContext { trace_digest: Some("sha256:t".into()), journal_closed: true, journal_high_water: 4 };
        let labels: BTreeSet<String> = ["collect_wood".to_string()].into_iter().collect();
        let by_id: BTreeMap<_, _> = fold.findings.iter().map(|row| (row.finding_id.as_str(), row)).collect();
        assert_eq!(reconcile_one(by_id["fm:1"], fold, &sealed, &labels), "resolved");
        assert_eq!(reconcile_one(by_id["ach:collect_wood"], fold, &sealed, &labels), "corroborated");
        assert_eq!(reconcile_one(by_id["ghost"], fold, &sealed, &labels), "unresolved");
        let open = SealContext { journal_closed: false, ..sealed.clone() };
        assert_eq!(reconcile_one(by_id["fm:1"], fold, &open, &labels), "unsealed");
    }

    #[test]
    fn rows_round_trip_through_sqlite_and_summarize() {
        let conn = Connection::open_in_memory().unwrap();
        crate::storage::migrations::apply_migrations(&conn).unwrap();
        let folds = fold_run_events(&sample());
        let records = vec![json!({
            "rolloutId": "roll_a",
            "sealedTrace": {"traces": []},
            "relay": {"journalClosed": true, "highWater": 4},
        })];
        let seals = seal_contexts(&records);
        assert!(seals["roll_a"].journal_closed && seals["roll_a"].trace_digest.is_none());
        let summary = replace_run_rows(&conn, "opt_eval_1", &folds, &seals).unwrap();
        assert_eq!(summary["findings"], 4);
        assert_eq!(summary["resolved"], 3);
        assert_eq!(summary["unresolved"], 1);
        assert_eq!(summary["retracted"], 1);
        assert_eq!(summary["superseded"], 1);
        let rows = list_run_rows(&conn, "opt_eval_1", Some("roll_a")).unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0]["findingId"], "fm:1");
        assert_eq!(rows[0]["status"], "superseded");
        assert_eq!(rows[3]["reconciliation"], "unresolved");
        // Idempotent replace.
        let again = replace_run_rows(&conn, "opt_eval_1", &folds, &seals).unwrap();
        assert_eq!(again["findings"], 4);
        assert_eq!(list_run_rows(&conn, "opt_eval_1", None).unwrap().len(), 4);
        let summary = run_summary(&conn, "opt_eval_1").unwrap().unwrap();
        assert_eq!(summary["findings"], 4);
        assert_eq!(summary["unresolved"], 1);
        assert!(run_summary(&conn, "other").unwrap().is_none());
    }
}
