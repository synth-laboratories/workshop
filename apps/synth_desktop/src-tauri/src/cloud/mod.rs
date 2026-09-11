//! Cloud-backed product services owned by the Rust desktop runtime.

pub mod intern;

pub(crate) mod storage;

// Candidate DTO validation only; no live authority activation.
pub(crate) mod identity;
