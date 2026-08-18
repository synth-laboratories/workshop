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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::approval::ApprovalScope;
    use serde_json::json;
    use tempfile::tempdir;

    fn action(value: Value) -> Action {
        serde_json::from_value(value).unwrap()
    }

    struct Fixture {
        session: ComputerUseSession,
        allowlist: Arc<AppAllowlist>,
        _dir: tempfile::TempDir,
    }

    fn fixture() -> Fixture {
        let dir = tempdir().unwrap();
        let allowlist = Arc::new(AppAllowlist::open(dir.path().join("allowlist.json")));
        let session = ComputerUseSession::new(
            "session-1",
            "run-1",
            allowlist.clone(),
            LockGuard::with_default_ceiling(),
        );
        Fixture {
            session,
            allowlist,
            _dir: dir,
        }
    }

    fn allow(fixture: &Fixture, app: &str) {
        fixture
            .allowlist
            .record(
                app,
                &ApprovalScope::Workspace,
                "session-1",
                "approval-1",
                "2026-08-16T00:00:00Z".into(),
            )
            .unwrap();
    }

    fn read(fixture: &mut Fixture, app: &str, labels: &[(u64, &str)]) {
        fixture.session.observe_read(
            app,
            AppSnapshot {
                labels: labels
                    .iter()
                    .map(|(index, label)| (*index, (*label).to_owned()))
                    .collect(),
                element_count: labels.len() as u64,
            },
            false,
        );
    }

    #[test]
    fn a_first_action_on_an_unapproved_app_raises_a_card() {
        let fixture = fixture();
        let plan = fixture
            .session
            .plan(action(
                json!({"verb":"get_app_state","app":"com.apple.mail"}),
            ))
            .unwrap();
        let approval = plan.approval.unwrap();
        assert_eq!(approval.app, "com.apple.mail");
        assert!(!approval.hazard);
        // An app-scope grant is exactly the kind that may be remembered.
        assert!(approval.remembered_allowed);
    }

    #[test]
    fn an_allowlisted_app_needs_no_card_for_an_ordinary_action() {
        let mut fixture = fixture();
        allow(&fixture, "com.figma.desktop");
        read(&mut fixture, "com.figma.desktop", &[(1, "Zoom In")]);
        let plan = fixture
            .session
            .plan(action(
                json!({"verb":"click","app":"com.figma.desktop","element_index":1}),
            ))
            .unwrap();
        assert!(plan.approval.is_none());
        assert!(plan.hazard.is_none());
    }

    /// G6. An allowlisted app does not make its Send button ordinary.
    #[test]
    fn a_hazard_action_raises_a_payload_bound_card_even_in_an_allowlisted_app() {
        let mut fixture = fixture();
        allow(&fixture, "com.apple.mail");
        read(&mut fixture, "com.apple.mail", &[(9, "Send")]);
        let plan = fixture
            .session
            .plan(action(
                json!({"verb":"click","app":"com.apple.mail","element_index":9}),
            ))
            .unwrap();
        let approval = plan.approval.expect("hazard actions always ask");
        assert!(approval.hazard);
        // Never remembered: the consent is about this payload.
        assert!(!approval.remembered_allowed);
        assert_eq!(approval.element_index, Some(9));
        assert!(matches!(
            plan.hazard,
            Some(HazardReason::IrreversibleControl { .. })
        ));
    }

    /// The label comes from the last read, not from the agent. An agent that
    /// simply never reads cannot make a Send button look ordinary — it gets
    /// refused for staleness instead.
    #[test]
    fn hazard_detection_cannot_be_dodged_by_not_reading() {
        let fixture = fixture();
        allow(&fixture, "com.apple.mail");
        let refusal = fixture
            .session
            .plan(action(
                json!({"verb":"click","app":"com.apple.mail","element_index":9}),
            ))
            .unwrap_err();
        assert_eq!(refusal.code, CODE_STALE_ELEMENT_INDEX);
    }

    #[test]
    fn terminal_class_apps_refuse_regardless_of_any_grant() {
        let mut fixture = fixture();
        // Even with a grant forced into the file, the app class refuses.
        let _ = fixture.allowlist.record(
            "com.apple.Terminal",
            &ApprovalScope::Workspace,
            "session-1",
            "a",
            "t".into(),
        );
        read(&mut fixture, "com.apple.Terminal", &[(1, "Shell")]);
        let refusal = fixture
            .session
            .plan(action(
                json!({"verb":"type_text","app":"com.apple.Terminal","text":"rm -rf /"}),
            ))
            .unwrap_err();
        assert_eq!(refusal.code, CODE_APP_DENIED);
        assert!(refusal.remediation.contains("shell tool"), "{refusal:?}");
    }

    #[test]
    fn an_index_is_refused_after_the_ui_was_mutated() {
        let mut fixture = fixture();
        allow(&fixture, "com.figma.desktop");
        read(&mut fixture, "com.figma.desktop", &[(1, "Zoom In")]);
        let click = action(json!({"verb":"click","app":"com.figma.desktop","element_index":1}));
        fixture.session.plan(click.clone()).unwrap();
        fixture.session.observe_performed(&click);
        let refusal = fixture.session.plan(click).unwrap_err();
        assert_eq!(refusal.code, CODE_STALE_ELEMENT_INDEX);
        assert!(refusal.remediation.contains("get_app_state"));
    }

    /// A mutation in one app must not invalidate another app's indexes; that
    /// would make multi-app tasks impossible for no safety benefit.
    #[test]
    fn mutating_one_app_leaves_another_apps_indexes_alone() {
        let mut fixture = fixture();
        allow(&fixture, "com.figma.desktop");
        allow(&fixture, "com.apple.notes");
        read(&mut fixture, "com.figma.desktop", &[(1, "Zoom In")]);
        read(&mut fixture, "com.apple.notes", &[(1, "New Note")]);
        let typed = action(json!({"verb":"type_text","app":"com.figma.desktop","text":"x"}));
        fixture.session.observe_performed(&typed);
        fixture
            .session
            .plan(action(
                json!({"verb":"click","app":"com.apple.notes","element_index":1}),
            ))
            .unwrap();
    }

    /// A refused action must leave the session exactly as it found it.
    #[test]
    fn planning_never_mutates_session_state() {
        let mut fixture = fixture();
        allow(&fixture, "com.figma.desktop");
        read(&mut fixture, "com.figma.desktop", &[(1, "Zoom In")]);
        let denied = action(json!({"verb":"type_text","app":"com.apple.Terminal","text":"x"}));
        assert!(fixture.session.plan(denied).is_err());
        // Figma's indexes are untouched.
        fixture
            .session
            .plan(action(
                json!({"verb":"click","app":"com.figma.desktop","element_index":1}),
            ))
            .unwrap();
    }

    #[test]
    fn a_locked_screen_refuses_everything_and_unlock_demands_a_full_read() {
        let mut fixture = fixture();
        allow(&fixture, "com.figma.desktop");
        read(&mut fixture, "com.figma.desktop", &[(1, "Zoom In")]);
        let now = chrono::Utc::now();
        fixture.session.lock_mut().on_lock(now);

        let refusal = fixture
            .session
            .plan(action(
                json!({"verb":"get_app_state","app":"com.figma.desktop"}),
            ))
            .unwrap_err();
        assert_eq!(refusal.code, CODE_SESSION_PAUSED);

        fixture
            .session
            .lock_mut()
            .on_unlock(now + chrono::Duration::minutes(1));

        let refusal = fixture
            .session
            .plan(action(
                json!({"verb":"click","app":"com.figma.desktop","element_index":1}),
            ))
            .unwrap_err();
        assert_eq!(refusal.code, CODE_NEEDS_FULL_READ);

        // Only a non-diffed read is admitted, and it clears the hold.
        let full =
            action(json!({"verb":"get_app_state","app":"com.figma.desktop","disable_diff":true}));
        let plan = fixture.session.plan(full).unwrap();
        assert!(plan.is_full_read);
        fixture.session.observe_read(
            "com.figma.desktop",
            AppSnapshot {
                labels: [(1u64, "Zoom In".to_owned())].into_iter().collect(),
                element_count: 1,
            },
            true,
        );
        fixture
            .session
            .plan(action(
                json!({"verb":"click","app":"com.figma.desktop","element_index":1}),
            ))
            .unwrap();
    }

    #[test]
    fn a_session_past_its_pause_ceiling_reports_ended_not_paused() {
        let mut fixture = fixture();
        let now = chrono::Utc::now();
        fixture.session.lock_mut().on_lock(now);
        fixture
            .session
            .lock_mut()
            .tick(now + chrono::Duration::hours(4));
        let refusal = fixture
            .session
            .plan(action(json!({"verb":"list_apps"})))
            .unwrap_err();
        assert_eq!(refusal.code, CODE_SESSION_ENDED);
    }

    #[test]
    fn listing_apps_needs_no_app_grant() {
        let fixture = fixture();
        let plan = fixture
            .session
            .plan(action(json!({"verb":"list_apps"})))
            .unwrap();
        assert!(plan.approval.is_none());
    }

    #[test]
    fn coordinate_use_is_remembered_for_the_whole_run() {
        let mut fixture = fixture();
        assert!(fixture.session.element_indexed_only());
        fixture.session.observe_performed(&action(
            json!({"verb":"click","app":"com.figma.desktop","x":1.0,"y":2.0}),
        ));
        assert!(!fixture.session.element_indexed_only());
    }

    #[test]
    fn a_structurally_invalid_action_is_refused_before_any_policy_runs() {
        let fixture = fixture();
        let refusal = fixture
            .session
            .plan(action(json!({"verb":"click","app":"com.apple.Terminal"})))
            .unwrap_err();
        // Not app_denied: the request never made sense in the first place.
        assert_eq!(refusal.code, CODE_INVALID_ACTION);
    }
}
