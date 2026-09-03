//! Platform contracts for failure identity, logs, operations, and persistence.
//!
//! See `notes/specifications/workshop/failure_runtime.md`.

pub mod approval;
pub mod failure;
pub mod logging;
pub mod operations;
pub mod persistence;

#[allow(unused_imports)]
pub use failure::{FailureRuntime, OperationalFailure};
#[allow(unused_imports)]
pub use logging::{LogRecord, LogRuntime, ObservabilityMode};
#[allow(unused_imports)]
pub use operations::OperationContext;
