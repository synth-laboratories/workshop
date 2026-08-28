//! Session noun: Codex + Intern transports.
pub(crate) mod approval;
pub(crate) mod approval_policy;
pub mod codex;
mod persistence;
/// The gate on persisting agent- or user-written visual template code.
pub(crate) mod template_persist;

pub use persistence::SessionPersistence;
