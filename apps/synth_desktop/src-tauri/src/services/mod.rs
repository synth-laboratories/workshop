//! Managed sidecar / loopback service lifecycle.

mod supervisor;

pub use supervisor::{ManagedService, ServiceSupervisor};
