//! Pause and resume across a screen lock. `docs/COMPUTER_USE.md` §7, gate G11.
//!
//! We deliberately do not build lock-screen authentication. The reference suite
//! does — `LockScreenGuardianCoordinator`, an XPC broker, a socket under
//! `/tmp` — and an agent that can get through a lock screen is a
//! credential-bypass primitive. Refusing it is a feature, so the only correct
//! behavior while locked is to stop.
//!
//! The state machine is pure and takes `now` as an argument, so the ceiling and
//! the resume path are testable without locking anyone's screen.

use anyhow::{bail, Result};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// A machine locked overnight must not wake up and resume against a world that
/// has moved on. Thirty minutes is a starting proposal, not a measured number —
/// see `docs/COMPUTER_USE.md` §10 — which is why it is configurable.
pub const DEFAULT_PAUSE_CEILING: Duration = Duration::from_secs(30 * 60);

pub const REASON_LOCKED_TOO_LONG: &str = "locked_too_long";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LockState {
    Unlocked,
    Paused { since: DateTime<Utc> },
    /// The session is over. Terminal states do not resume on unlock.
    Terminal { reason: String },
}

/// What the rest of the system must do about a transition. Returned rather than
/// performed so the caller owns event emission and approval expiry, and so the
/// machine stays testable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LockEffect {
    /// Stop delivering events now, mid-sequence if necessary, and record it.
    Paused { at: DateTime<Utc> },
    /// Hold pending approvals open. A coffee break must not silently become a
    /// failed run by expiring the card against `PLUGIN_APPROVAL_TIMEOUT`.
    SuspendApprovalExpiry,
    /// The screen came back. Every cached index, screenshot id, and coordinate
    /// is now suspect: displays may have been disconnected, windows moved,
    /// dialogs raised.
    Resumed {
        at: DateTime<Utc>,
        paused_for: ChronoDuration,
    },
    ResumeApprovalExpiry,
    /// End the session and expire its approvals with this reason.
    Terminalized { reason: String },
}

/// Whether an action may proceed right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Admission {
    Allow,
    /// Allowed only after a full, non-diffed read re-derives the tree.
    RequireFullRead,
    Refuse { reason: String },
}

pub struct LockGuard {
    state: LockState,
    ceiling: Duration,
    /// Set on resume, cleared by a full non-diffed read. Survives across
    /// actions, so an agent cannot step over it by ignoring one refusal.
    awaiting_full_read: bool,
}

impl LockGuard {
    pub fn new(ceiling: Duration) -> Self {
        Self {
            state: LockState::Unlocked,
            ceiling,
            awaiting_full_read: false,
        }
    }

    pub fn with_default_ceiling() -> Self {
        Self::new(DEFAULT_PAUSE_CEILING)
    }

    pub fn state(&self) -> &LockState {
        &self.state
    }

    pub fn is_paused(&self) -> bool {
        matches!(self.state, LockState::Paused { .. })
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self.state, LockState::Terminal { .. })
    }

    pub fn awaiting_full_read(&self) -> bool {
        self.awaiting_full_read
    }

    /// `com.apple.screenIsLocked`, corroborated by `CGSSessionScreenIsLocked`.
    /// Idempotent: a duplicate notification must not restart the clock, or a
    /// noisy source would hold a session open past its ceiling forever.
    pub fn on_lock(&mut self, now: DateTime<Utc>) -> Vec<LockEffect> {
        match self.state {
            LockState::Unlocked => {
                self.state = LockState::Paused { since: now };
                vec![
                    LockEffect::Paused { at: now },
                    LockEffect::SuspendApprovalExpiry,
                ]
            }
            _ => Vec::new(),
        }
    }

    /// Unlock. If the ceiling was already blown while locked, the session
    /// terminalizes instead of resuming — checked here as well as in `tick`,
    /// because a sleeping machine may deliver no ticks at all.
    pub fn on_unlock(&mut self, now: DateTime<Utc>) -> Vec<LockEffect> {
        let LockState::Paused { since } = self.state else {
            return Vec::new();
        };
        if self.exceeded(since, now) {
            return self.terminalize(REASON_LOCKED_TOO_LONG.to_owned());
        }
        self.state = LockState::Unlocked;
        self.awaiting_full_read = true;
        vec![
            LockEffect::Resumed {
                at: now,
                paused_for: now - since,
            },
            LockEffect::ResumeApprovalExpiry,
        ]
    }

    /// Periodic check while paused. Terminalizes once the ceiling passes, so a
    /// session does not sit open indefinitely waiting for an unlock that is not
    /// coming.
    pub fn tick(&mut self, now: DateTime<Utc>) -> Vec<LockEffect> {
        match self.state {
            LockState::Paused { since } if self.exceeded(since, now) => {
                self.terminalize(REASON_LOCKED_TOO_LONG.to_owned())
            }
            _ => Vec::new(),
        }
    }

    /// May this action run? `is_full_read` marks a non-diffed `get_app_state`,
    /// the one thing allowed to clear the post-unlock hold.
    pub fn admit(&self, is_full_read: bool) -> Admission {
        match &self.state {
            LockState::Terminal { reason } => Admission::Refuse {
                reason: format!("this computer-use session ended: {reason}"),
            },
            // Refused, never queued. A queued keystroke is a keystroke that
            // arrives at the login window a moment later.
            LockState::Paused { .. } => Admission::Refuse {
                reason: "the screen is locked; this session is paused and will resume on unlock"
                    .to_owned(),
            },
            LockState::Unlocked if self.awaiting_full_read && !is_full_read => {
                Admission::RequireFullRead
            }
            LockState::Unlocked => Admission::Allow,
        }
    }

    /// Record that a full, non-diffed read happened. Clears the post-unlock
    /// hold; every element index the agent holds is re-derived from that tree.
    pub fn observed_full_read(&mut self) -> Result<()> {
        if self.is_paused() {
            bail!("cannot read while the screen is locked");
        }
        self.awaiting_full_read = false;
        Ok(())
    }

    fn exceeded(&self, since: DateTime<Utc>, now: DateTime<Utc>) -> bool {
        match ChronoDuration::from_std(self.ceiling) {
            Ok(ceiling) => now - since >= ceiling,
            // An unrepresentable ceiling must not silently mean "never expire".
            Err(_) => true,
        }
    }

    fn terminalize(&mut self, reason: String) -> Vec<LockEffect> {
        self.state = LockState::Terminal {
            reason: reason.clone(),
        };
        self.awaiting_full_read = false;
        vec![LockEffect::Terminalized { reason }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(minute: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_770_000_000 + minute * 60, 0).unwrap()
    }

    #[test]
    fn locking_pauses_and_holds_approvals_open() {
        let mut guard = LockGuard::with_default_ceiling();
        let effects = guard.on_lock(at(0));
        assert_eq!(
            effects,
            vec![
                LockEffect::Paused { at: at(0) },
                LockEffect::SuspendApprovalExpiry
            ]
        );
        assert!(guard.is_paused());
    }

    /// A synthesized keystroke while locked can reach the login window, so
    /// there is no "queue it for later" — only refusal.
    #[test]
    fn nothing_is_admitted_while_locked_including_reads() {
        let mut guard = LockGuard::with_default_ceiling();
        guard.on_lock(at(0));
        for full_read in [true, false] {
            assert!(matches!(
                guard.admit(full_read),
                Admission::Refuse { .. }
            ));
        }
        assert!(guard.observed_full_read().is_err());
    }

    #[test]
    fn a_repeated_lock_notification_does_not_restart_the_clock() {
        let mut guard = LockGuard::new(Duration::from_secs(600));
        guard.on_lock(at(0));
        assert!(guard.on_lock(at(5)).is_empty());
        // Still measured from minute 0, so the ceiling lands at minute 10.
        assert_eq!(
            guard.tick(at(10)),
            vec![LockEffect::Terminalized {
                reason: REASON_LOCKED_TOO_LONG.into()
            }]
        );
    }

    #[test]
    fn unlocking_resumes_but_forces_a_full_read_first() {
        let mut guard = LockGuard::with_default_ceiling();
        guard.on_lock(at(0));
        let effects = guard.on_unlock(at(5));
        assert_eq!(
            effects,
            vec![
                LockEffect::Resumed {
                    at: at(5),
                    paused_for: ChronoDuration::minutes(5)
                },
                LockEffect::ResumeApprovalExpiry
            ]
        );
        // Every cached index is suspect until the tree is re-read in full.
        assert_eq!(guard.admit(false), Admission::RequireFullRead);
        assert_eq!(guard.admit(true), Admission::Allow);
        guard.observed_full_read().unwrap();
        assert_eq!(guard.admit(false), Admission::Allow);
    }

    /// Ignoring one refusal must not clear the hold. The agent has to actually
    /// perform the full read.
    #[test]
    fn the_post_unlock_hold_survives_repeated_attempts() {
        let mut guard = LockGuard::with_default_ceiling();
        guard.on_lock(at(0));
        guard.on_unlock(at(1));
        for _ in 0..5 {
            assert_eq!(guard.admit(false), Admission::RequireFullRead);
        }
        assert!(guard.awaiting_full_read());
    }

    /// A sleeping machine delivers no ticks, so the ceiling has to be enforced
    /// on the unlock path too or an overnight lock resumes as if nothing
    /// happened.
    #[test]
    fn a_lock_past_the_ceiling_terminalizes_even_with_no_ticks() {
        let mut guard = LockGuard::new(Duration::from_secs(30 * 60));
        guard.on_lock(at(0));
        let effects = guard.on_unlock(at(600));
        assert_eq!(
            effects,
            vec![LockEffect::Terminalized {
                reason: REASON_LOCKED_TOO_LONG.into()
            }]
        );
        assert!(guard.is_terminal());
        assert!(matches!(guard.admit(true), Admission::Refuse { .. }));
    }

    #[test]
    fn a_terminal_session_does_not_come_back_on_a_later_unlock() {
        let mut guard = LockGuard::new(Duration::from_secs(60));
        guard.on_lock(at(0));
        guard.tick(at(10));
        assert!(guard.is_terminal());
        assert!(guard.on_unlock(at(11)).is_empty());
        assert!(guard.is_terminal());
    }

    #[test]
    fn unlocking_without_a_lock_is_a_no_op() {
        let mut guard = LockGuard::with_default_ceiling();
        assert!(guard.on_unlock(at(1)).is_empty());
        assert_eq!(guard.admit(false), Admission::Allow);
    }
}
