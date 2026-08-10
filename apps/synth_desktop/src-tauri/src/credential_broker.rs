//! Native custody for the Synth Cloud credential.
//!
//! Codex records the environment it was launched with — `CODEX_HOME/shell_snapshots`
//! is a plain `.sh` file containing `export NAME=value` for the inherited
//! environment. Handing the child `SYNTH_API_KEY=<real key>` therefore wrote the
//! user's long-lived Synth credential to disk in cleartext, outside anything the
//! renderer isolation protects.
//!
//! This module keeps the real key inside the Desktop host. It runs a loopback
//! reverse proxy that Codex talks to as an ordinary Responses provider; the proxy
//! swaps the caller's short-lived lease token for the real `Authorization` header
//! and forwards to the configured Synth backend. The child process therefore
//! never sees, and never serializes, the Synth key.
//!
//! The lease token that *does* reach the child is deliberately not a credential
//! substitute: it is minted per spawned child, is only accepted from loopback by
//! this process, only reaches one pre-bound upstream origin, and is revoked when
//! the session closes or the app exits.

use std::collections::HashMap;
use std::fmt;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

use anyhow::{Context, Result};
use bytes::Bytes;
use futures_util::StreamExt;
use http_body_util::{combinators::BoxBody, BodyExt, Full, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use uuid::Uuid;

/// The environment variable Codex reads for the loopback provider. It is
/// intentionally *not* `SYNTH_API_KEY`: anything under that name is a real
/// credential and must never be handed to a child process.
pub const LEASE_ENV_KEY: &str = "SYNTH_DESKTOP_PROVIDER_LEASE";

/// Provider environment variables that have ever carried a real secret into a
/// Desktop-managed Codex home. Existing snapshots are scrubbed of these.
pub const REDACTED_ENV_KEYS: &[&str] = &[
    "SYNTH_API_KEY",
    "OPENROUTER_API_KEY",
    "OPENAI_API_KEY",
    "SYNTH_LAGUNA_API_KEY",
    LEASE_ENV_KEY,
];

const REDACTION: &str = "<redacted-by-synth-desktop>";
/// Cloud turns are long; the proxy must outlive a full streamed response.
const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(900);

/// A live authorization mapping: lease token -> real upstream credential.
struct Lease {
    /// Origin the lease may reach, e.g. `http://127.0.0.1:41209`. Requests are
    /// forwarded to this origin and nowhere else.
    upstream_origin: String,
    api_key: String,
}

impl fmt::Debug for Lease {
    /// Never let a panic message, log line, or `{:?}` of an error reproduce the
    /// credential this type exists to contain.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lease")
            .field("upstream_origin", &self.upstream_origin)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

/// What the caller needs in order to point Codex at the loopback provider.
#[derive(Clone, PartialEq, Eq)]
pub struct LeaseHandle {
    /// Loopback origin the child should call, e.g. `http://127.0.0.1:53123`.
    pub origin: String,
    /// Short-lived bearer token the child presents to the proxy.
    pub token: String,
}

impl fmt::Debug for LeaseHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LeaseHandle")
            .field("origin", &self.origin)
            .field("token", &"<redacted>")
            .finish()
    }
}

struct BrokerState {
    /// session id -> lease token. Keying by session means a restart replaces
    /// rather than accumulates tokens, and closing a session revokes exactly one.
    by_session: RwLock<HashMap<String, String>>,
    /// lease token -> lease.
    leases: RwLock<HashMap<String, Arc<Lease>>>,
    http: reqwest::Client,
}

impl BrokerState {
    fn lookup(&self, token: &str) -> Option<Arc<Lease>> {
        self.leases.read().unwrap().get(token).cloned()
    }
}

/// A loopback reverse proxy that holds provider credentials on behalf of child
/// processes.
pub struct CredentialBroker {
    state: Arc<BrokerState>,
    addr: SocketAddr,
}

impl fmt::Debug for CredentialBroker {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CredentialBroker")
            .field("addr", &self.addr)
            .field(
                "leases",
                &self.state.leases.read().map(|map| map.len()).unwrap_or(0),
            )
            .finish()
    }
}

impl CredentialBroker {
    /// Bind the loopback listener without serving it. Splitting bind from serve
    /// gives callers the port synchronously and lets unit tests exercise leasing
    /// without a live accept loop.
    pub fn bind() -> Result<(Self, std::net::TcpListener)> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .context("bind the Synth Desktop credential proxy to loopback")?;
        let addr = listener.local_addr().context("read the proxy address")?;
        let state = Arc::new(BrokerState {
            by_session: RwLock::new(HashMap::new()),
            leases: RwLock::new(HashMap::new()),
            http: reqwest::Client::builder()
                .timeout(UPSTREAM_TIMEOUT)
                .build()
                .context("build the credential proxy HTTP client")?,
        });
        Ok((Self { state, addr }, listener))
    }

    /// Bind and start serving on the shared async runtime.
    pub fn start() -> Result<Self> {
        let (broker, listener) = Self::bind()?;
        listener
            .set_nonblocking(true)
            .context("make the credential proxy listener non-blocking")?;
        let state = broker.state.clone();
        tauri::async_runtime::spawn(async move {
            // Adopt the listener inside the task: a tokio listener registers with
            // the reactor of the runtime it is created on, so converting it here
            // keeps the socket and its driver on the same runtime.
            let served = match tokio::net::TcpListener::from_std(listener) {
                Ok(listener) => serve(state, listener).await,
                Err(error) => {
                    Err(anyhow::Error::from(error).context("adopt the credential proxy listener"))
                }
            };
            if let Err(error) = served {
                // Nothing can await this task, so the failure is reported here
                // rather than swallowed. Cloud sessions fail closed after it.
                eprintln!("synth-desktop: credential proxy stopped serving: {error:#}");
            }
        });
        Ok(broker)
    }

    pub fn origin(&self) -> String {
        format!("http://127.0.0.1:{}", self.addr.port())
    }

    /// Mint the lease a child process is about to be spawned with, replacing
    /// and invalidating any token the session held before. The returned token
    /// is what the child may see; `api_key` stays here.
    ///
    /// Contract: called exactly once per spawned child, after the previous
    /// child (if any) was closed and its lease revoked. Minting while a child
    /// is live strands that child with a dead token mid-conversation, so a
    /// caller that reuses a live child must not lease again —
    /// `CodexManager::start` leases only on its spawn path.
    pub fn lease(&self, session_id: &str, upstream_origin: &str, api_key: &str) -> LeaseHandle {
        let upstream_origin = upstream_origin.trim_end_matches('/');
        let token = format!(
            "sdl_{}{}",
            Uuid::new_v4().simple(),
            Uuid::new_v4().simple()
        );
        let lease = Arc::new(Lease {
            upstream_origin: upstream_origin.to_owned(),
            api_key: api_key.to_owned(),
        });
        let previous = self
            .state
            .by_session
            .write()
            .unwrap()
            .insert(session_id.to_owned(), token.clone());
        let mut leases = self.state.leases.write().unwrap();
        if let Some(previous) = previous {
            leases.remove(&previous);
        }
        leases.insert(token.clone(), lease);
        LeaseHandle {
            origin: self.origin(),
            token,
        }
    }

    /// Drop the session's lease. After this the token is inert.
    pub fn revoke(&self, session_id: &str) {
        let token = self.state.by_session.write().unwrap().remove(session_id);
        if let Some(token) = token {
            self.state.leases.write().unwrap().remove(&token);
        }
    }

    /// The origin a lease is bound to, for tests that assert routing.
    #[cfg(test)]
    pub(crate) fn upstream_for(&self, token: &str) -> Option<String> {
        self.state
            .lookup(token)
            .map(|lease| lease.upstream_origin.clone())
    }

    /// Whether a token still maps to a live lease, for tests that assert the
    /// lease lifecycle.
    #[cfg(test)]
    pub(crate) fn resolves(&self, token: &str) -> bool {
        self.state.lookup(token).is_some()
    }

    /// The session's current token, for tests that assert lease lifecycle.
    #[cfg(test)]
    pub(crate) fn token_for(&self, session_id: &str) -> Option<String> {
        self.state.by_session.read().unwrap().get(session_id).cloned()
    }
}

/// One proxy per process. Every cloud session leases from the same broker, and
/// session teardown revokes through it, so it outlives any single manager.
static SHARED: OnceLock<Mutex<Option<Arc<CredentialBroker>>>> = OnceLock::new();

/// The process-wide broker, started on first use.
pub fn shared() -> Result<Arc<CredentialBroker>> {
    let slot = SHARED.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().unwrap();
    if let Some(existing) = guard.as_ref() {
        return Ok(existing.clone());
    }
    let started = Arc::new(CredentialBroker::start()?);
    *guard = Some(started.clone());
    Ok(started)
}

/// Drop a session's lease if the broker was ever started. Safe to call for
/// sessions that never held one.
pub fn revoke_shared(session_id: &str) {
    if let Some(broker) = SHARED.get().and_then(|slot| slot.lock().unwrap().clone()) {
        broker.revoke(session_id);
    }
}

async fn serve(state: Arc<BrokerState>, listener: tokio::net::TcpListener) -> Result<()> {
    loop {
        // Retrying a failed accept forever turns a dead listener into a silent
        // hot loop. Ending here makes every later lease fail visibly instead.
        let (stream, _peer) = listener
            .accept()
            .await
            .context("accept a connection on the Synth Desktop credential proxy")?;
        let state = state.clone();
        tauri::async_runtime::spawn(async move {
            let io = TokioIo::new(stream);
            let service = service_fn(move |request| proxy(state.clone(), request));
            // Reported rather than discarded: a connection that ends in an
            // error took an agent's turn with it, and the reason is not
            // recoverable from anywhere else.
            if let Err(error) = http1::Builder::new().serve_connection(io, service).await {
                eprintln!("synth-desktop: credential proxy connection ended: {error}");
            }
        });
    }
}

type ProxyBody = BoxBody<Bytes, std::io::Error>;

fn json_error(status: StatusCode, message: &str) -> Response<ProxyBody> {
    let body = serde_json::json!({ "error": { "message": message } }).to_string();
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(
            Full::new(Bytes::from(body))
                .map_err(|never| match never {})
                .boxed(),
        )
        .expect("static proxy error response")
}

/// Headers that describe *this* hop and must not be replayed to the other side.
fn is_hop_by_hop(name: &str) -> bool {
    matches!(
        name,
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "host"
            | "content-length"
            | "authorization"
    )
}

fn bearer(request: &Request<Incoming>) -> Option<&str> {
    request
        .headers()
        .get(hyper::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
}

async fn proxy(
    state: Arc<BrokerState>,
    request: Request<Incoming>,
) -> Result<Response<ProxyBody>, std::convert::Infallible> {
    let Some(lease) = bearer(&request).and_then(|token| state.lookup(token)) else {
        return Ok(json_error(
            StatusCode::UNAUTHORIZED,
            "This Synth Desktop provider lease is not valid.",
        ));
    };
    let (parts, body) = request.into_parts();
    let path = parts
        .uri
        .path_and_query()
        .map(|value| value.as_str())
        .unwrap_or("/");
    let url = format!("{}{path}", lease.upstream_origin);

    let Ok(method) = reqwest::Method::from_bytes(parts.method.as_str().as_bytes()) else {
        return Ok(json_error(
            StatusCode::BAD_REQUEST,
            "Unsupported request method.",
        ));
    };
    let Ok(collected) = body.collect().await else {
        return Ok(json_error(
            StatusCode::BAD_GATEWAY,
            "Synth Desktop could not read the request body.",
        ));
    };

    let mut outbound = state.http.request(method, url).body(collected.to_bytes());
    for (name, value) in parts.headers.iter() {
        if is_hop_by_hop(name.as_str()) {
            continue;
        }
        let (Ok(name), Ok(value)) = (
            reqwest::header::HeaderName::from_bytes(name.as_str().as_bytes()),
            reqwest::header::HeaderValue::from_bytes(value.as_bytes()),
        ) else {
            // Silently dropping it would forward a request the caller never
            // wrote; refuse the whole request instead.
            return Ok(json_error(
                StatusCode::BAD_REQUEST,
                "Synth Desktop could not forward a header on this request.",
            ));
        };
        outbound = outbound.header(name, value);
    }
    // The one header the child could not supply, because it never had the key.
    outbound = outbound.bearer_auth(&lease.api_key);

    let upstream = match outbound.send().await {
        Ok(response) => response,
        // Deliberately not `error.to_string()`: transport errors are allowed to
        // carry request detail, and this response is rendered by the agent.
        Err(_) => {
            return Ok(json_error(
                StatusCode::BAD_GATEWAY,
                "Synth Cloud is unavailable right now.",
            ))
        }
    };

    let mut response = Response::builder().status(upstream.status().as_u16());
    for (name, value) in upstream.headers().iter() {
        if is_hop_by_hop(name.as_str()) {
            continue;
        }
        response = response.header(name.as_str(), value.as_bytes());
    }
    // Stream rather than buffer: a governed Responses call is server-sent events
    // and must reach the agent token by token.
    let stream = upstream.bytes_stream().map(|chunk| {
        chunk
            .map(Frame::data)
            .map_err(|_| std::io::Error::other("Synth Cloud ended the response early."))
    });
    Ok(response
        .body(BodyExt::boxed(StreamBody::new(stream)))
        .unwrap_or_else(|_| {
            json_error(
                StatusCode::BAD_GATEWAY,
                "Synth Cloud sent a response Synth Desktop could not relay.",
            )
        }))
}

/// Scrub provider secrets that earlier builds wrote into Desktop's own Codex
/// homes, and tighten the permissions of what is left.
///
/// Scope is deliberately narrow: only `<codex_root>/homes/*/shell_snapshots`,
/// which Desktop creates and owns. User shell files elsewhere are never read or
/// rewritten.
pub fn redact_managed_shell_snapshots(codex_root: &Path) -> Result<usize> {
    let homes = codex_root.join("homes");
    // No managed homes is a real answer: nothing was ever written to scrub.
    // Any other read failure is not, because it means there may be snapshots
    // holding a credential that this pass silently never looked at.
    let entries = match std::fs::read_dir(&homes) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error).with_context(|| format!("read {}", homes.display())),
    };
    let mut redacted = 0usize;
    for entry in entries {
        let entry = entry.with_context(|| format!("enumerate {}", homes.display()))?;
        let snapshots = entry.path().join("shell_snapshots");
        let files = match std::fs::read_dir(&snapshots) {
            Ok(files) => files,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error).with_context(|| format!("read {}", snapshots.display()))
            }
        };
        for file in files {
            let file = file.with_context(|| format!("enumerate {}", snapshots.display()))?;
            let path = file.path();
            if !path.is_file() {
                continue;
            }
            // A snapshot that cannot be read is a snapshot that cannot be
            // proven free of the credential, so it is an error rather than a
            // file to pass over.
            let contents = std::fs::read_to_string(&path)
                .with_context(|| format!("read {}", path.display()))?;
            let scrubbed = redact_env_assignments(&contents);
            if scrubbed != contents {
                std::fs::write(&path, &scrubbed)
                    .with_context(|| format!("rewrite {}", path.display()))?;
                redacted += 1;
            }
            restrict_to_owner(&path)?;
        }
    }
    Ok(redacted)
}

/// Replace the value of every known provider variable, whatever quoting the
/// snapshot used, leaving the rest of the line shape intact.
fn redact_env_assignments(contents: &str) -> String {
    let mut out = String::with_capacity(contents.len());
    for (index, line) in contents.lines().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        match assignment_prefix(line) {
            Some(prefix) => {
                out.push_str(&line[..prefix]);
                out.push('\'');
                out.push_str(REDACTION);
                out.push('\'');
            }
            None => out.push_str(line),
        }
    }
    if contents.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Byte offset just past `NAME=` when the line assigns a known provider secret.
fn assignment_prefix(line: &str) -> Option<usize> {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    let rest = trimmed.strip_prefix("export ").map(str::trim_start);
    let (offset, body) = match rest {
        Some(body) => (indent + (trimmed.len() - body.len()), body),
        None => (indent, trimmed),
    };
    let equals = body.find('=')?;
    let name = &body[..equals];
    REDACTED_ENV_KEYS
        .contains(&name)
        .then_some(offset + equals + 1)
}

/// Files Desktop writes that may hold, or have held, a secret are owner-only.
pub fn restrict_to_owner(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path)
            .with_context(|| format!("stat {}", path.display()))?
            .permissions()
            .mode();
        if mode & 0o777 != 0o600 {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .with_context(|| format!("restrict {}", path.display()))?;
        }
    }
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    const SENTINEL: &str = "sk_dev_SENTINEL_CREDENTIAL_MUST_NOT_ESCAPE";

    /// Minimal upstream that records the `Authorization` it was given.
    fn spawn_upstream(body: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let seen: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut buf = [0u8; 16384];
                let read = stream.read(&mut buf).unwrap_or(0);
                sink.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buf[..read]).into_owned());
                let payload = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(payload.as_bytes());
            }
        });
        (origin, seen)
    }

    #[test]
    fn a_lease_hands_back_a_token_that_is_not_the_credential() {
        let (broker, _listener) = CredentialBroker::bind().unwrap();
        let handle = broker.lease("session-a", "http://127.0.0.1:41209", SENTINEL);
        assert_ne!(handle.token, SENTINEL);
        assert!(!handle.token.contains(SENTINEL));
        assert!(handle.origin.starts_with("http://127.0.0.1:"));
        assert!(broker.resolves(&handle.token));
    }

    #[test]
    fn re_leasing_a_session_replaces_and_invalidates_its_previous_token() {
        let (broker, _listener) = CredentialBroker::bind().unwrap();
        let first = broker.lease("session-a", "http://127.0.0.1:41209", SENTINEL);
        let second = broker.lease("session-a", "http://127.0.0.1:41209", SENTINEL);
        assert_ne!(first.token, second.token);
        assert!(!broker.resolves(&first.token));
        assert!(broker.resolves(&second.token));
        assert_eq!(broker.token_for("session-a"), Some(second.token));
    }

    #[test]
    fn revoking_a_session_makes_its_token_inert() {
        let (broker, _listener) = CredentialBroker::bind().unwrap();
        let handle = broker.lease("session-a", "http://127.0.0.1:41209", SENTINEL);
        broker.revoke("session-a");
        assert!(!broker.resolves(&handle.token));
    }

    #[test]
    fn debug_formatting_never_reproduces_the_credential() {
        let (broker, _listener) = CredentialBroker::bind().unwrap();
        let handle = broker.lease("session-a", "http://127.0.0.1:41209", SENTINEL);
        let lease = broker.state.lookup(&handle.token).unwrap();
        for rendered in [
            format!("{broker:?}"),
            format!("{handle:?}"),
            format!("{lease:?}"),
        ] {
            assert!(
                !rendered.contains(SENTINEL),
                "debug output leaked the credential: {rendered}"
            );
        }
    }

    #[tokio::test]
    async fn the_proxy_swaps_a_lease_token_for_the_real_credential() {
        let (upstream, seen) = spawn_upstream(r#"{"id":"resp_1","status":"completed"}"#);
        let broker = CredentialBroker::start().unwrap();
        let handle = broker.lease("session-a", &upstream, SENTINEL);

        let response = reqwest::Client::new()
            .post(format!("{}/api/v1/responses", handle.origin))
            .bearer_auth(&handle.token)
            .json(&serde_json::json!({"model": "laguna-s", "input": "hi"}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert!(response.text().await.unwrap().contains("resp_1"));

        let request = seen.lock().unwrap().first().cloned().unwrap();
        assert!(
            request.to_lowercase().contains(&format!(
                "authorization: bearer {}",
                SENTINEL.to_lowercase()
            )),
            "upstream did not receive the real credential: {request}"
        );
        assert!(
            !request.contains(&handle.token),
            "the lease token must not be forwarded upstream"
        );
        assert!(request.starts_with("POST /api/v1/responses "));
    }

    #[tokio::test]
    async fn the_proxy_refuses_a_token_it_never_issued() {
        let broker = CredentialBroker::start().unwrap();
        let response = reqwest::Client::new()
            .post(format!("{}/api/v1/responses", broker.origin()))
            .bearer_auth("sdl_not_a_real_lease")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 401);
        assert!(response.text().await.unwrap().contains("lease is not valid"));
    }

    #[tokio::test]
    async fn a_revoked_lease_stops_reaching_the_backend() {
        let (upstream, seen) = spawn_upstream(r#"{"ok":true}"#);
        let broker = CredentialBroker::start().unwrap();
        let handle = broker.lease("session-a", &upstream, SENTINEL);
        broker.revoke("session-a");

        let response = reqwest::Client::new()
            .get(format!("{}/api/v1/responses", handle.origin))
            .bearer_auth(&handle.token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 401);
        assert!(seen.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_unreachable_backend_is_reported_without_transport_detail() {
        let dead = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", dead.local_addr().unwrap());
        drop(dead);
        let broker = CredentialBroker::start().unwrap();
        let handle = broker.lease("session-a", &origin, SENTINEL);
        let response = reqwest::Client::new()
            .post(format!("{}/api/v1/responses", handle.origin))
            .bearer_auth(&handle.token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 502);
        let body = response.text().await.unwrap();
        assert_eq!(
            body,
            r#"{"error":{"message":"Synth Cloud is unavailable right now."}}"#
        );
        assert!(!body.contains(SENTINEL));
    }

    #[test]
    fn redaction_rewrites_known_provider_variables_in_any_quoting() {
        let snapshot = concat!(
            "#!/bin/sh\n",
            "export PATH='/usr/bin'\n",
            "export SYNTH_API_KEY=sk_dev_SENTINEL_CREDENTIAL_MUST_NOT_ESCAPE\n",
            "export OPENROUTER_API_KEY='sk_or_secret'\n",
            "  export OPENAI_API_KEY=\"sk_openai_secret\"\n",
            "SYNTH_LAGUNA_API_KEY=loopback-token\n",
            "export SYNTH_API_KEY_FINGERPRINT=abcd\n",
        );
        let redacted = redact_env_assignments(snapshot);
        for secret in [
            SENTINEL,
            "sk_or_secret",
            "sk_openai_secret",
            "loopback-token",
        ] {
            assert!(!redacted.contains(secret), "{secret} survived redaction");
        }
        // Unrelated variables, indentation and a lookalike name are untouched.
        assert!(redacted.contains("export PATH='/usr/bin'"));
        assert!(redacted.contains("  export OPENAI_API_KEY="));
        assert!(redacted.contains("export SYNTH_API_KEY_FINGERPRINT=abcd"));
        assert_eq!(redacted.lines().count(), snapshot.lines().count());
        assert!(redacted.ends_with('\n'));
    }

    #[test]
    fn redaction_walks_only_desktop_owned_codex_homes() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("codex");
        let snapshots = root.join("homes/abc123/shell_snapshots");
        std::fs::create_dir_all(&snapshots).unwrap();
        let snapshot = snapshots.join("snap.sh");
        std::fs::write(&snapshot, format!("export SYNTH_API_KEY={SENTINEL}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&snapshot, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
        // A user file outside the managed root must not be considered at all.
        let outside = temp.path().join("home_profile");
        std::fs::create_dir_all(&outside).unwrap();
        let user_file = outside.join(".zshenv");
        std::fs::write(&user_file, format!("export SYNTH_API_KEY={SENTINEL}\n")).unwrap();

        assert_eq!(redact_managed_shell_snapshots(&root).unwrap(), 1);
        assert!(!std::fs::read_to_string(&snapshot).unwrap().contains(SENTINEL));
        assert!(std::fs::read_to_string(&user_file).unwrap().contains(SENTINEL));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&snapshot).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600, "snapshots must be owner-only");
        }
    }

    #[test]
    fn redaction_refuses_a_snapshot_it_cannot_read_rather_than_skipping_it() {
        // Skipping an unreadable file would report success while leaving a
        // credential in cleartext, which is the one outcome this pass exists
        // to prevent.
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("codex");
        let snapshots = root.join("homes/abc123/shell_snapshots");
        std::fs::create_dir_all(&snapshots).unwrap();
        let unreadable = snapshots.join("snap.sh");
        std::fs::write(&unreadable, format!("export SYNTH_API_KEY={SENTINEL}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000)).unwrap();
        }

        let error = redact_managed_shell_snapshots(&root)
            .expect_err("an unreadable snapshot must not be passed over")
            .to_string();
        assert!(error.contains("snap.sh"), "{error}");
        assert!(!error.contains(SENTINEL), "{error}");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
    }

    #[test]
    fn redaction_is_a_no_op_when_there_are_no_managed_homes() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(
            redact_managed_shell_snapshots(&temp.path().join("codex")).unwrap(),
            0
        );
    }

    #[test]
    fn concurrent_sessions_hold_independent_leases() {
        let (broker, _listener) = CredentialBroker::bind().unwrap();
        let a = broker.lease("session-a", "http://127.0.0.1:1", SENTINEL);
        let b = broker.lease("session-b", "http://127.0.0.1:2", SENTINEL);
        broker.revoke("session-a");
        assert!(!broker.resolves(&a.token));
        assert!(broker.resolves(&b.token));
    }
}
