//! Shared durable domain services used by local and cloud agent transports.

mod session_kind;
mod session_run;

pub use session_kind::SessionKind;
pub use session_run::{
    CommandReceiptInput, DomainMutation, RunCreate, RunService, RunStatus, SessionCreate,
    SessionService, SessionStatus, SessionTitleOrigin,
};
