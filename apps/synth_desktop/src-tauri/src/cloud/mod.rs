//! Cloud-backed product services owned by the Rust desktop runtime.

pub mod intern;
pub(crate) mod legacy_authority;

pub(crate) mod storage;

// Native MQ mailbox: grant client, transport, policy and restricted delivery.
// Gated: only instance (eval-driver) builds reach it until a profile opts in.
#[cfg_attr(not(feature = "eval-driver"), allow(dead_code))]
pub(crate) mod mailbox;

// Candidate DTO validation only; no live authority activation.
pub(crate) mod identity;
pub(crate) mod scoped_runtime;
