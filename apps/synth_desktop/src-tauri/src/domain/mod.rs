//! Shared durable domain services used by local and cloud agent transports.

mod runtime_target;
mod session_run;

pub use runtime_target::{InternBinding, InternMode, RuntimeTarget, LOCAL_LAGUNA_MODEL};
pub use session_run::{
    CommandReceiptInput, DomainMutation, RunCreate, RunService, RunStatus, SessionCreate,
    SessionService, SessionStatus, SessionTitleOrigin,
};
