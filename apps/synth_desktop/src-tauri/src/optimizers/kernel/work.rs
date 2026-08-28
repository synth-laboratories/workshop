//! Work items owned by the kernel, not by an external producer identity.

use serde::{Deserialize, Serialize};

use super::error::{KernelError, KernelErrorCode, KernelResult};
use super::types::{TerminalKind, WorkItemKind, WorkItemLifecycle};
use crate::optimizers::models::OptimizerRunArtifact;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkItem {
    pub work_item_id: String,
    pub kind: WorkItemKind,
    pub lifecycle: WorkItemLifecycle,
    #[serde(default)]
    pub terminal: Option<TerminalKind>,
    #[serde(default)]
    pub external_ref: Option<String>,
    /// Query-time artifact chips. The kernel never derives these from raw
    /// payloads; the run-view service joins the durable artifact index.
    #[serde(default)]
    pub artifact_refs: Vec<OptimizerRunArtifact>,
}

impl WorkItem {
    pub fn planned(work_item_id: impl Into<String>, kind: WorkItemKind) -> KernelResult<Self> {
        let work_item_id = work_item_id.into();
        if work_item_id.trim().is_empty() {
            return Err(KernelError::new(
                KernelErrorCode::WorkItemIdentityMissing,
                "work item identity must be assigned before dispatch",
            ));
        }
        Ok(Self {
            work_item_id,
            kind,
            lifecycle: WorkItemLifecycle::Planned,
            terminal: None,
            external_ref: None,
            artifact_refs: vec![],
        })
    }

    pub fn transition(&mut self, next: WorkItemLifecycle) -> KernelResult<()> {
        self.lifecycle = self.lifecycle.transition_to(next)?;
        if next == WorkItemLifecycle::Terminal && self.terminal.is_none() {
            return Err(KernelError::new(
                KernelErrorCode::WorkItemTransitionInvalid,
                format!(
                    "work item {} reached terminal without a terminal kind",
                    self.work_item_id
                ),
            ));
        }
        Ok(())
    }

    pub fn seal_terminal(&mut self, kind: TerminalKind) -> KernelResult<()> {
        self.lifecycle = self.lifecycle.transition_to(WorkItemLifecycle::Terminal)?;
        self.terminal = Some(kind);
        Ok(())
    }

    pub fn bind_external_ref(&mut self, external_ref: String) -> KernelResult<()> {
        if self.lifecycle == WorkItemLifecycle::Planned {
            return Err(KernelError::new(
                KernelErrorCode::WorkItemIdentityMissing,
                format!(
                    "work item {} must keep its internal identity before an external ref is bound",
                    self.work_item_id
                ),
            ));
        }
        if let Some(existing) = &self.external_ref {
            if existing != &external_ref {
                return Err(KernelError::new(
                    KernelErrorCode::WorkItemIdentityMissing,
                    format!(
                        "work item {} already bound to {existing}, refusing {external_ref}",
                        self.work_item_id
                    ),
                ));
            }
        }
        self.external_ref = Some(external_ref);
        Ok(())
    }
}

/// Seal every nonterminal work item as `cancelled`.
///
/// Interrupted children did not fail; they were cut short by the run's own
/// terminal fact, and `cancelled` is the only honest spelling of that. Planned
/// work that never dispatched closes the same way: a sealed run may not carry
/// open work of any lifecycle. Pure over the items, so calling it from the
/// reducer's seal step makes closure a function of the terminal event that
/// replay reproduces.
pub fn close_open_items(items: &mut [WorkItem]) -> KernelResult<usize> {
    let mut closed = 0usize;
    for item in items.iter_mut() {
        if item.lifecycle != WorkItemLifecycle::Terminal {
            item.seal_terminal(TerminalKind::Cancelled)?;
            closed += 1;
        }
    }
    Ok(closed)
}

/// Counts a projection may report. Missing stays `None`; it is never zero.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkSummary {
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub planned: Option<u64>,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub queued: Option<u64>,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub running: Option<u64>,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub succeeded: Option<u64>,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub failed: Option<u64>,
    #[serde(default)]
    #[specta(type = specta_typescript::Number)]
    pub cancelled: Option<u64>,
    #[serde(default)]
    pub unit: Option<String>,
    /// False when the algorithm's work ceiling is a budget, not a fixed plan.
    #[serde(default)]
    pub fixed_denominator: bool,
}

impl WorkSummary {
    pub fn from_items(items: &[WorkItem], unit: &str, fixed_denominator: bool) -> Self {
        if items.is_empty() {
            return Self {
                unit: Some(unit.to_string()),
                fixed_denominator,
                ..Self::default()
            };
        }
        let mut queued = 0u64;
        let mut running = 0u64;
        let mut succeeded = 0u64;
        let mut failed = 0u64;
        let mut cancelled = 0u64;
        for item in items {
            match item.lifecycle {
                WorkItemLifecycle::Planned | WorkItemLifecycle::Queued => queued += 1,
                WorkItemLifecycle::Starting | WorkItemLifecycle::Running => running += 1,
                WorkItemLifecycle::Terminal => match item.terminal {
                    Some(TerminalKind::Completed) => succeeded += 1,
                    Some(TerminalKind::Cancelled) => cancelled += 1,
                    Some(TerminalKind::Failed) | Some(TerminalKind::Degraded) | None => failed += 1,
                },
            }
        }
        Self {
            planned: Some(items.len() as u64),
            queued: Some(queued),
            running: Some(running),
            succeeded: Some(succeeded),
            failed: Some(failed),
            cancelled: Some(cancelled),
            unit: Some(unit.to_string()),
            fixed_denominator,
        }
    }
}

