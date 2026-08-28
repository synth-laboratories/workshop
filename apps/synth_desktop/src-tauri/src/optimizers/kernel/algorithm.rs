//! Algorithm-owned events, projections, and results.
//!
//! Central services dispatch through this closed enum. Adding an algorithm is
//! adding a variant and its module, not another match in `service.rs`.

use serde::{Deserialize, Serialize};

use super::error::{KernelError, KernelErrorCode, KernelResult};
use super::evidence::{EvidenceState, UsageCompleteness};
use super::sequences::CommittedEvent;
use super::types::AlgorithmKind;
use super::work::{WorkItem, WorkSummary};
use super::{algorithms, types::EvidenceCompleteness};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "algorithm", rename_all = "kebab-case")]
pub enum AlgorithmProjection {
    Eval(algorithms::eval::EvalProjection),
    Gepa(algorithms::gepa::GepaProjection),
    GoEx(algorithms::go_ex::GoExProjection),
    Sft(algorithms::sft::SftProjection),
    Cispo(algorithms::cispo::CispoProjection),
}

impl AlgorithmProjection {
    pub fn new(kind: AlgorithmKind) -> Self {
        match kind {
            AlgorithmKind::Eval => Self::Eval(algorithms::eval::EvalProjection::default()),
            AlgorithmKind::Gepa => Self::Gepa(algorithms::gepa::GepaProjection::default()),
            AlgorithmKind::GoEx => Self::GoEx(algorithms::go_ex::GoExProjection::default()),
            AlgorithmKind::Sft => Self::Sft(algorithms::sft::SftProjection::default()),
            AlgorithmKind::Cispo => Self::Cispo(algorithms::cispo::CispoProjection::default()),
        }
    }

    pub fn kind(&self) -> AlgorithmKind {
        match self {
            Self::Eval(_) => AlgorithmKind::Eval,
            Self::Gepa(_) => AlgorithmKind::Gepa,
            Self::GoEx(_) => AlgorithmKind::GoEx,
            Self::Sft(_) => AlgorithmKind::Sft,
            Self::Cispo(_) => AlgorithmKind::Cispo,
        }
    }

    pub fn apply(&mut self, event: &CommittedEvent) -> KernelResult<()> {
        let offered = event.producer.algorithm()?;
        if offered != self.kind() {
            return Err(KernelError::new(
                KernelErrorCode::EventSchemaMismatch,
                format!(
                    "event algorithm {} does not match projection {}",
                    offered.wire_id(),
                    self.kind().wire_id()
                ),
            ));
        }
        match self {
            Self::Eval(projection) => projection.apply(event),
            Self::Gepa(projection) => projection.apply(event),
            Self::GoEx(projection) => projection.apply(event),
            Self::Sft(projection) => projection.apply(event),
            Self::Cispo(projection) => projection.apply(event),
        }
    }

    pub fn work_items(&self) -> &[WorkItem] {
        match self {
            Self::Eval(p) => &p.work_items,
            Self::Gepa(p) => &p.work_items,
            Self::GoEx(p) => &p.work_items,
            Self::Sft(p) => &p.work_items,
            Self::Cispo(p) => &p.work_items,
        }
    }

    pub fn work_summary(&self) -> WorkSummary {
        match self {
            Self::Eval(p) => p.work_summary(),
            Self::Gepa(p) => p.work_summary(),
            Self::GoEx(p) => p.work_summary(),
            Self::Sft(p) => p.work_summary(),
            Self::Cispo(p) => p.work_summary(),
        }
    }

    /// Close every open work item at the run's terminal seal. Interrupted
    /// children settle `cancelled` — they did not fail — and so does planned
    /// work that never dispatched; a sealed projection is closed-world.
    pub fn close_open_work(&mut self) -> KernelResult<usize> {
        match self {
            Self::Eval(p) => p.close_open_work(),
            Self::Gepa(p) => p.close_open_work(),
            Self::GoEx(p) => p.close_open_work(),
            Self::Sft(p) => p.close_open_work(),
            Self::Cispo(p) => p.close_open_work(),
        }
    }

    pub fn evidence_state(&self) -> EvidenceState {
        match self {
            Self::Eval(p) => p.evidence_state(),
            Self::Gepa(p) => p.evidence_state(),
            Self::GoEx(p) => p.evidence_state(),
            Self::Sft(p) => p.evidence_state(),
            Self::Cispo(p) => p.evidence_state(),
        }
    }

    pub fn usage(&self) -> UsageCompleteness {
        match self {
            Self::Eval(p) => p.usage.clone(),
            Self::Gepa(p) => p.usage.clone(),
            Self::GoEx(p) => p.usage.clone(),
            Self::Sft(p) => p.usage.clone(),
            Self::Cispo(p) => p.usage.clone(),
        }
    }

    pub fn phase(&self) -> Option<super::types::RunPhase> {
        match self {
            Self::Eval(p) => p.phase,
            Self::Gepa(p) => p.phase,
            Self::GoEx(p) => p.phase,
            Self::Sft(p) => p.phase,
            Self::Cispo(p) => p.phase,
        }
    }

    pub fn settle(&self) -> KernelResult<AlgorithmResult> {
        match self {
            Self::Eval(p) => p.settle().map(AlgorithmResult::Eval),
            Self::Gepa(p) => p.settle().map(AlgorithmResult::Gepa),
            Self::GoEx(p) => p.settle().map(AlgorithmResult::GoEx),
            Self::Sft(p) => p.settle().map(AlgorithmResult::Sft),
            Self::Cispo(p) => p.settle().map(AlgorithmResult::Cispo),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "algorithm", rename_all = "kebab-case")]
pub enum AlgorithmResult {
    Eval(algorithms::eval::EvalResult),
    Gepa(algorithms::gepa::GepaResult),
    GoEx(algorithms::go_ex::GoExResult),
    Sft(algorithms::sft::SftResult),
    Cispo(algorithms::cispo::CispoResult),
}

impl AlgorithmResult {
    pub const fn kind(&self) -> AlgorithmKind {
        match self {
            Self::Eval(_) => AlgorithmKind::Eval,
            Self::Gepa(_) => AlgorithmKind::Gepa,
            Self::GoEx(_) => AlgorithmKind::GoEx,
            Self::Sft(_) => AlgorithmKind::Sft,
            Self::Cispo(_) => AlgorithmKind::Cispo,
        }
    }
}

pub fn decode_registered(algorithm_id: &str) -> KernelResult<AlgorithmKind> {
    AlgorithmKind::parse_wire(algorithm_id)
}

pub fn evidence_from_work(items: &[WorkItem], traces: usize) -> EvidenceState {
    if items.is_empty() && traces == 0 {
        return EvidenceState::absent();
    }
    let terminal = items
        .iter()
        .filter(|item| item.lifecycle == super::types::WorkItemLifecycle::Terminal)
        .count();
    let completeness = if terminal == items.len() && traces > 0 {
        EvidenceCompleteness::Complete
    } else if terminal > 0 || traces > 0 {
        EvidenceCompleteness::Partial
    } else {
        EvidenceCompleteness::Absent
    };
    EvidenceState {
        completeness,
        reason: None,
        refs: Vec::new(),
    }
}
