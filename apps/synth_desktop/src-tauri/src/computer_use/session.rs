//! The admission pipeline: what has to be true before an action reaches the
//! helper, and what the operator has to be asked.
//!
//! This module is deliberately pure. It decides; it does not perform. Approval
//! needs an `AppHandle` and the broker, and driving needs a subprocess, and
//! both of those make a decision impossible to test. So `plan` returns what
//! must happen and the service layer makes it happen.
//!
//! Order matters and is not arbitrary. Structure, then lock, then hard policy,
//! then index freshness, then consent. Consent is last because asking a person
//! to approve something that was going to be refused anyway trains them to
//! click through cards.

use super::allowlist::AppAllowlist;
use super::lock::{Admission, LockGuard};
use super::policy::{classify_app, hazard, HazardReason};
use super::vocabulary::Action;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Wait after an action before capturing the next state. The agent must never
/// sleep — settling is the runtime's job, per §5.
pub const SETTLE_MIN_MS: u64 = 1_000;
/// Extended while a loading indicator or state churn is still visible.
pub const SETTLE_MAX_MS: u64 = 5_000;

/// Typed refusal codes. An agent can branch on these; prose it can only relay.
pub const CODE_INVALID_ACTION: &str = "invalid_action";
pub const CODE_APP_DENIED: &str = "app_denied";
pub const CODE_SESSION_PAUSED: &str = "session_paused";
pub const CODE_SESSION_ENDED: &str = "session_ended";
pub const CODE_NEEDS_FULL_READ: &str = "needs_full_read";
pub const CODE_STALE_ELEMENT_INDEX: &str = "stale_element_index";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Refusal {
    pub code: String,
    pub message: String,
    /// What the agent should do instead. Empty when there is nothing useful to
    /// suggest, which is better than inventing a suggestion.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub remediation: String,
}

impl Refusal {
    fn new(code: &str, message: impl Into<String>, remediation: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            remediation: remediation.into(),
        }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.message)
    }
}

impl std::error::Error for Refusal {}

/// The approval this action needs before it may run.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRequest {
    pub app: String,
    pub action: String,
    /// Redacted, and concrete: the recipient and the text, not "may use Mail".
    pub payload: Value,
    pub hazard: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element_index: Option<u64>,
    /// Whether the card may offer `This session` and `Always`. False for hazard
    /// actions: the consent is about this payload, so remembering it would
    /// answer a question nobody asked.
    pub remembered_allowed: bool,
}

/// What must happen for this action, decided but not yet done.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub action: Action,
    pub hazard: Option<HazardReason>,
    /// `None` when the app is already allowlisted and the action is ordinary.
    pub approval: Option<ApprovalRequest>,
    /// A non-diffed read, the one thing that clears the post-unlock hold.
    pub is_full_read: bool,
}

/// What the helper reported for one app, kept so hazard classification can use
/// the element's real label rather than the agent's description of it.
#[derive(Clone, Debug, Default)]
pub struct AppSnapshot {
    pub labels: HashMap<u64, String>,
    pub element_count: u64,
}

pub struct ComputerUseSession {
    session_id: String,
    run_id: String,
    lock: LockGuard,
    allowlist: Arc<AppAllowlist>,
    /// Apps whose tree has been read since their last mutation. Element indexes
    /// are only meaningful for apps in this set.
    read_since_mutation: HashSet<String>,
    snapshots: HashMap<String, AppSnapshot>,
    /// G10: whether this run ever needed pixels.
    used_coordinates: bool,
}

impl ComputerUseSession {
    pub fn new(
        session_id: impl Into<String>,
        run_id: impl Into<String>,
        allowlist: Arc<AppAllowlist>,
        lock: LockGuard,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            run_id: run_id.into(),
            lock,
            allowlist,
            read_since_mutation: HashSet::new(),
            snapshots: HashMap::new(),
            used_coordinates: false,
        }
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }

    pub fn lock_mut(&mut self) -> &mut LockGuard {
        &mut self.lock
    }

    pub fn lock(&self) -> &LockGuard {
        &self.lock
    }

    /// True when this run has completed entirely through element indexes. G10.
    pub fn element_indexed_only(&self) -> bool {
        !self.used_coordinates
    }

    /// Decide what this action needs. Never mutates session state on refusal —
    /// a refused action must leave the session exactly as it found it, or a
    /// denied app could still invalidate another app's indexes.
    pub fn plan(&self, action: Action) -> Result<Plan, Refusal> {
        action.validate().map_err(|error| {
            Refusal::new(
                CODE_INVALID_ACTION,
                error.to_string(),
                "Fix the arguments and call again.",
            )
        })?;

        let is_full_read = matches!(
            action,
            Action::GetAppState {
                disable_diff: true,
                ..
            }
        );

        match self.lock.admit(is_full_read) {
            Admission::Allow => {}
            Admission::RequireFullRead => {
                return Err(Refusal::new(
                    CODE_NEEDS_FULL_READ,
                    "the screen was locked; every cached element index, screenshot, and coordinate is stale",
                    "Call get_app_state with disable_diff set before acting.",
                ))
            }
            Admission::Refuse { reason } => {
                let code = if self.lock.is_terminal() {
                    CODE_SESSION_ENDED
                } else {
                    CODE_SESSION_PAUSED
                };
                return Err(Refusal::new(code, reason, ""));
            }
        }

        let Some(app) = action.app() else {
            // `list_apps` targets nothing and reveals only what is running.
            return Ok(Plan {
                action,
                hazard: None,
                approval: None,
                is_full_read,
            });
        };
        let app = app.to_owned();

        if let Some(reason) = classify_app(&app).denial() {
            return Err(Refusal::new(
                CODE_APP_DENIED,
                format!("`{app}` cannot be driven"),
                reason.explain(),
            ));
        }

        // An index from before the last mutation points at whatever now
        // occupies that slot. Refusing is the difference between "clicked the
        // wrong button" and "clicked a button that moved under us".
        if action.element_index().is_some() && !self.read_since_mutation.contains(&app) {
            return Err(Refusal::new(
                CODE_STALE_ELEMENT_INDEX,
                format!("element indexes for `{app}` were invalidated by the last action"),
                "Call get_app_state for this app, then use an index from that tree.",
            ));
        }

        let label = action
            .element_index()
            .and_then(|index| self.snapshots.get(&app)?.labels.get(&index))
            .map(String::as_str);
        let hazard = hazard(&app, &action, label);

        let allowed = self.allowlist.is_allowed(&app, &self.session_id);
        let approval = if hazard.is_some() || !allowed {
            Some(ApprovalRequest {
                app: app.clone(),
                action: action.verb().to_owned(),
                payload: action.approval_payload(),
                hazard: hazard.is_some(),
                element_index: action.element_index(),
                // A hazard action is never remembered; an app-scope grant is
                // exactly the thing that is.
                remembered_allowed: hazard.is_none(),
            })
        } else {
            None
        };

        Ok(Plan {
            action,
            hazard,
            approval,
            is_full_read,
        })
    }

    /// Record that an action ran. Called only after the helper actually acted,
    /// so a failed call does not invalidate indexes that are still good.
    pub fn observe_performed(&mut self, action: &Action) {
        if let Some(app) = action.app() {
            if !action.is_read_only() {
                // Any UI change invalidates every index for that app, including
                // the one just used.
                self.read_since_mutation.remove(app);
                self.snapshots.remove(app);
            }
        }
        if action.uses_coordinates() {
            self.used_coordinates = true;
        }
    }

    /// Record the result of a read. This is what makes indexes usable again.
    pub fn observe_read(&mut self, app: &str, snapshot: AppSnapshot, was_full_read: bool) {
        self.read_since_mutation.insert(app.to_owned());
        self.snapshots.insert(app.to_owned(), snapshot);
        if was_full_read {
            // Only a full read clears the post-unlock hold: a diff against a
            // tree captured before the lock is a diff against a fiction.
            let _ = self.lock.observed_full_read();
        }
    }

    /// Apps this session may drive without a fresh card — the chip in §6.
    pub fn allowlisted_apps(&self) -> Vec<String> {
        self.allowlist.apps_for(&self.session_id)
    }
}

