//! Shared reqwest client factory.
//!
//! Call sites must not construct a bare reqwest client (silent defaults). Prefer
//! [`http_client`] or [`http_client_with_timeout`].

use crate::limits;
use reqwest::Client;
use std::time::Duration;

/// Process-wide default client with an explicit timeout from [`limits`].
pub fn http_client() -> Client {
    http_client_with_timeout(limits::HTTP_DEFAULT_TIMEOUT)
}

/// Build a client with a named timeout. Panics only if the TLS stack cannot
/// initialize — that is a process-fatal misconfiguration, not a call-site error.
pub fn http_client_with_timeout(timeout: Duration) -> Client {
    Client::builder()
        .timeout(timeout)
        .build()
        .expect("build Synth Desktop HTTP client")
}

/// Builder that already pins the default timeout so callers only layer extras
/// (redirect policy, user-agent, …).
pub fn http_client_builder() -> reqwest::ClientBuilder {
    Client::builder().timeout(limits::HTTP_DEFAULT_TIMEOUT)
}
