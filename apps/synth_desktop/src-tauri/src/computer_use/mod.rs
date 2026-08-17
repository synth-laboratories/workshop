//! Computer Use — driving native macOS apps under explicit, per-app consent.
//!
//! The contract is `docs/COMPUTER_USE.md`. Three things about the shape of this
//! module are load-bearing and easy to undo by accident:
//!
//! * **Desktop never touches a TCC API or synthesizes an event.** Those live in
//!   the signed helper, because macOS binds grants to the code identity that
//!   asks. Desktop decides *whether*; the helper does.
//! * **Policy is separate from consent.** [`policy`] answers "may this app be
//!   driven at all", which no approval overrides. [`allowlist`] answers "has
//!   the operator said yes to this app", which is exactly what approval sets.
//! * **The lock guard refuses rather than queues.** A queued keystroke is a
//!   keystroke delivered to the login window a moment later.

pub mod allowlist;
pub mod helper;
pub mod lock;
pub mod permissions;
pub mod policy;
pub mod trajectory;
pub mod vocabulary;
