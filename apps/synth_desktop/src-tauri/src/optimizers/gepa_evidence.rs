//! What a GEPA run actually proved, reduced from its own durable event log.
//!
//! A completed Banking77 search used to settle with `work: {planned: null,
//! succeeded: null, ...}`, `selection: null`, and no usage lanes, because the
//! terminal manifest looked for rollout events GEPA never emits
//! (`rollout.completed`) and for a `selection` snapshot GEPA never writes. The
//! run had in fact spent 140 rollouts across four distinct stages, registered
//! two candidates, and measured one of them on heldout — every one of those
//! facts was already durable, and none of them reached a single surface.
//!
//! Worse than the missing numbers was the missing *verdict*. That run's winner
//! was the seed: its one proposal lost at the minibatch gate and was never
//! promoted. `get_result` still answered with a `selectedCandidate` carrying a
//! materialized prompt and `frontierMember: true`, which reads exactly like a
//! promotion. A search that found nothing must say it found nothing.
//!
//! This module is the single reduction. It reads:
//!
//!   · `optimizer.candidate_evaluation.allocated` — one per dispatched rollout,
//!     carrying `stage` and `candidate_id`, so seed / minibatch / full-train /
//!     heldout stay separate lanes instead of one "rollouts" total.
//!   · `optimizer.evaluation_result.received` — the scored result, deduplicated
//!     by `evaluation_id` because the producer emits a partial and a final for
//!     the same evaluation. It also carries `active_workers`, which is the only
//!     durable proof that rollouts ran concurrently.
//!   · `candidate.registered` — lineage: parent, generation, proposal index,
//!     source, and the candidate's own payload values.
//!   · `proposer.completed` — how many proposals the proposer actually returned,
//!     which is not the same number as the recipe requested.
//!   · `candidate.evaluated` / `heldout.completed` / `frontier.snapshot` — the
//!     scores the producer itself settled on.
//!   · `gepa.run.finished` — the producer's own per-lane usage roll-up.
//!
//! Nothing here fills a gap with zero. A stage with no results has no mean, not
//! a mean of 0.0, and a run whose proposer returned fewer candidates than the
//! recipe asked for reports both numbers rather than the one that flatters it.

use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};

use super::models::{OptimizerEventEnvelope, OptimizerRunRecord};

pub(super) const GEPA_EVIDENCE_SCHEMA: &str = "gepa_run_evidence.v1";

/// The rollout stages GEPA plans in. Kept as distinct lanes because they answer
/// different questions: a minibatch score gates promotion, a heldout score is
/// the only one allowed to support an uplift claim, and collapsing them lets
/// train evidence masquerade as a measured result.
pub(super) const STAGE_SEED_FULL_TRAIN: &str = "seed_full_train";
pub(super) const STAGE_PARENT_MINIBATCH: &str = "parent_minibatch_reference";
pub(super) const STAGE_CANDIDATE_MINIBATCH: &str = "candidate_minibatch";
pub(super) const STAGE_CANDIDATE_FULL_TRAIN: &str = "candidate_full_train";
pub(super) const STAGE_HELDOUT: &str = "heldout";

/// The four things a GEPA run is allowed to conclude.
///
/// `Failed` is a run that did not finish its search. `Inconclusive` finished but
/// never measured a candidate against the baseline on heldout — no comparison
/// exists, so no claim may be made. `NoMeasuredImprovement` made the comparison
/// and the challenger did not win. Only `MeasuredImprovement` may be reported as
/// a win, and only from heldout evidence with a sample count attached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Verdict {
    MeasuredImprovement,
    NoMeasuredImprovement,
    Inconclusive,
    Failed,
}

impl Verdict {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::MeasuredImprovement => "measured_improvement",
            Self::NoMeasuredImprovement => "no_measured_improvement",
            Self::Inconclusive => "inconclusive",
            Self::Failed => "failed",
        }
    }
}

/// Scores for one candidate in one stage. `mean` is `None` until at least one
/// result lands; `samples` is what any claim about that mean is worth.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct StageScore {
    pub samples: u64,
    pub sum: f64,
    pub scored: u64,
}

impl StageScore {
    fn mean(&self) -> Option<f64> {
        (self.scored > 0).then(|| self.sum / self.scored as f64)
    }

    fn to_value(&self) -> Value {
        json!({
            "samples": self.samples,
            "scored": self.scored,
            "mean": self.mean(),
        })
    }
}

/// One candidate and everything the log proves about it.
#[derive(Clone, Debug, Default)]
pub(super) struct Candidate {
    pub id: String,
    pub parent_id: Option<String>,
    pub generation: Option<u64>,
    pub proposal_index: Option<u64>,
    pub source: Option<String>,
    /// Content digest of the candidate's own payload — the identity that
    /// survives a rename, and the one two "distinct" proposals can collide on.
    pub digest: Option<String>,
    pub values: Option<Value>,
    pub stages: BTreeMap<String, StageScore>,
    /// The producer's own settled train reward, when it emitted one.
    pub train_reward: Option<f64>,
    pub heldout_reward: Option<f64>,
    /// How this candidate fared at the promotion gate, and on which rows.
    pub gate: Option<MinibatchGate>,
}

/// The minibatch gate, as the producer reports it.
///
/// This is the record that makes a `+0.00` run diagnosable. It carries the
/// candidate's and the parent's reward *and the row ids each was scored on*,
/// so "every proposal was judged against the same unusually strong subset" is
/// something a reader can see rather than infer. `paired` is the invariant the
/// comparison rests on: an unpaired gate is not a comparison at all.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct MinibatchGate {
    pub candidate_reward: Option<f64>,
    pub parent_reward: Option<f64>,
    pub delta: Option<f64>,
    pub accepted: Option<bool>,
    pub rejection_reason: Option<String>,
    pub comparison_result: Option<String>,
    pub candidate_task_ids: Vec<String>,
    pub parent_task_ids: Vec<String>,
}

impl MinibatchGate {
    /// Was the candidate scored on the same rows as the parent it is compared
    /// against? The producer asserts this itself, but a reader of the evidence
    /// should not have to take that on trust.
    fn paired(&self) -> Option<bool> {
        if self.candidate_task_ids.is_empty() || self.parent_task_ids.is_empty() {
            return None;
        }
        Some(self.candidate_task_ids == self.parent_task_ids)
    }

    /// A stable identity for the row draw, so two proposals sharing a minibatch
    /// are visible as one repeated fingerprint instead of two long id lists.
    fn draw_digest(&self) -> Option<String> {
        if self.candidate_task_ids.is_empty() {
            return None;
        }
        use sha2::{Digest, Sha256};
        let joined = self.candidate_task_ids.join(",");
        Some(format!("sha256:{:.16x}", Sha256::digest(joined.as_bytes())))
    }

    fn to_value(&self) -> Value {
        json!({
            "candidateReward": self.candidate_reward,
            "parentReward": self.parent_reward,
            "delta": self.delta,
            "accepted": self.accepted,
            "rejectionReason": self.rejection_reason,
            "comparisonResult": self.comparison_result,
            "rowCount": self.candidate_task_ids.len(),
            "paired": self.paired(),
            "drawDigest": self.draw_digest(),
        })
    }
}

impl Candidate {
    fn is_seed(&self) -> bool {
        self.source.as_deref() == Some("seed")
    }

    fn heldout(&self) -> Option<f64> {
        self.heldout_reward
            .or_else(|| self.stages.get(STAGE_HELDOUT).and_then(StageScore::mean))
    }

    fn heldout_samples(&self) -> u64 {
        self.stages
            .get(STAGE_HELDOUT)
            .map(|stage| stage.scored)
            .unwrap_or(0)
    }

    fn to_value(&self) -> Value {
        json!({
            "id": self.id,
            "parentId": self.parent_id,
            "generation": self.generation,
            "proposalIndex": self.proposal_index,
            "source": self.source,
            "digest": self.digest,
            "values": self.values,
            "trainReward": self.train_reward,
            "heldoutReward": self.heldout(),
            "gate": self.gate.as_ref().map(MinibatchGate::to_value),
            "stages": self
                .stages
                .iter()
                .map(|(stage, score)| (stage.clone(), score.to_value()))
                .collect::<Map<String, Value>>(),
        })
    }
}

/// The whole reduction.
#[derive(Clone, Debug, Default)]
pub(super) struct GepaEvidence {
    /// What the recipe asked the proposer for, from the admitted limits.
    pub proposals_requested: Option<u64>,
    /// What the proposer said it returned.
    pub proposals_returned: Option<u64>,
    /// What actually became a candidate. The three are separate numbers on
    /// purpose: a proposer can under-deliver, and a returned proposal can fail
    /// registration.
    pub proposals_registered: u64,
    pub candidates: Vec<Candidate>,
    pub seed_candidate_id: Option<String>,
    /// The candidate the optimizer's own frontier settled on.
    pub selected_candidate_id: Option<String>,
    pub rollouts_allocated: u64,
    pub rollouts_scored: u64,
    pub rollouts_failed: u64,
    /// Highest concurrent worker count the producer reported. One is not
    /// concurrency; the acceptance gate asks for proof, so record the proof.
    pub max_active_workers: Option<u64>,
    pub usage_lanes: Option<Value>,
    pub failure: Option<Value>,
}

fn as_u64(value: Option<&Value>) -> Option<u64> {
    value.and_then(Value::as_u64)
}

fn as_str(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_owned)
}

/// Row ids, sorted so two draws of the same rows compare equal however the
/// producer happened to order them.
fn string_list(value: Option<&Value>) -> Vec<String> {
    let mut out: Vec<String> = value
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// The event body producers use interchangeably. GEPA writes most of its
/// payloads into `delta`, a few into `snapshot`, and `item` for per-item events.
fn body(event: &OptimizerEventEnvelope) -> Map<String, Value> {
    if !event.delta.is_empty() {
        return event.delta.clone();
    }
    if let Some(snapshot) = event.snapshot.as_ref() {
        return snapshot.clone();
    }
    event
        .item
        .as_ref()
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default()
}

fn digest_of(values: &Value) -> Option<String> {
    use sha2::{Digest, Sha256};
    let canonical = serde_json::to_string(values).ok()?;
    Some(format!("sha256:{:x}", Sha256::digest(canonical.as_bytes())))
}

/// Reduce a run's durable events. Pure — callers seal the result.
pub(super) fn reduce(run: &OptimizerRunRecord, events: &[OptimizerEventEnvelope]) -> GepaEvidence {
    let mut evidence = GepaEvidence {
        proposals_requested: run
            .summary
            .pointer("/limits/proposalsPerGeneration")
            .and_then(Value::as_u64),
        ..GepaEvidence::default()
    };

    let mut order: Vec<String> = Vec::new();
    let mut candidates: BTreeMap<String, Candidate> = BTreeMap::new();
    // The producer emits a partial and a final result for the same evaluation.
    // Counting both would double every lane.
    let mut seen_evaluations: BTreeSet<String> = BTreeSet::new();

    for event in events {
        let payload = body(event);
        match event.event_type.as_str() {
            "candidate.registered" => {
                let Some(id) = as_str(payload.get("candidate_id")) else {
                    continue;
                };
                let values = payload.get("values").cloned();
                let entry = candidates.entry(id.clone()).or_insert_with(|| {
                    order.push(id.clone());
                    Candidate {
                        id: id.clone(),
                        ..Candidate::default()
                    }
                });
                // Identity and lineage are first-write-wins. A candidate's
                // parent, generation, proposal index, and payload are facts
                // fixed at registration; a re-emitted or replayed registration
                // must not be able to reassign them, or a stale reconcile could
                // silently reparent a candidate and change what the search is
                // understood to have explored.
                if entry.parent_id.is_none() {
                    entry.parent_id = as_str(payload.get("parent_id"));
                }
                if entry.generation.is_none() {
                    entry.generation = as_u64(payload.get("generation"));
                }
                if entry.proposal_index.is_none() {
                    entry.proposal_index = as_u64(payload.get("proposal_index"));
                }
                if entry.source.is_none() {
                    entry.source = as_str(payload.get("source"));
                }
                if entry.values.is_none() {
                    if let Some(values) = values {
                        entry.digest = digest_of(&values);
                        entry.values = Some(values);
                    }
                }
                if entry.is_seed() {
                    // Likewise the seed: the first candidate to declare itself
                    // the seed is the baseline every uplift is measured against.
                    evidence.seed_candidate_id.get_or_insert(id);
                }
            }
            "optimizer.candidate_evaluation.allocated" => {
                evidence.rollouts_allocated += 1;
                let (Some(id), Some(stage)) = (
                    as_str(payload.get("candidate_id")),
                    as_str(payload.get("stage")),
                ) else {
                    continue;
                };
                let entry = candidates.entry(id.clone()).or_insert_with(|| {
                    order.push(id.clone());
                    Candidate {
                        id,
                        ..Candidate::default()
                    }
                });
                entry.stages.entry(stage).or_default().samples += 1;
            }
            "optimizer.evaluation_result.received" => {
                // `evaluation_id` is the producer's own identity for one
                // scored example; without it a partial and its final both count.
                let evaluation_id = as_str(payload.get("evaluation_id"));
                if let Some(evaluation_id) = evaluation_id.as_ref() {
                    if !seen_evaluations.insert(evaluation_id.clone()) {
                        continue;
                    }
                }
                if let Some(active) = as_u64(payload.get("active_workers")) {
                    evidence.max_active_workers =
                        Some(evidence.max_active_workers.unwrap_or(0).max(active));
                }
                let (Some(id), Some(stage)) = (
                    as_str(payload.get("candidate_id")),
                    as_str(payload.get("stage")),
                ) else {
                    continue;
                };
                let entry = candidates.entry(id.clone()).or_insert_with(|| {
                    order.push(id.clone());
                    Candidate {
                        id,
                        ..Candidate::default()
                    }
                });
                let score = entry.stages.entry(stage).or_default();
                match payload.get("reward").and_then(Value::as_f64) {
                    Some(reward) => {
                        score.scored += 1;
                        score.sum += reward;
                        evidence.rollouts_scored += 1;
                    }
                    // A rollout that came back without a reward is a rollout
                    // that measured nothing. Scoring it 0.0 would be a
                    // fabricated observation, and with enough of them a failing
                    // candidate looks merely mediocre.
                    None => evidence.rollouts_failed += 1,
                }
            }
            "candidate.evaluated" => {
                let Some(id) = as_str(payload.get("candidate_id")) else {
                    continue;
                };
                let reward = payload.get("train_reward").and_then(Value::as_f64);
                let entry = candidates.entry(id.clone()).or_insert_with(|| {
                    order.push(id.clone());
                    Candidate {
                        id,
                        ..Candidate::default()
                    }
                });
                entry.train_reward = reward.or(entry.train_reward);
            }
            "heldout.completed" => {
                let Some(id) = as_str(payload.get("candidate_id")) else {
                    continue;
                };
                let reward = payload.get("heldout_reward").and_then(Value::as_f64);
                let entry = candidates.entry(id.clone()).or_insert_with(|| {
                    order.push(id.clone());
                    Candidate {
                        id,
                        ..Candidate::default()
                    }
                });
                entry.heldout_reward = reward.or(entry.heldout_reward);
            }
            "frontier.snapshot" | "frontier.updated" => {
                if let Some(id) = as_str(payload.get("best_candidate_id")) {
                    evidence.selected_candidate_id = Some(id);
                }
            }
            "candidate.minibatch_evaluated" | "candidate.rejected" => {
                let Some(id) = as_str(payload.get("candidate_id")) else {
                    continue;
                };
                let entry = candidates.entry(id.clone()).or_insert_with(|| {
                    order.push(id.clone());
                    Candidate {
                        id,
                        ..Candidate::default()
                    }
                });
                let gate = entry.gate.get_or_insert_with(MinibatchGate::default);
                // `candidate.minibatch_evaluated` carries the row ids and the
                // delta; `candidate.rejected` carries the acceptance reason.
                // Both describe one gate, so they merge rather than replace,
                // and neither may overwrite a value the other already proved.
                if gate.candidate_reward.is_none() {
                    gate.candidate_reward = payload
                        .get("minibatch_reward")
                        .or_else(|| payload.get("candidate_minibatch_reward"))
                        .and_then(Value::as_f64);
                }
                if gate.parent_reward.is_none() {
                    gate.parent_reward = payload
                        .get("parent_minibatch_reward")
                        .and_then(Value::as_f64);
                }
                if gate.delta.is_none() {
                    gate.delta = payload.get("minibatch_delta").and_then(Value::as_f64);
                }
                if gate.accepted.is_none() {
                    gate.accepted = payload.get("accepted_minibatch").and_then(Value::as_bool);
                }
                if gate.rejection_reason.is_none() {
                    gate.rejection_reason = as_str(payload.get("reason"));
                }
                if gate.comparison_result.is_none() {
                    gate.comparison_result = as_str(payload.get("comparison_result"));
                }
                if gate.candidate_task_ids.is_empty() {
                    gate.candidate_task_ids =
                        string_list(payload.get("candidate_minibatch_task_ids"));
                }
                if gate.parent_task_ids.is_empty() {
                    gate.parent_task_ids = string_list(payload.get("parent_minibatch_task_ids"));
                }
                if entry.parent_id.is_none() {
                    entry.parent_id = as_str(payload.get("parent_id"));
                }
            }
            "proposer.completed" => {
                evidence.proposals_returned =
                    as_u64(payload.get("proposal_count")).or(evidence.proposals_returned);
            }
            "gepa.run.finished" => {
                if let Some(summary) = payload.get("runtime_summary") {
                    evidence.usage_lanes = Some(usage_lanes(summary));
                }
            }
            "gepa.run.failed" => {
                evidence.failure = payload.get("failure").cloned();
                if let Some(summary) = payload.get("runtime_summary") {
                    evidence.usage_lanes = Some(usage_lanes(summary));
                }
            }
            _ => {}
        }
    }

    evidence.candidates = order
        .into_iter()
        .filter_map(|id| candidates.remove(&id))
        .collect();
    // Counted from distinct candidates rather than incremented per event: the
    // producer can re-emit a registration, and a proposal counted twice is a
    // shortfall hidden.
    evidence.proposals_registered = evidence
        .candidates
        .iter()
        .filter(|candidate| candidate.source.is_some() && !candidate.is_seed())
        .count() as u64;
    evidence
}

/// Split the producer's roll-up into the lanes that are different money.
///
/// The proposer is a frontier reasoning model billed per call; the policy is a
/// small model billed per rollout. One total hides which of the two a recipe
/// actually spends its budget on.
fn usage_lanes(summary: &Value) -> Value {
    let lane = |name: &str| -> Value {
        summary
            .get(name)
            .map(|lane| {
                json!({
                    "model": lane.get("model").cloned().unwrap_or(Value::Null),
                    "calls": lane.get("calls").cloned().unwrap_or(Value::Null),
                    "promptTokens": lane.get("prompt_tokens").cloned().unwrap_or(Value::Null),
                    "completionTokens": lane.get("completion_tokens").cloned().unwrap_or(Value::Null),
                    "costUsd": lane.get("cost_usd").cloned().unwrap_or(Value::Null),
                    "wallSeconds": lane.get("wall_seconds").cloned().unwrap_or(Value::Null),
                })
            })
            .unwrap_or(Value::Null)
    };
    json!({ "proposer": lane("proposer"), "policy": lane("policy") })
}

impl GepaEvidence {
    fn candidate(&self, id: &str) -> Option<&Candidate> {
        self.candidates.iter().find(|entry| entry.id == id)
    }

    fn seed(&self) -> Option<&Candidate> {
        self.seed_candidate_id
            .as_deref()
            .and_then(|id| self.candidate(id))
            .or_else(|| self.candidates.iter().find(|entry| entry.is_seed()))
    }

    fn selected(&self) -> Option<&Candidate> {
        self.selected_candidate_id
            .as_deref()
            .and_then(|id| self.candidate(id))
    }

    /// Every proposal the recipe asked for that never became a candidate.
    fn proposal_shortfall(&self) -> Option<u64> {
        let requested = self.proposals_requested?;
        (self.proposals_registered < requested).then(|| requested - self.proposals_registered)
    }

    /// The verdict, and the numbers it rests on.
    ///
    /// Only a heldout comparison between the seed and a *different* selected
    /// candidate can support an improvement claim. Everything else is honest
    /// about being something less.
    pub(super) fn verdict(&self, run_status: &str) -> (Verdict, Value) {
        if self.failure.is_some() || matches!(run_status, "failed" | "degraded" | "cancelled") {
            return (
                Verdict::Failed,
                json!({
                    "reason": "the search did not finish",
                    "failure": self.failure.clone().unwrap_or(Value::Null),
                }),
            );
        }
        let seed = self.seed();
        let selected = self.selected();
        let baseline = seed.and_then(Candidate::heldout);
        let baseline_samples = seed.map(Candidate::heldout_samples).unwrap_or(0);

        let Some(selected) = selected else {
            return (
                Verdict::Inconclusive,
                json!({
                    "reason": "the run settled on no candidate",
                    "baselineHeldout": baseline,
                    "baselineHeldoutSamples": baseline_samples,
                }),
            );
        };

        // The winner is the seed: the search ran and kept the incumbent. That is
        // a real, reportable outcome — and it is not an improvement.
        if seed.map(|seed| seed.id == selected.id).unwrap_or(false) {
            return (
                Verdict::NoMeasuredImprovement,
                json!({
                    "reason": "the seed candidate was retained; no proposal beat it",
                    "baselineHeldout": baseline,
                    "baselineHeldoutSamples": baseline_samples,
                    "selectedHeldout": baseline,
                    "selectedHeldoutSamples": baseline_samples,
                    "upliftAbsolute": Value::Null,
                    "proposalsRegistered": self.proposals_registered,
                }),
            );
        }

        let challenger = selected.heldout();
        let challenger_samples = selected.heldout_samples();
        // A mean with no samples behind it is a producer summary we could not
        // corroborate from the rollout log. Reporting an uplift on it would put
        // a number where a measurement belongs, so the comparison is refused
        // rather than dressed up.
        if baseline.is_some()
            && challenger.is_some()
            && (baseline_samples == 0 || challenger_samples == 0)
        {
            return (
                Verdict::Inconclusive,
                json!({
                    "reason": "a heldout mean was reported without per-rollout evidence behind it",
                    "baselineCandidateId": seed.map(|seed| seed.id.clone()),
                    "baselineHeldout": baseline,
                    "baselineHeldoutSamples": baseline_samples,
                    "selectedCandidateId": selected.id,
                    "selectedHeldout": challenger,
                    "selectedHeldoutSamples": challenger_samples,
                }),
            );
        }
        let (Some(baseline), Some(challenger)) = (baseline, challenger) else {
            return (
                Verdict::Inconclusive,
                json!({
                    "reason": "no heldout comparison exists between the baseline and the selected candidate",
                    "baselineHeldout": baseline,
                    "baselineHeldoutSamples": baseline_samples,
                    "selectedHeldout": challenger,
                    "selectedHeldoutSamples": challenger_samples,
                }),
            );
        };
        let uplift = challenger - baseline;
        let detail = json!({
            "baselineCandidateId": seed.map(|seed| seed.id.clone()),
            "baselineHeldout": baseline,
            "baselineHeldoutSamples": baseline_samples,
            "selectedCandidateId": selected.id,
            "selectedHeldout": challenger,
            "selectedHeldoutSamples": challenger_samples,
            "upliftAbsolute": uplift,
            "proposalsRegistered": self.proposals_registered,
        });
        if uplift > 0.0 {
            (Verdict::MeasuredImprovement, detail)
        } else {
            (Verdict::NoMeasuredImprovement, detail)
        }
    }

    /// The full evidence body, as it is sealed into the terminal manifest.
    pub(super) fn to_value(&self, run_status: &str) -> Value {
        let (verdict, verdict_detail) = self.verdict(run_status);
        // Deployment is a separate decision from optimizer selection. A frontier
        // winner that never beat the baseline is not a thing to ship, and the
        // gap between "selected" and "recommended" is exactly where "Promoted"
        // used to appear without evidence.
        let deployment = if verdict == Verdict::MeasuredImprovement {
            json!({
                "candidateId": self.selected_candidate_id,
                "recommended": true,
                "basis": "heldout improvement over the baseline",
            })
        } else {
            json!({
                "candidateId": Value::Null,
                "recommended": false,
                "basis": verdict_detail
                    .get("reason")
                    .cloned()
                    .unwrap_or_else(|| json!("no measured heldout improvement")),
            })
        };
        let mut stage_totals: BTreeMap<String, u64> = BTreeMap::new();
        for candidate in &self.candidates {
            for (stage, score) in &candidate.stages {
                *stage_totals.entry(stage.clone()).or_default() += score.samples;
            }
        }
        // Gate accounting. `distinctDraws` against `gated` is the check that
        // caught the +0.00 run: ten proposals sharing one row draw is a search
        // that cannot select, and it reads here as `gated: 10, distinctDraws: 1`.
        let gates: Vec<&MinibatchGate> = self
            .candidates
            .iter()
            .filter_map(|candidate| candidate.gate.as_ref())
            .collect();
        let distinct_draws = gates
            .iter()
            .filter_map(|gate| gate.draw_digest())
            .collect::<BTreeSet<_>>();
        let mut rejection_reasons: BTreeMap<String, u64> = BTreeMap::new();
        for gate in &gates {
            if gate.accepted == Some(true) {
                continue;
            }
            if let Some(reason) = gate.rejection_reason.as_ref() {
                *rejection_reasons.entry(reason.clone()).or_default() += 1;
            }
        }
        // `None` where nothing was gated, so an absent gate is never reported
        // as a gate that everything passed.
        let all_paired =
            (!gates.is_empty()).then(|| gates.iter().all(|gate| gate.paired() == Some(true)));
        json!({
            "schemaVersion": GEPA_EVIDENCE_SCHEMA,
            "gate": {
                "gated": gates.len(),
                "accepted": gates.iter().filter(|gate| gate.accepted == Some(true)).count(),
                "rejected": gates.iter().filter(|gate| gate.accepted == Some(false)).count(),
                "rejectionReasons": rejection_reasons,
                "distinctDraws": distinct_draws.len(),
                "allComparisonsPaired": all_paired,
            },
            "proposals": {
                "requested": self.proposals_requested,
                "returned": self.proposals_returned,
                "registered": self.proposals_registered,
                "shortfall": self.proposal_shortfall(),
            },
            "candidates": self.candidates.iter().map(Candidate::to_value).collect::<Vec<_>>(),
            "seedCandidateId": self.seed_candidate_id,
            "selectedCandidateId": self.selected_candidate_id,
            "deployment": deployment,
            "rollouts": {
                "allocated": self.rollouts_allocated,
                "scored": self.rollouts_scored,
                "unscored": self.rollouts_failed,
                "byStage": stage_totals,
                "maxActiveWorkers": self.max_active_workers,
            },
            "usageLanes": self.usage_lanes.clone().unwrap_or(Value::Null),
            "verdict": verdict.as_str(),
            "verdictDetail": verdict_detail,
        })
    }
}

/// Every event type this module reduces, paired with the payload keys it reads.
///
/// This is the reducer's contract with the producer, stated in one place so a
/// test can hold it against real captured logs. Before this existed the module
/// matched `rollout.completed`, `rollout.terminal`, and `gepa.rollout.completed`
/// — three names the producer has never emitted — and nothing failed. Every
/// GEPA manifest just sealed empty.
pub(super) const HANDLED_PRODUCER_EVENTS: &[(&str, &[&str])] = &[
    (
        "candidate.registered",
        &["candidate_id", "source", "values"],
    ),
    (
        "optimizer.candidate_evaluation.allocated",
        &["candidate_id", "stage"],
    ),
    (
        "optimizer.evaluation_result.received",
        &["candidate_id", "stage", "evaluation_id", "active_workers"],
    ),
    ("candidate.evaluated", &["candidate_id", "train_reward"]),
    ("heldout.completed", &["candidate_id", "heldout_reward"]),
    ("frontier.snapshot", &["best_candidate_id"]),
    ("frontier.updated", &["best_candidate_id"]),
    (
        "candidate.minibatch_evaluated",
        &[
            "candidate_id",
            "minibatch_reward",
            "parent_minibatch_reward",
            "minibatch_delta",
            "accepted_minibatch",
            "candidate_minibatch_task_ids",
            "parent_minibatch_task_ids",
        ],
    ),
    (
        "candidate.rejected",
        &["candidate_id", "reason", "comparison_result"],
    ),
    ("proposer.completed", &["proposal_count"]),
    ("gepa.run.finished", &["runtime_summary"]),
    ("gepa.run.failed", &["failure"]),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::optimizers::models::OPTIMIZER_EVENT_SCHEMA_VERSION;

    pub(super) fn run(status: &str, requested: u64) -> OptimizerRunRecord {
        OptimizerRunRecord {
            schema_version: "optimizer_run.v1".into(),
            id: "banking77_gepa_luna_med_959b2d7c".into(),
            algorithm_id: "gepa".into(),
            algorithm_version: Some("synth-optimizers-0.2.14".into()),
            status: status.into(),
            source: "local".into(),
            objective: None,
            project_ref: None,
            session_ref: Some("chat_1".into()),
            created_at: "2026-08-17T22:50:20+00:00".into(),
            started_at: Some("2026-08-17T22:50:20+00:00".into()),
            finished_at: Some("2026-08-17T22:53:26+00:00".into()),
            cursor_seq: 1030,
            capabilities: Default::default(),
            execution_bindings: vec![],
            input_refs: vec![],
            output_refs: vec![],
            visual_refs: vec![],
            summary: json!({ "limits": { "proposalsPerGeneration": requested } }),
            usage: Default::default(),
            error: None,
        }
    }

    fn event(seq: u64, event_type: &str, delta: Value) -> OptimizerEventEnvelope {
        OptimizerEventEnvelope {
            schema_version: OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
            event_id: Some(format!("run:{seq}")),
            event_type: event_type.into(),
            sequence_number: seq,
            occurred_at: "2026-08-17T22:50:21+00:00".into(),
            optimizer_run_id: "banking77_gepa_luna_med_959b2d7c".into(),
            algorithm_id: "gepa".into(),
            level: None,
            item: None,
            delta: delta.as_object().cloned().unwrap_or_default(),
            snapshot: None,
            usage_delta: None,
            artifact_refs: vec![],
            error: None,
            raw: json!({}),
        }
    }

    /// Every payload shape below is taken from the durable log of the real
    /// packaged run `banking77_gepa_luna_med_959b2d7c`.
    fn seed_retained_history() -> Vec<OptimizerEventEnvelope> {
        let mut seq = 0;
        let mut events = vec![event(
            {
                seq += 1;
                seq
            },
            "candidate.registered",
            json!({
                "candidate_id": "gepa_seed",
                "source": "seed",
                "status": "registered",
                "values": { "stage2_system": "Classify the query." }
            }),
        )];
        let push_stage = |events: &mut Vec<OptimizerEventEnvelope>,
                          seq: &mut u64,
                          candidate: &str,
                          stage: &str,
                          rewards: &[f64]| {
            for (index, reward) in rewards.iter().enumerate() {
                *seq += 1;
                events.push(event(
                    *seq,
                    "optimizer.candidate_evaluation.allocated",
                    json!({ "candidate_id": candidate, "stage": stage, "example_id": format!("row:{index}") }),
                ));
                *seq += 1;
                events.push(event(
                    *seq,
                    "optimizer.evaluation_result.received",
                    json!({
                        "candidate_id": candidate,
                        "stage": stage,
                        "example_id": format!("row:{index}"),
                        "evaluation_id": format!("{candidate}:{stage}:row:{index}"),
                        "reward": reward,
                        "active_workers": 7,
                        "partial": true
                    }),
                ));
                // The producer re-emits the same evaluation as a final.
                *seq += 1;
                events.push(event(
                    *seq,
                    "optimizer.evaluation_result.received",
                    json!({
                        "candidate_id": candidate,
                        "stage": stage,
                        "example_id": format!("row:{index}"),
                        "evaluation_id": format!("{candidate}:{stage}:row:{index}"),
                        "reward": reward,
                        "active_workers": 7
                    }),
                ));
            }
        };
        push_stage(
            &mut events,
            &mut seq,
            "gepa_seed",
            STAGE_SEED_FULL_TRAIN,
            &[1.0, 1.0, 0.0, 1.0],
        );
        seq += 1;
        events.push(event(
            seq,
            "candidate.evaluated",
            json!({ "candidate_id": "gepa_seed", "train_reward": 0.75 }),
        ));
        seq += 1;
        events.push(event(
            seq,
            "proposer.completed",
            json!({ "proposal_count": 1, "backend": "codex_app_server", "model": "gpt-5.6-luna" }),
        ));
        seq += 1;
        events.push(event(
            seq,
            "candidate.registered",
            json!({
                "candidate_id": "gepa_child",
                "parent_id": "gepa_seed",
                "generation": 0,
                "proposal_index": 0,
                "source": "reflector:parent_variation",
                "values": { "stage2_system": "Route the query with an ordered procedure." }
            }),
        ));
        push_stage(
            &mut events,
            &mut seq,
            "gepa_seed",
            STAGE_PARENT_MINIBATCH,
            &[1.0, 1.0],
        );
        push_stage(
            &mut events,
            &mut seq,
            "gepa_child",
            STAGE_CANDIDATE_MINIBATCH,
            &[0.0, 1.0],
        );
        push_stage(
            &mut events,
            &mut seq,
            "gepa_seed",
            STAGE_HELDOUT,
            &[1.0, 0.0, 1.0, 0.0],
        );
        seq += 1;
        events.push(event(
            seq,
            "heldout.completed",
            json!({ "candidate_id": "gepa_seed", "heldout_reward": 0.5, "train_reward": 0.75 }),
        ));
        seq += 1;
        events.push(event(
            seq,
            "frontier.snapshot",
            json!({ "best_candidate_id": "gepa_seed", "best_train_reward": 0.75, "candidate_count": 2 }),
        ));
        seq += 1;
        events.push(event(
            seq,
            "gepa.run.finished",
            json!({
                "state": "completed",
                "runtime_summary": {
                    "policy": { "model": "gpt-4.1-nano", "calls": 12, "cost_usd": 0.01, "prompt_tokens": 100, "completion_tokens": 5, "wall_seconds": 85.4 },
                    "proposer": { "model": "gpt-5.6-luna", "calls": 1, "cost_usd": 0.0, "prompt_tokens": 53753, "completion_tokens": 164, "wall_seconds": 100.1 }
                }
            }),
        ));
        events
    }

    /// The reported defect as a contract: a search whose seed survived is not a
    /// promotion, however good the seed's heldout number looks.
    #[test]
    fn a_search_that_kept_its_seed_reports_no_measured_improvement() {
        let evidence = reduce(&run("completed", 10), &seed_retained_history());
        let (verdict, detail) = evidence.verdict("completed");
        assert_eq!(verdict, Verdict::NoMeasuredImprovement);
        assert_eq!(detail["selectedHeldout"], json!(0.5));
        assert_eq!(detail["upliftAbsolute"], Value::Null);
        let value = evidence.to_value("completed");
        assert_eq!(value["deployment"]["recommended"], json!(false));
        assert_eq!(value["deployment"]["candidateId"], Value::Null);
    }

    /// Ten requested, one delivered. Both numbers survive; the shortfall is
    /// named rather than rounded away.
    #[test]
    fn a_proposer_that_under_delivers_reports_both_numbers() {
        let evidence = reduce(&run("completed", 10), &seed_retained_history());
        assert_eq!(evidence.proposals_requested, Some(10));
        assert_eq!(evidence.proposals_returned, Some(1));
        assert_eq!(evidence.proposals_registered, 1);
        assert_eq!(evidence.proposal_shortfall(), Some(9));
        let value = evidence.to_value("completed");
        assert_eq!(value["proposals"]["shortfall"], json!(9));
    }

    /// A partial and its final are one evaluation, not two rollouts.
    #[test]
    fn a_re_emitted_evaluation_is_not_counted_twice() {
        let evidence = reduce(&run("completed", 10), &seed_retained_history());
        // 4 seed train + 2 parent minibatch + 2 candidate minibatch + 4 heldout.
        assert_eq!(evidence.rollouts_allocated, 12);
        assert_eq!(evidence.rollouts_scored, 12);
    }

    /// Stages stay separate: a train mean must never stand in for heldout.
    #[test]
    fn each_stage_keeps_its_own_samples_and_mean() {
        let evidence = reduce(&run("completed", 10), &seed_retained_history());
        let seed = evidence.candidate("gepa_seed").expect("seed candidate");
        assert_eq!(seed.stages[STAGE_SEED_FULL_TRAIN].scored, 4);
        assert_eq!(seed.stages[STAGE_SEED_FULL_TRAIN].mean(), Some(0.75));
        assert_eq!(seed.stages[STAGE_HELDOUT].scored, 4);
        assert_eq!(seed.stages[STAGE_HELDOUT].mean(), Some(0.5));
        let child = evidence.candidate("gepa_child").expect("child candidate");
        assert_eq!(child.stages[STAGE_CANDIDATE_MINIBATCH].scored, 2);
        assert!(
            child.stages.get(STAGE_HELDOUT).is_none(),
            "the child never reached heldout"
        );
    }

    #[test]
    fn lineage_records_the_parent_generation_and_digest() {
        let evidence = reduce(&run("completed", 10), &seed_retained_history());
        let child = evidence.candidate("gepa_child").expect("child candidate");
        assert_eq!(child.parent_id.as_deref(), Some("gepa_seed"));
        assert_eq!(child.generation, Some(0));
        assert_eq!(child.proposal_index, Some(0));
        let seed = evidence.candidate("gepa_seed").expect("seed candidate");
        assert!(
            child.digest.is_some() && child.digest != seed.digest,
            "distinct payloads must not share a digest"
        );
    }

    #[test]
    fn concurrency_is_read_from_the_producers_own_worker_count() {
        let evidence = reduce(&run("completed", 10), &seed_retained_history());
        assert_eq!(evidence.max_active_workers, Some(7));
    }

    #[test]
    fn usage_stays_split_between_the_proposer_and_the_policy() {
        let evidence = reduce(&run("completed", 10), &seed_retained_history());
        let lanes = evidence.usage_lanes.expect("lanes");
        assert_eq!(lanes["proposer"]["model"], json!("gpt-5.6-luna"));
        assert_eq!(lanes["policy"]["model"], json!("gpt-4.1-nano"));
        assert_eq!(
            evidence.failure, None,
            "the successful captured event set must not contain a run failure"
        );
        assert_eq!(lanes["proposer"]["promptTokens"], json!(53753));
    }

    /// A rollout that returned no reward measured nothing. Scoring it zero would
    /// invent an observation and drag every mean down with it.
    #[test]
    fn an_unscored_rollout_is_not_a_zero() {
        let mut events = seed_retained_history();
        events.push(event(
            9_000,
            "optimizer.evaluation_result.received",
            json!({
                "candidate_id": "gepa_seed",
                "stage": STAGE_HELDOUT,
                "evaluation_id": "gepa_seed:heldout:row:99"
            }),
        ));
        let evidence = reduce(&run("completed", 10), &events);
        assert_eq!(evidence.rollouts_failed, 1);
        let seed = evidence.candidate("gepa_seed").expect("seed candidate");
        assert_eq!(
            seed.stages[STAGE_HELDOUT].scored, 4,
            "the unscored result adds no sample"
        );
        assert_eq!(
            seed.stages[STAGE_HELDOUT].mean(),
            Some(0.5),
            "and does not move the mean"
        );
    }

    #[test]
    fn a_challenger_that_beats_the_baseline_on_heldout_is_a_measured_improvement() {
        let mut events = seed_retained_history();
        let mut seq = 8_000;
        for (index, reward) in [1.0, 1.0, 1.0, 0.0].iter().enumerate() {
            seq += 1;
            events.push(event(
                seq,
                "optimizer.candidate_evaluation.allocated",
                json!({ "candidate_id": "gepa_child", "stage": STAGE_HELDOUT }),
            ));
            seq += 1;
            events.push(event(
                seq,
                "optimizer.evaluation_result.received",
                json!({
                    "candidate_id": "gepa_child",
                    "stage": STAGE_HELDOUT,
                    "evaluation_id": format!("gepa_child:heldout:row:{index}"),
                    "reward": reward
                }),
            ));
        }
        seq += 1;
        events.push(event(
            seq,
            "heldout.completed",
            json!({ "candidate_id": "gepa_child", "heldout_reward": 0.75 }),
        ));
        seq += 1;
        events.push(event(
            seq,
            "frontier.snapshot",
            json!({ "best_candidate_id": "gepa_child" }),
        ));
        let evidence = reduce(&run("completed", 10), &events);
        let (verdict, detail) = evidence.verdict("completed");
        assert_eq!(verdict, Verdict::MeasuredImprovement);
        assert_eq!(detail["baselineHeldout"], json!(0.5));
        assert_eq!(detail["selectedHeldout"], json!(0.75));
        assert_eq!(detail["selectedHeldoutSamples"], json!(4));
        assert_eq!(detail["upliftAbsolute"], json!(0.25));
        let value = evidence.to_value("completed");
        assert_eq!(value["deployment"]["recommended"], json!(true));
        assert_eq!(value["deployment"]["candidateId"], json!("gepa_child"));
    }

    /// Craftax GEPA's real ending: the seed evaluated, the proposer call timed
    /// out, nothing was compared. That is `failed`, not `no improvement`.
    #[test]
    fn a_run_whose_proposer_never_answered_is_failed_not_merely_unimproved() {
        let events = vec![
            event(
                1,
                "candidate.registered",
                json!({ "candidate_id": "gepa_seed", "source": "seed" }),
            ),
            event(
                2,
                "gepa.run.failed",
                json!({
                    "state": "failed",
                    "failure": {
                        "error_code": "synth_optimizer_failed",
                        "failure": { "failure_type": "timeout", "message": "codex app-server timed out waiting for response", "retryable": true }
                    }
                }),
            ),
        ];
        let evidence = reduce(&run("failed", 1), &events);
        let (verdict, detail) = evidence.verdict("failed");
        assert_eq!(verdict, Verdict::Failed);
        assert!(detail["failure"]["failure"]["retryable"]
            .as_bool()
            .unwrap_or(false));
    }

    /// Lineage is fixed at registration. A replayed or stale registration
    /// arriving later must not be able to reparent a candidate, rewrite its
    /// payload, or move which candidate is the baseline — all of which would
    /// silently change what the search is understood to have explored.
    #[test]
    fn a_later_registration_cannot_rewrite_identity_or_lineage() {
        let mut events = seed_retained_history();
        events.push(event(
            9_100,
            "candidate.registered",
            json!({
                "candidate_id": "gepa_child",
                "parent_id": "gepa_someone_else",
                "generation": 7,
                "proposal_index": 4,
                "source": "seed",
                "values": { "stage2_system": "a different payload entirely" }
            }),
        ));
        let evidence = reduce(&run("completed", 10), &events);
        let child = evidence.candidate("gepa_child").expect("child candidate");
        assert_eq!(child.parent_id.as_deref(), Some("gepa_seed"));
        assert_eq!(child.generation, Some(0));
        assert_eq!(child.proposal_index, Some(0));
        assert_eq!(child.source.as_deref(), Some("reflector:parent_variation"));
        assert_eq!(
            evidence.seed_candidate_id.as_deref(),
            Some("gepa_seed"),
            "the baseline every uplift is measured against cannot be reassigned"
        );
    }

    /// A registration re-emitted verbatim is the same proposal, not a second
    /// one. Counting it twice would hide a shortfall.
    #[test]
    fn a_replayed_registration_is_not_a_second_proposal() {
        let mut events = seed_retained_history();
        let replay = events
            .iter()
            .find(|event| {
                event.event_type == "candidate.registered"
                    && event.delta.get("candidate_id") == Some(&json!("gepa_child"))
            })
            .cloned()
            .expect("the child registration");
        events.push(replay);
        let evidence = reduce(&run("completed", 10), &events);
        assert_eq!(evidence.proposals_registered, 1);
        assert_eq!(evidence.proposal_shortfall(), Some(9));
    }

    /// A producer summary with no per-rollout evidence behind it is not a
    /// measurement, and must not become an uplift.
    #[test]
    fn a_heldout_mean_with_no_samples_behind_it_is_inconclusive() {
        let events = vec![
            event(
                1,
                "candidate.registered",
                json!({ "candidate_id": "gepa_seed", "source": "seed" }),
            ),
            event(
                2,
                "candidate.registered",
                json!({ "candidate_id": "gepa_child", "parent_id": "gepa_seed", "source": "reflector:parent_variation" }),
            ),
            event(
                3,
                "heldout.completed",
                json!({ "candidate_id": "gepa_seed", "heldout_reward": 0.5 }),
            ),
            event(
                4,
                "heldout.completed",
                json!({ "candidate_id": "gepa_child", "heldout_reward": 0.9 }),
            ),
            event(
                5,
                "frontier.snapshot",
                json!({ "best_candidate_id": "gepa_child" }),
            ),
        ];
        let evidence = reduce(&run("completed", 10), &events);
        let (verdict, detail) = evidence.verdict("completed");
        assert_eq!(verdict, Verdict::Inconclusive);
        assert_eq!(detail["selectedHeldoutSamples"], json!(0));
        assert!(
            detail.get("upliftAbsolute").is_none(),
            "no uplift is claimed"
        );
    }

    /// A finished search with no heldout evidence at all cannot claim anything.
    #[test]
    fn a_run_with_no_heldout_evidence_is_inconclusive() {
        let events = vec![
            event(
                1,
                "candidate.registered",
                json!({ "candidate_id": "gepa_seed", "source": "seed" }),
            ),
            event(
                2,
                "candidate.registered",
                json!({ "candidate_id": "gepa_child", "parent_id": "gepa_seed", "source": "reflector:parent_variation" }),
            ),
            event(
                3,
                "frontier.snapshot",
                json!({ "best_candidate_id": "gepa_child" }),
            ),
        ];
        let evidence = reduce(&run("completed", 10), &events);
        assert_eq!(evidence.verdict("completed").0, Verdict::Inconclusive);
    }
}

/// Producer-vocabulary drift, held against real captured logs.
///
/// `fixtures/gepa_producer_vocabulary.json` is verbatim payloads and a full
/// event census from two real packaged runs — a completed Banking77 search and
/// a Craftax search that died on a proposer timeout. If the producer renames an
/// event or a payload key, these fail instead of the manifest quietly going
/// null again.
#[cfg(test)]
mod producer_vocabulary_tests {
    use super::*;

    fn fixture() -> Value {
        serde_json::from_str(include_str!("fixtures/gepa_producer_vocabulary.json"))
            .expect("the captured producer vocabulary must parse")
    }

    #[test]
    fn every_event_the_reducer_handles_exists_in_a_real_producer_log() {
        let fixture = fixture();
        let census = fixture["census"]
            .as_object()
            .expect("census")
            .values()
            .filter_map(Value::as_object)
            .flat_map(|run| run.keys().cloned())
            .collect::<BTreeSet<_>>();
        let missing = HANDLED_PRODUCER_EVENTS
            .iter()
            .map(|(event, _)| *event)
            .filter(|event| !census.contains(*event))
            .collect::<Vec<_>>();
        assert!(
            missing.is_empty(),
            "the reducer matches events no real producer log contains: {missing:?}. \
             Either the producer renamed them or they were never emitted — and a \
             reducer listening for a name nobody sends seals an empty manifest."
        );
    }

    #[test]
    fn every_payload_key_the_reducer_reads_is_present_in_a_real_payload() {
        let fixture = fixture();
        let samples = fixture["samples"].as_object().expect("samples");
        let mut missing: Vec<String> = Vec::new();
        for (event, keys) in HANDLED_PRODUCER_EVENTS {
            let Some(sample) = samples.get(*event) else {
                missing.push(format!("{event}: no captured payload"));
                continue;
            };
            let present = sample["keys"]
                .as_array()
                .expect("captured keys")
                .iter()
                .filter_map(Value::as_str)
                .collect::<BTreeSet<_>>();
            for key in *keys {
                if !present.contains(key) {
                    missing.push(format!("{event}.{key}"));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "payload keys the reducer reads are absent from real payloads: {missing:?}"
        );
    }

    /// The reduction, run over the producer's own bytes rather than a
    /// hand-written approximation of them.
    #[test]
    fn the_reducer_extracts_the_captured_payloads_it_claims_to() {
        let fixture = fixture();
        let samples = fixture["samples"].as_object().expect("samples");
        let events = HANDLED_PRODUCER_EVENTS
            .iter()
            .enumerate()
            // `gepa.run.failed` is captured from the Craftax run; replaying it
            // beside Banking77's `gepa.run.finished` would be two endings for
            // one run. It gets its own assertion below.
            .filter(|(_, (event_type, _))| *event_type != "gepa.run.failed")
            .filter_map(|(index, (event_type, _))| {
                let payload = samples.get(*event_type)?.get("payload")?.clone();
                Some(OptimizerEventEnvelope {
                    schema_version: crate::optimizers::models::OPTIMIZER_EVENT_SCHEMA_VERSION
                        .into(),
                    event_id: Some(format!("captured:{index}")),
                    event_type: (*event_type).into(),
                    sequence_number: index as u64 + 1,
                    occurred_at: "2026-08-17T22:50:21+00:00".into(),
                    optimizer_run_id: "banking77_gepa_luna_med_959b2d7c".into(),
                    algorithm_id: "gepa".into(),
                    level: None,
                    item: None,
                    delta: payload.as_object().cloned().unwrap_or_default(),
                    snapshot: None,
                    usage_delta: None,
                    artifact_refs: vec![],
                    error: None,
                    raw: json!({}),
                })
            })
            .collect::<Vec<_>>();

        let mut run = super::tests::run("completed", 10);
        run.id = "banking77_gepa_luna_med_959b2d7c".into();
        let evidence = reduce(&run, &events);

        // The real run's own numbers, read back out of its own payloads.
        assert_eq!(
            evidence.seed_candidate_id.as_deref(),
            Some("gepa_1c284a9e221e")
        );
        assert_eq!(
            evidence.selected_candidate_id.as_deref(),
            Some("gepa_1c284a9e221e")
        );
        assert_eq!(evidence.proposals_returned, Some(1));
        assert_eq!(evidence.max_active_workers, Some(7));
        let lanes = evidence.usage_lanes.clone().expect("per-lane usage");
        assert_eq!(lanes["proposer"]["model"], json!("gpt-5.6-luna"));
        assert_eq!(lanes["policy"]["model"], json!("gpt-4.1-nano"));

        // The gate that rejected the one proposal, with the rows it was judged on.
        let child = evidence
            .candidates
            .iter()
            .find(|candidate| candidate.id == "gepa_f5fb431d3f4f")
            .expect("the rejected proposal");
        let gate = child.gate.as_ref().expect("a gate record");
        assert_eq!(gate.candidate_reward, Some(0.85));
        assert_eq!(gate.parent_reward, Some(0.9));
        assert_eq!(gate.accepted, Some(false));
        assert_eq!(
            gate.rejection_reason.as_deref(),
            Some("primary_not_improved")
        );
        assert_eq!(
            gate.comparison_result.as_deref(),
            Some("incumbent_dominates")
        );
        assert_eq!(
            gate.paired(),
            Some(true),
            "the producer scored candidate and parent on the same rows"
        );

        // And the verdict the whole lane exists to produce.
        assert_eq!(
            evidence.verdict("completed").0,
            Verdict::NoMeasuredImprovement
        );

        // The Craftax run's captured ending, on the same payload bytes: a
        // proposer that never answered is a failed search, not an unimproved one.
        let mut failed = events.clone();
        let failure = samples["gepa.run.failed"]["payload"].clone();
        failed.push(OptimizerEventEnvelope {
            event_type: "gepa.run.failed".into(),
            sequence_number: 999,
            delta: failure.as_object().cloned().unwrap_or_default(),
            ..events[0].clone()
        });
        let evidence = reduce(&run, &failed);
        let (verdict, detail) = evidence.verdict("failed");
        assert_eq!(verdict, Verdict::Failed);
        assert_eq!(
            detail["failure"]["failure"]["failure_type"],
            json!("timeout")
        );
    }

    /// The degeneracy that produced `+0.00`, expressed as the number a reader
    /// can check: ten proposals gated on one row draw.
    #[test]
    fn one_row_draw_shared_across_every_proposal_is_visible_in_the_evidence() {
        let fixture = fixture();
        let sample = fixture["samples"]["candidate.minibatch_evaluated"]["payload"].clone();
        let mut events = Vec::new();
        for index in 0..3u64 {
            let mut payload = sample.as_object().cloned().expect("payload");
            payload.insert("candidate_id".into(), json!(format!("gepa_child_{index}")));
            events.push(OptimizerEventEnvelope {
                schema_version: crate::optimizers::models::OPTIMIZER_EVENT_SCHEMA_VERSION.into(),
                event_id: Some(format!("draw:{index}")),
                event_type: "candidate.minibatch_evaluated".into(),
                sequence_number: index + 1,
                occurred_at: "2026-08-17T22:50:21+00:00".into(),
                optimizer_run_id: "run".into(),
                algorithm_id: "gepa".into(),
                level: None,
                item: None,
                delta: payload,
                snapshot: None,
                usage_delta: None,
                artifact_refs: vec![],
                error: None,
                raw: json!({}),
            });
        }
        let evidence = reduce(&super::tests::run("completed", 10), &events);
        let value = evidence.to_value("completed");
        assert_eq!(value["gate"]["gated"], json!(3));
        assert_eq!(
            value["gate"]["distinctDraws"],
            json!(1),
            "three proposals judged on one draw is a search that cannot select"
        );
        assert_eq!(value["gate"]["allComparisonsPaired"], json!(true));
    }
}
