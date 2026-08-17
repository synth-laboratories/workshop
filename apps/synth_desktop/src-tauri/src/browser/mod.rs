//! Backend-neutral Workshop Browser Protocol.
//!
//! Browser state belongs to an implementation of [`BrowserBackend`], never to
//! an agent transcript. The Playwright sidecar is the v1 reference backend;
//! an embedded CEF implementation must satisfy these same types and invariants.

pub mod protocol;

pub use protocol::*;
