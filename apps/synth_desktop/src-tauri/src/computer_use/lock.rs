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
    Paused {
        since: DateTime<Utc>,
    },
    /// The session is over. Terminal states do not resume on unlock.
    Terminal {
        reason: String,
    },
}

/// What the rest of the system must do about a transition. Returned rather than
/// performed so the caller owns event emission and approval expiry, and so the
/// machine stays testable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LockEffect {
    /// Stop delivering events now, mid-sequence if necessary, and record it.
    Paused {
        at: DateTime<Utc>,
    },
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
    Terminalized {
        reason: String,
    },
}

/// Whether an action may proceed right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Admission {
    Allow,
    /// Allowed only after a full, non-diffed read re-derives the tree.
    RequireFullRead,
    Refuse {
        reason: String,
    },
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

