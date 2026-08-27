//! Explicit run and rollout state machines.
//!
//! Progress is tracked per rollout, never inferred from a completed count. The
//! failure this replaces read `5/5` off a records array and rendered a full
//! progress bar while three of those rollouts were still running and two had
//! no reward at all. A count is a projection of state; it is not state.
//!
//! Two rules are enforced structurally:
//!
//! 1. Every transition is checked against a declared table. An undeclared
//!    transition is an error, never a coercion to the nearest plausible state.
//! 2. `Completed` is a claim about evidence, not about arithmetic. It is
//!    reachable only through [`RunProgress::settle`], which requires every
//!    declared rollout to hold a valid terminal record.

use super::error::AdmissionErrorCode;
use super::ids::RolloutId;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fmt;

// ---------------------------------------------------------------------------
// Run state
// ---------------------------------------------------------------------------

/// The admission-and-execution lifecycle of one evaluation run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Draft,
    Validating,
    ReadyForApproval,
    AwaitingApproval,
    Admitted,
    Starting,
    Running,
    Completed,
    Failed,
    Cancelled,
    Degraded,
}

impl RunState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::Validating => "validating",
            Self::ReadyForApproval => "ready_for_approval",
            Self::AwaitingApproval => "awaiting_approval",
            Self::Admitted => "admitted",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Degraded => "degraded",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Degraded
        )
    }

    /// The declared transition table. Anything absent from here is invalid.
    ///
    /// Cancellation is reachable from every non-terminal state because an
    /// operator may stop a run at any point; nothing else is.
    fn permitted_successors(self) -> &'static [RunState] {
        match self {
            Self::Draft => &[Self::Validating, Self::Failed, Self::Cancelled],
            Self::Validating => &[Self::ReadyForApproval, Self::Failed, Self::Cancelled],
            Self::ReadyForApproval => &[Self::AwaitingApproval, Self::Failed, Self::Cancelled],
            // A rejected approval is a failure of the run, not of the compute.
            Self::AwaitingApproval => &[Self::Admitted, Self::Failed, Self::Cancelled],
            Self::Admitted => &[Self::Starting, Self::Failed, Self::Cancelled],
            Self::Starting => &[Self::Running, Self::Failed, Self::Cancelled, Self::Degraded],
            Self::Running => &[
                Self::Completed,
                Self::Failed,
                Self::Cancelled,
                Self::Degraded,
            ],
            // Terminal states are terminal. A late writeback must not reopen a
            // settled run, which is how a finished campaign got overwritten.
            Self::Completed | Self::Failed | Self::Cancelled | Self::Degraded => &[],
        }
    }

    /// Check a transition without performing it.
    pub fn may_transition_to(self, next: RunState) -> bool {
        self.permitted_successors().contains(&next)
    }

    /// Perform a transition, or return the typed rejection.
    pub fn transition_to(self, next: RunState) -> Result<RunState, StateTransitionError> {
        if self.may_transition_to(next) {
            return Ok(next);
        }
        Err(StateTransitionError {
            code: AdmissionErrorCode::ExecutionSpecInvalid,
            kind: TransitionKind::Run,
            from: self.as_str(),
            to: next.as_str(),
            permitted: self
                .permitted_successors()
                .iter()
                .map(|state| state.as_str())
                .collect(),
        })
    }
}

impl fmt::Display for RunState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// Rollout state
// ---------------------------------------------------------------------------

/// One rollout's own lifecycle, tracked independently of every other rollout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RolloutState {
    Planned,
    Queued,
    Starting,
    Running,
    Completed,
    Failed,
    Cancelled,
    Degraded,
}

impl RolloutState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Queued => "queued",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Degraded => "degraded",
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Degraded
        )
    }

    /// States that mean "this rollout has not finished". A run holding any of
    /// these cannot be `completed`.
    pub fn is_in_flight(self) -> bool {
        matches!(self, Self::Queued | Self::Starting | Self::Running)
    }

    fn permitted_successors(self) -> &'static [RolloutState] {
        match self {
            Self::Planned => &[Self::Queued, Self::Cancelled, Self::Failed],
            // A rollout that starts must leave `queued`. Staying queued while
            // the container is already stepping it is the state lie that made
            // a run look idle while it was spending.
            Self::Queued => &[Self::Starting, Self::Cancelled, Self::Failed],
            Self::Starting => &[
                Self::Running,
                Self::Failed,
                Self::Cancelled,
                Self::Degraded,
            ],
            Self::Running => &[
                Self::Completed,
                Self::Failed,
                Self::Cancelled,
                Self::Degraded,
            ],
            Self::Completed | Self::Failed | Self::Cancelled | Self::Degraded => &[],
        }
    }

    pub fn may_transition_to(self, next: RolloutState) -> bool {
        self.permitted_successors().contains(&next)
    }

    pub fn transition_to(self, next: RolloutState) -> Result<RolloutState, StateTransitionError> {
        if self.may_transition_to(next) {
            return Ok(next);
        }
        Err(StateTransitionError {
            code: AdmissionErrorCode::ExecutionSpecInvalid,
            kind: TransitionKind::Rollout,
            from: self.as_str(),
            to: next.as_str(),
            permitted: self
                .permitted_successors()
                .iter()
                .map(|state| state.as_str())
                .collect(),
        })
    }
}

impl fmt::Display for RolloutState {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionKind {
    Run,
    Rollout,
}

impl TransitionKind {
    /// The stable public code for an illegal transition.
    pub fn error_code(self) -> &'static str {
        match self {
            Self::Run => "invalid_run_state_transition",
            Self::Rollout => "invalid_rollout_state_transition",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StateTransitionError {
    /// Kept so the error can be folded into the admission taxonomy when it
    /// surfaces through an admission-facing call.
    pub code: AdmissionErrorCode,
    pub kind: TransitionKind,
    pub from: &'static str,
    pub to: &'static str,
    pub permitted: Vec<&'static str>,
}

impl StateTransitionError {
    pub fn public_code(&self) -> &'static str {
        self.kind.error_code()
    }

    pub fn to_json(&self) -> Value {
        json!({
            "code": self.public_code(),
            "from": self.from,
            "to": self.to,
            "permitted": self.permitted,
        })
    }
}

impl fmt::Display for StateTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}: `{}` may not transition to `{}` (permitted: {})",
            self.public_code(),
            self.from,
            self.to,
            if self.permitted.is_empty() {
                "none, this state is terminal".to_string()
            } else {
                self.permitted.join(", ")
            }
        )
    }
}

impl std::error::Error for StateTransitionError {}

// ---------------------------------------------------------------------------
// Per-rollout evidence
// ---------------------------------------------------------------------------

/// What a finished rollout must be able to show. Each field is tri-state by
/// way of `Option`: `None` means the value was never observed, which is
/// reported as unavailable and never rendered as zero.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RolloutRecord {
    pub state: Option<RolloutStateHolder>,
    /// The container-minted identity. Absent means the rollout never got one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollout_id: Option<RolloutId>,
    /// Scored reward. `None` is "reward missing", not `0.0`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reward: Option<f64>,
    /// Sealed trace reference. `None` is "trace missing".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace_ref: Option<String>,
    /// Observed spend in micros. `None` is "cost unavailable".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_micros: Option<u64>,
    /// Observed token usage. `None` is "usage unavailable".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<u64>,
}

/// Newtype wrapper so a `RolloutRecord` deserialized from an older row without
/// a state field is legible as "state unknown" rather than defaulting to a
/// plausible one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RolloutStateHolder(pub RolloutState);

/// Why a declared rollout cannot count toward terminal success.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceGap {
    StateUnknown,
    StillInFlight,
    RolloutIdMissing,
    RewardMissing,
    TraceMissing,
    UsageMissing,
    CostMissing,
}

impl EvidenceGap {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StateUnknown => "state_unknown",
            Self::StillInFlight => "still_in_flight",
            Self::RolloutIdMissing => "rollout_id_missing",
            Self::RewardMissing => "reward_missing",
            Self::TraceMissing => "trace_missing",
            Self::UsageMissing => "usage_missing",
            Self::CostMissing => "cost_missing",
        }
    }
}

/// What the output contract demands of each rollout record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvidenceRequirements {
    pub requires_reward: bool,
    pub requires_trace: bool,
    pub requires_usage: bool,
}

impl RolloutRecord {
    /// Every reason this record falls short, in a stable order. An empty list
    /// means the rollout genuinely completed with all required evidence.
    pub fn evidence_gaps(&self, requirements: EvidenceRequirements) -> Vec<EvidenceGap> {
        let mut gaps = Vec::new();
        let Some(RolloutStateHolder(state)) = self.state else {
            gaps.push(EvidenceGap::StateUnknown);
            return gaps;
        };
        if state.is_in_flight() {
            gaps.push(EvidenceGap::StillInFlight);
            return gaps;
        }
        // A rollout that failed or was cancelled is a legitimate terminal
        // record. It is not required to carry a reward it never earned; it is
        // only required to be honest about that.
        if state != RolloutState::Completed {
            return gaps;
        }
        if self.rollout_id.is_none() {
            gaps.push(EvidenceGap::RolloutIdMissing);
        }
        if requirements.requires_reward && self.reward.is_none() {
            gaps.push(EvidenceGap::RewardMissing);
        }
        if requirements.requires_trace && self.trace_ref.is_none() {
            gaps.push(EvidenceGap::TraceMissing);
        }
        if requirements.requires_usage && self.total_tokens.is_none() {
            gaps.push(EvidenceGap::UsageMissing);
        }
        gaps
    }
}

// ---------------------------------------------------------------------------
// Run progress
// ---------------------------------------------------------------------------

/// The authoritative per-rollout progress of one run.
///
/// Constructed with every declared rollout already present in `Planned`, so a
/// rollout that never reported cannot be mistaken for one that was never
/// planned.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunProgress {
    pub state: RunState,
    /// Keyed by the rollout's index in the declared plan, which is the only
    /// identity a rollout has before the container mints one.
    pub rollouts: BTreeMap<u32, RolloutRecord>,
    /// Whether the credential capability minted for this run has been
    /// confirmed revoked. Unconfirmed revocation blocks `completed`.
    pub credential_revocation_confirmed: bool,
}

impl RunProgress {
    /// Plan `declared` rollouts. All start `Planned`; none start `Queued`,
    /// because nothing has been queued yet.
    pub fn plan(declared: usize) -> Self {
        let mut rollouts = BTreeMap::new();
        for index in 0..declared {
            rollouts.insert(
                index as u32,
                RolloutRecord {
                    state: Some(RolloutStateHolder(RolloutState::Planned)),
                    ..RolloutRecord::default()
                },
            );
        }
        Self {
            state: RunState::Draft,
            rollouts,
            credential_revocation_confirmed: false,
        }
    }

    pub fn declared_rollouts(&self) -> usize {
        self.rollouts.len()
    }

    /// Advance the run itself.
    pub fn transition_run(&mut self, next: RunState) -> Result<(), StateTransitionError> {
        // `Completed` is never reachable by transition alone; it is only
        // reachable through `settle`, which checks evidence first.
        self.state = self.state.transition_to(next)?;
        Ok(())
    }

    /// Advance one rollout.
    pub fn transition_rollout(
        &mut self,
        index: u32,
        next: RolloutState,
    ) -> Result<(), StateTransitionError> {
        let record = self.rollouts.entry(index).or_default();
        let current = match record.state {
            Some(RolloutStateHolder(state)) => state,
            // A record with no state cannot be advanced from a guess.
            None => {
                return Err(StateTransitionError {
                    code: AdmissionErrorCode::ExecutionSpecInvalid,
                    kind: TransitionKind::Rollout,
                    from: "unknown",
                    to: next.as_str(),
                    permitted: Vec::new(),
                })
            }
        };
        record.state = Some(RolloutStateHolder(current.transition_to(next)?));
        Ok(())
    }

    /// Attach observed evidence to a rollout. Never invents a value: a field
    /// left `None` by the caller stays `None`.
    pub fn record_evidence(&mut self, index: u32, update: RolloutRecord) {
        let record = self.rollouts.entry(index).or_default();
        if update.rollout_id.is_some() {
            record.rollout_id = update.rollout_id;
        }
        if update.reward.is_some() {
            record.reward = update.reward;
        }
        if update.trace_ref.is_some() {
            record.trace_ref = update.trace_ref;
        }
        if update.cost_micros.is_some() {
            record.cost_micros = update.cost_micros;
        }
        if update.total_tokens.is_some() {
            record.total_tokens = update.total_tokens;
        }
    }

    /// Rollouts that are queued, starting, or running.
    pub fn in_flight(&self) -> usize {
        self.rollouts
            .values()
            .filter(|record| {
                matches!(record.state, Some(RolloutStateHolder(state)) if state.is_in_flight())
            })
            .count()
    }

    /// Every gap that stands between this run and a truthful `completed`.
    pub fn completion_gaps(
        &self,
        requirements: EvidenceRequirements,
    ) -> BTreeMap<u32, Vec<EvidenceGap>> {
        let mut gaps = BTreeMap::new();
        for (index, record) in &self.rollouts {
            let found = record.evidence_gaps(requirements);
            if !found.is_empty() {
                gaps.insert(*index, found);
            }
        }
        gaps
    }

    /// Settle the run into its true terminal state.
    ///
    /// The declared rule, applied in order:
    ///
    /// 1. Any rollout still in flight → the run is not settleable at all.
    /// 2. Every rollout completed with all required evidence, and credential
    ///    revocation confirmed → `Completed`.
    /// 3. No rollout completed at all → `Failed`. Nothing was produced.
    /// 4. Otherwise → `Degraded`. Some compute succeeded but the run cannot
    ///    honestly claim a complete result.
    ///
    /// `Degraded` is deliberately the outcome for missing evidence rather than
    /// `Failed`: the compute did happen and was paid for, and reporting it as a
    /// clean failure would hide the spend.
    pub fn settle(
        &mut self,
        requirements: EvidenceRequirements,
    ) -> Result<RunState, SettlementRefusal> {
        if self.rollouts.is_empty() {
            return Err(SettlementRefusal::NoRolloutsDeclared);
        }
        let in_flight = self.in_flight();
        if in_flight > 0 {
            return Err(SettlementRefusal::RolloutsStillInFlight(in_flight));
        }
        let gaps = self.completion_gaps(requirements);
        let completed = self
            .rollouts
            .values()
            .filter(|record| {
                matches!(
                    record.state,
                    Some(RolloutStateHolder(RolloutState::Completed))
                )
            })
            .count();

        let target = if gaps.is_empty()
            && completed == self.rollouts.len()
            && self.credential_revocation_confirmed
        {
            RunState::Completed
        } else if completed == 0 {
            RunState::Failed
        } else {
            RunState::Degraded
        };
        self.state = self.state.transition_to(target).map_err(|error| {
            SettlementRefusal::InvalidTransition(Box::new(error))
        })?;
        Ok(self.state)
    }

    /// A truthful projection for the UI. Counts are per state, and every
    /// unavailable aggregate is `null` rather than `0`.
    pub fn project(&self, requirements: EvidenceRequirements) -> Value {
        let mut counts: BTreeMap<&'static str, u32> = BTreeMap::new();
        for record in self.rollouts.values() {
            let key = match record.state {
                Some(RolloutStateHolder(state)) => state.as_str(),
                None => "unknown",
            };
            *counts.entry(key).or_insert(0) += 1;
        }
        // An aggregate is only reported when every rollout that should have
        // contributed actually did. A partial sum presented as a total is the
        // estimate-as-actual failure in a different costume.
        let terminal: Vec<&RolloutRecord> = self
            .rollouts
            .values()
            .filter(|record| {
                record
                    .state
                    .is_some_and(|RolloutStateHolder(state)| state.is_terminal())
            })
            .collect();
        let total_cost_micros = if terminal.len() == self.rollouts.len()
            && terminal.iter().all(|record| record.cost_micros.is_some())
        {
            Some(
                terminal
                    .iter()
                    .map(|record| record.cost_micros.unwrap_or_default())
                    .sum::<u64>(),
            )
        } else {
            None
        };
        let total_tokens = if terminal.len() == self.rollouts.len()
            && terminal.iter().all(|record| record.total_tokens.is_some())
        {
            Some(
                terminal
                    .iter()
                    .map(|record| record.total_tokens.unwrap_or_default())
                    .sum::<u64>(),
            )
        } else {
            None
        };
        let mean_reward = if terminal.len() == self.rollouts.len()
            && terminal.iter().all(|record| record.reward.is_some())
        {
                let sum: f64 = terminal
                    .iter()
                    .map(|record| record.reward.unwrap_or_default())
                    .sum();
                Some(sum / terminal.len() as f64)
            } else {
                None
            };

        json!({
            "state": self.state.as_str(),
            "declaredRollouts": self.rollouts.len(),
            "rolloutStateCounts": counts,
            "inFlight": self.in_flight(),
            "credentialRevocationConfirmed": self.credential_revocation_confirmed,
            // `null` is the honest answer for an aggregate that is not yet
            // knowable. The UI renders it as "unavailable".
            "totalCostMicros": total_cost_micros,
            "totalTokens": total_tokens,
            "meanReward": mean_reward,
            "completionGaps": self
                .completion_gaps(requirements)
                .into_iter()
                .map(|(index, gaps)| {
                    (
                        index.to_string(),
                        gaps.into_iter().map(EvidenceGap::as_str).collect::<Vec<_>>(),
                    )
                })
                .collect::<BTreeMap<_, _>>(),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SettlementRefusal {
    NoRolloutsDeclared,
    RolloutsStillInFlight(usize),
    InvalidTransition(Box<StateTransitionError>),
}

impl fmt::Display for SettlementRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRolloutsDeclared => {
                formatter.write_str("a run with no declared rollouts cannot settle")
            }
            Self::RolloutsStillInFlight(count) => write!(
                formatter,
                "{count} rollout(s) are still queued, starting, or running; the run cannot settle yet"
            ),
            Self::InvalidTransition(error) => write!(formatter, "{error}"),
        }
    }
}

impl std::error::Error for SettlementRefusal {}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: EvidenceRequirements = EvidenceRequirements {
        requires_reward: true,
        requires_trace: true,
        requires_usage: true,
    };

    fn drive_to_running(progress: &mut RunProgress) {
        for next in [
            RunState::Validating,
            RunState::ReadyForApproval,
            RunState::AwaitingApproval,
            RunState::Admitted,
            RunState::Starting,
            RunState::Running,
        ] {
            progress.transition_run(next).unwrap();
        }
    }

    fn finish_rollout(progress: &mut RunProgress, index: u32) {
        for next in [
            RolloutState::Queued,
            RolloutState::Starting,
            RolloutState::Running,
            RolloutState::Completed,
        ] {
            progress.transition_rollout(index, next).unwrap();
        }
        progress.record_evidence(
            index,
            RolloutRecord {
                rollout_id: Some(RolloutId::new(format!("rollout-{index}")).unwrap()),
                reward: Some(0.5),
                trace_ref: Some(format!("trace-{index}")),
                cost_micros: Some(100_000),
                total_tokens: Some(1_234),
                ..RolloutRecord::default()
            },
        );
    }

    #[test]
    fn the_happy_path_walks_the_declared_states() {
        let mut progress = RunProgress::plan(5);
        assert_eq!(progress.state, RunState::Draft);
        drive_to_running(&mut progress);
        assert_eq!(progress.state, RunState::Running);
        for index in 0..5 {
            finish_rollout(&mut progress, index);
        }
        progress.credential_revocation_confirmed = true;
        assert_eq!(progress.settle(ALL).unwrap(), RunState::Completed);
    }

    #[test]
    fn invalid_run_transitions_are_rejected_not_coerced() {
        let mut progress = RunProgress::plan(1);
        // Skipping validation and approval to run is exactly the shortcut the
        // table exists to forbid.
        let error = progress.transition_run(RunState::Running).unwrap_err();
        assert_eq!(error.public_code(), "invalid_run_state_transition");
        assert_eq!(error.from, "draft");
        assert_eq!(error.to, "running");
        assert_eq!(progress.state, RunState::Draft, "state must not move");
    }

    #[test]
    fn a_terminal_run_cannot_be_reopened_by_a_late_writeback() {
        let mut progress = RunProgress::plan(1);
        drive_to_running(&mut progress);
        finish_rollout(&mut progress, 0);
        progress.credential_revocation_confirmed = true;
        assert_eq!(progress.settle(ALL).unwrap(), RunState::Completed);
        let error = progress.transition_run(RunState::Running).unwrap_err();
        assert_eq!(error.public_code(), "invalid_run_state_transition");
        assert!(error.permitted.is_empty());
    }

    #[test]
    fn a_rollout_must_leave_queued_when_it_starts() {
        let mut progress = RunProgress::plan(1);
        progress
            .transition_rollout(0, RolloutState::Queued)
            .unwrap();
        // Jumping straight from queued to running would hide the starting
        // phase, which is where container acquisition failures surface.
        let error = progress
            .transition_rollout(0, RolloutState::Running)
            .unwrap_err();
        assert_eq!(error.public_code(), "invalid_rollout_state_transition");
        progress
            .transition_rollout(0, RolloutState::Starting)
            .unwrap();
        progress
            .transition_rollout(0, RolloutState::Running)
            .unwrap();
    }

    #[test]
    fn completion_is_impossible_while_a_rollout_is_in_flight() {
        let mut progress = RunProgress::plan(2);
        drive_to_running(&mut progress);
        finish_rollout(&mut progress, 0);
        progress
            .transition_rollout(1, RolloutState::Queued)
            .unwrap();
        let refusal = progress.settle(ALL).unwrap_err();
        assert_eq!(refusal, SettlementRefusal::RolloutsStillInFlight(1));
        assert_eq!(progress.state, RunState::Running);
    }

    #[test]
    fn completion_is_impossible_with_a_missing_reward() {
        let mut progress = RunProgress::plan(1);
        drive_to_running(&mut progress);
        for next in [
            RolloutState::Queued,
            RolloutState::Starting,
            RolloutState::Running,
            RolloutState::Completed,
        ] {
            progress.transition_rollout(0, next).unwrap();
        }
        progress.record_evidence(
            0,
            RolloutRecord {
                rollout_id: Some(RolloutId::new("rollout-0").unwrap()),
                trace_ref: Some("trace-0".into()),
                total_tokens: Some(10),
                // reward deliberately absent
                ..RolloutRecord::default()
            },
        );
        progress.credential_revocation_confirmed = true;
        // Compute succeeded, evidence did not: degraded, never completed, and
        // never a reward of 0.0.
        assert_eq!(progress.settle(ALL).unwrap(), RunState::Degraded);
        let gaps = progress.completion_gaps(ALL);
        assert_eq!(gaps[&0], vec![EvidenceGap::RewardMissing]);
    }

    #[test]
    fn completion_is_impossible_while_credential_revocation_is_unconfirmed() {
        let mut progress = RunProgress::plan(1);
        drive_to_running(&mut progress);
        finish_rollout(&mut progress, 0);
        assert!(!progress.credential_revocation_confirmed);
        assert_eq!(progress.settle(ALL).unwrap(), RunState::Degraded);
    }

    #[test]
    fn a_run_where_nothing_completed_is_failed_not_degraded() {
        let mut progress = RunProgress::plan(2);
        drive_to_running(&mut progress);
        for index in 0..2 {
            progress
                .transition_rollout(index, RolloutState::Queued)
                .unwrap();
            progress
                .transition_rollout(index, RolloutState::Failed)
                .unwrap();
        }
        progress.credential_revocation_confirmed = true;
        assert_eq!(progress.settle(ALL).unwrap(), RunState::Failed);
    }

    #[test]
    fn a_failed_rollout_is_a_valid_terminal_record_without_a_reward() {
        let mut progress = RunProgress::plan(2);
        drive_to_running(&mut progress);
        finish_rollout(&mut progress, 0);
        progress
            .transition_rollout(1, RolloutState::Queued)
            .unwrap();
        progress
            .transition_rollout(1, RolloutState::Failed)
            .unwrap();
        progress.credential_revocation_confirmed = true;
        // One completed, one honestly failed: degraded, because the run cannot
        // claim a complete result over all five declared seeds.
        assert_eq!(progress.settle(ALL).unwrap(), RunState::Degraded);
        assert!(
            progress.completion_gaps(ALL).is_empty(),
            "a failed rollout owes no reward, so it registers no evidence gap"
        );
    }

    #[test]
    fn missing_aggregates_project_as_null_never_zero() {
        let mut progress = RunProgress::plan(2);
        drive_to_running(&mut progress);
        finish_rollout(&mut progress, 0);
        // Second rollout completes with no cost telemetry at all.
        for next in [
            RolloutState::Queued,
            RolloutState::Starting,
            RolloutState::Running,
            RolloutState::Completed,
        ] {
            progress.transition_rollout(1, next).unwrap();
        }
        progress.record_evidence(
            1,
            RolloutRecord {
                rollout_id: Some(RolloutId::new("rollout-1").unwrap()),
                reward: Some(0.25),
                trace_ref: Some("trace-1".into()),
                total_tokens: Some(5),
                ..RolloutRecord::default()
            },
        );
        let projected = progress.project(ALL);
        assert!(
            projected["totalCostMicros"].is_null(),
            "a partial cost sum must not be presented as the total"
        );
        assert_eq!(projected["totalTokens"], json!(1_239));
        assert_eq!(projected["rolloutStateCounts"]["completed"], json!(2));
    }

    #[test]
    fn an_unstated_rollout_state_is_unknown_rather_than_planned() {
        let mut progress = RunProgress::plan(0);
        progress.rollouts.insert(7, RolloutRecord::default());
        let gaps = progress.completion_gaps(ALL);
        assert_eq!(gaps[&7], vec![EvidenceGap::StateUnknown]);
        let error = progress
            .transition_rollout(7, RolloutState::Running)
            .unwrap_err();
        assert_eq!(error.from, "unknown");
    }

    #[test]
    fn every_state_name_is_stable_on_the_wire() {
        for (state, expected) in [
            (RunState::ReadyForApproval, "ready_for_approval"),
            (RunState::AwaitingApproval, "awaiting_approval"),
            (RunState::Degraded, "degraded"),
        ] {
            assert_eq!(state.as_str(), expected);
            assert_eq!(serde_json::to_value(state).unwrap(), json!(expected));
        }
        for (state, expected) in [
            (RolloutState::Planned, "planned"),
            (RolloutState::Queued, "queued"),
            (RolloutState::Degraded, "degraded"),
        ] {
            assert_eq!(state.as_str(), expected);
            assert_eq!(serde_json::to_value(state).unwrap(), json!(expected));
        }
    }
}
