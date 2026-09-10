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
            Self::Starting => &[Self::Running, Self::Failed, Self::Cancelled, Self::Degraded],
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

    /// Execution progress after admission has already been consumed.
    ///
    /// Admission lives on `optimizer_run_drafts`. Once the optimizer run exists,
    /// this record starts at `Starting` with rollouts `Queued` — it does not
    /// replay Draft → Admitted on the execution aggregate.
    pub fn for_admitted_execution(declared: usize) -> Self {
        let mut rollouts = BTreeMap::new();
        for index in 0..declared {
            rollouts.insert(
                index as u32,
                RolloutRecord {
                    state: Some(RolloutStateHolder(RolloutState::Queued)),
                    ..RolloutRecord::default()
                },
            );
        }
        Self {
            state: RunState::Starting,
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
        self.state = self
            .state
            .transition_to(target)
            .map_err(|error| SettlementRefusal::InvalidTransition(Box::new(error)))?;
        Ok(self.state)
    }

    /// Pre-dispatch or worker abort: every nonterminal child becomes terminal
    /// before the parent does. A failed campaign cannot keep queued rows.
    pub fn fail_pre_dispatch(&mut self) -> Result<(), StateTransitionError> {
        let indexes: Vec<u32> = self.rollouts.keys().copied().collect();
        for index in indexes {
            let Some(RolloutStateHolder(state)) = self.rollouts[&index].state else {
                continue;
            };
            if state.is_terminal() {
                continue;
            }
            let next = if state.may_transition_to(RolloutState::Cancelled) {
                RolloutState::Cancelled
            } else {
                RolloutState::Failed
            };
            self.transition_rollout(index, next)?;
        }
        if !self.state.is_terminal() {
            self.transition_run(RunState::Failed)?;
        }
        Ok(())
    }

    pub fn reconciliation_errors(&self) -> Vec<String> {
        let mut errors = Vec::new();
        let nonterminal = self
            .rollouts
            .values()
            .filter(|record| {
                record
                    .state
                    .is_none_or(|RolloutStateHolder(state)| !state.is_terminal())
            })
            .count();
        if self.state.is_terminal() && nonterminal > 0 {
            errors.push(format!(
                "This run is {}, but {nonterminal} {} still queued or running.",
                self.state.as_str(),
                if nonterminal == 1 {
                    "rollout is"
                } else {
                    "rollouts are"
                }
            ));
        }
        let counted = self.rollouts.len();
        if counted == 0 {
            errors.push("This run has no rollout plan.".into());
        }
        errors
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
            "reconciliationErrors": self.reconciliation_errors(),
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

