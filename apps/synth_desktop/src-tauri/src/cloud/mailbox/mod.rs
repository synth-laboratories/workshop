//! Workshop native MQ mailbox: grant authority client, granted-history and
//! wake transport, participant policy and the restricted delivery path.
//!
//! Contract: manderqueue `docs/WORKSHOP_GRANT_CONTRACT.md` (grant v0.11).
//! Persistence lives in `cloud::storage` (mailbox submodule); host
//! composition and fencing live in `cloud::scoped_runtime::mailbox`.
pub mod codex_executor;
pub mod grant;
pub mod host;
pub mod ipc;
pub mod policy;
pub mod wire;
