//! Fresh read DTO from backend source f67bc0fb204cb4f93c680e8a702657e9c8d99bbb.
//! This is not a stop receipt, identity lease, or qualified live profile.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum Coverage {
    #[serde(rename = "untracked")]
    Untracked,
    #[serde(rename = "explicit-v1")]
    ExplicitV1,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    RootTree,
    OwnedSubtree,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceSettlement {
    pub run_id: String,
    /// Server observation time. The host owns qualified clock/freshness policy.
    pub observed_at: String,
    pub coverage: Coverage,
    pub settled: bool,
    #[serde(default)]
    pub registered_tree_settled: bool,
    #[serde(default)]
    pub coverage_complete: bool,
    pub scope_kind: Option<ScopeKind>,
    pub root_run_id: Option<String>,
    pub edge_id: Option<String>,
    pub pending: Option<u64>,
    pub unknown: Option<u64>,
    pub confirmed: Option<u64>,
    pub excluded: Option<u64>,
    pub root_confirmed: Option<bool>,
}
impl ResourceSettlement {
    pub fn validate_for_run(&self, run_id: &str) -> Result<(), &'static str> {
        if self.run_id != run_id || self.observed_at.trim().is_empty() {
            return Err("settlement observation identity is invalid");
        }
        if self.settled
            && (self.coverage != Coverage::ExplicitV1
                || !self.coverage_complete
                || !self.registered_tree_settled
                || self.pending != Some(0)
                || self.unknown != Some(0)
                || (self.scope_kind == Some(ScopeKind::RootTree)
                    && self.root_confirmed != Some(true)))
        {
            return Err("settlement observation contradicts ownership coverage");
        }
        if self.coverage == Coverage::ExplicitV1 {
            if self.root_run_id.as_deref().is_none_or(str::is_empty) {
                return Err("tracked settlement lacks its root");
            }
            match self.scope_kind {
                Some(ScopeKind::RootTree)
                    if self.root_run_id.as_deref() == Some(run_id) && self.edge_id.is_none() => {}
                Some(ScopeKind::OwnedSubtree)
                    if self.edge_id.as_deref().is_some_and(|id| !id.is_empty()) => {}
                _ => return Err("tracked settlement scope is invalid"),
            }
        }
        Ok(())
    }
    /// Reports only what this observation proves. Callers must separately
    /// qualify profile, account, receipt freshness and the requested root.
    pub fn reports_settled_root(&self) -> bool {
        self.validate_for_run(&self.run_id).is_ok()
            && self.settled
            && self.scope_kind == Some(ScopeKind::RootTree)
    }
}
