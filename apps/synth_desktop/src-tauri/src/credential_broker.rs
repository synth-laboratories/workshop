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
use std::pin::Pin;
use std::sync::{Arc, Mutex, RwLock};
use std::task::{Context as TaskContext, Poll};
use std::time::Duration;

use anyhow::{Context, Result};
use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use http_body_util::{combinators::BoxBody, BodyExt, Full, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::{Request, Response, StatusCode};
use serde_json::Value;
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
const UPSTREAM_TIMEOUT: Duration = crate::limits::CREDENTIAL_UPSTREAM_TIMEOUT;

/// A live authorization mapping: lease token -> real upstream credential.
struct Lease {
    /// Origin the lease may reach, e.g. `http://127.0.0.1:41209`. Requests are
    /// forwarded to this origin and nowhere else.
    upstream_origin: String,
    api_key: String,
    /// The Desktop session this lease was minted for. Responses relayed under
    /// the lease attribute their settled accounting to this session.
    session_id: String,
}

impl fmt::Debug for Lease {
    /// Never let a panic message, log line, or `{:?}` of an error reproduce the
    /// credential this type exists to contain.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lease")
            .field("upstream_origin", &self.upstream_origin)
            .field("api_key", &"<redacted>")
            .field("session_id", &self.session_id)
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
    /// Current turn-attribution scope for each session. Requests capture this
    /// value when they enter the proxy, so a late response can never be charged
    /// to whichever turn happens to finalize next.
    turn_scopes: RwLock<HashMap<String, String>>,
    http: reqwest::Client,
    /// Settled receipts for sessions leasing through this broker. Shared with
    /// the composition root so finalizers can drain without owning the proxy.
    receipts: Arc<ReceiptStore>,
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
    pub fn bind(receipts: Arc<ReceiptStore>) -> Result<(Self, std::net::TcpListener)> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .context("bind the Synth Desktop credential proxy to loopback")?;
        let addr = listener.local_addr().context("read the proxy address")?;
        let state = Arc::new(BrokerState {
            by_session: RwLock::new(HashMap::new()),
            leases: RwLock::new(HashMap::new()),
            turn_scopes: RwLock::new(HashMap::new()),
            http: crate::http::http_client_with_timeout(UPSTREAM_TIMEOUT),
            receipts,
        });
        Ok((Self { state, addr }, listener))
    }

    /// Bind and start serving on the shared async runtime.
    ///
    /// `serve()` itself is unchanged: hyper loopback accept + per-connection
    /// `proxy`. Only the receipt store is injected instead of a process global.
    pub fn start(receipts: Arc<ReceiptStore>) -> Result<Self> {
        let (broker, listener) = Self::bind(receipts)?;
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
                crate::platform::logging::report("credential_broker", "eprintln", format!("synth-desktop: credential proxy stopped serving: {error:#}"));
            }
        });
        Ok(broker)
    }

    /// Receipt store this broker writes into. Finalizers drain the same Arc.
    pub fn receipts(&self) -> Arc<ReceiptStore> {
        self.state.receipts.clone()
    }

    /// Bind subsequent relayed requests for a session to one turn scope.
    pub fn begin_turn(&self, session_id: &str, turn_scope: &str) {
        self.state
            .turn_scopes
            .write()
            .unwrap()
            .insert(session_id.to_owned(), turn_scope.to_owned());
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
        let token = format!("sdl_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        let lease = Arc::new(Lease {
            upstream_origin: upstream_origin.to_owned(),
            api_key: api_key.to_owned(),
            session_id: session_id.to_owned(),
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

    /// Drop the session's lease. After this the token is inert. Settled
    /// receipts the session never drained are dropped with it — a closed
    /// session has no later finalize that could truthfully absorb them.
    pub fn revoke(&self, session_id: &str) {
        let token = self.state.by_session.write().unwrap().remove(session_id);
        if let Some(token) = token {
            self.state.leases.write().unwrap().remove(&token);
        }
        self.state.turn_scopes.write().unwrap().remove(session_id);
        self.state.receipts.discard(session_id);
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
        self.state
            .by_session
            .read()
            .unwrap()
            .get(session_id)
            .cloned()
    }
}

/// Settled provider accounting for one relayed upstream response, extracted
/// from the bytes that streamed through the proxy. Everything here was already
/// visible to the child process except the attribution to a Desktop session.
#[derive(Clone, Debug, PartialEq)]
pub struct SettledReceipt {
    pub session_id: String,
    /// Native-generated scope of the turn whose upstream request produced this
    /// receipt. It is intentionally independent of provider response ids.
    pub turn_scope: Option<String>,
    pub provider_response_id: String,
    pub model: Option<String>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub cached_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    /// The provider's settled charge. `None` when the response reported token
    /// usage without money — real tokens, no invented dollars.
    pub cost_usd: Option<f64>,
    pub completed_at_ms: i64,
}

/// Receipts waiting to be drained by their session's turn finalizer.
///
/// Deliberately separate from the broker instance: the finalizer must be able
/// to drain (and find nothing) without the proxy being the sole owner, and a
/// broker restart must not orphan receipts already captured. Injected as an
/// `Arc` from the composition root and shared with `CredentialBroker`.
#[derive(Default)]
pub struct ReceiptStore {
    inner: Mutex<HashMap<String, Vec<SettledReceipt>>>,
}

impl ReceiptStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Take every settled receipt queued for the session, removing them.
    ///
    /// Contract with the finalizer: draining is the only consumption path, the
    /// caller's `(provider, request_id)` upsert key makes replays idempotent, and
    /// a receipt that arrives *after* a drain (cancellation race) stays queued no
    /// longer than the session's next finalize — `revoke` drops leftovers when
    /// the session closes instead of inventing a row for them.
    pub fn drain(&self, session_id: &str) -> Vec<SettledReceipt> {
        self.inner
            .lock()
            .unwrap()
            .remove(session_id)
            .unwrap_or_default()
    }

    /// Take only receipts captured for one native turn scope, leaving late
    /// receipts from every other turn untouched rather than misattributing
    /// them to the caller.
    pub fn drain_for_turn(&self, session_id: &str, turn_scope: &str) -> Vec<SettledReceipt> {
        let mut store = self.inner.lock().unwrap();
        let Some(queue) = store.get_mut(session_id) else {
            return Vec::new();
        };
        let mut matched = Vec::new();
        let mut retained = Vec::new();
        for receipt in std::mem::take(queue) {
            if receipt.turn_scope.as_deref() == Some(turn_scope) {
                matched.push(receipt);
            } else {
                retained.push(receipt);
            }
        }
        *queue = retained;
        if queue.is_empty() {
            store.remove(session_id);
        }
        matched
    }

    /// Queue one settled receipt for its session. `pub(crate)` only so finalizer
    /// tests can stage receipts; production receipts are all born in the relay.
    pub(crate) fn push(&self, receipt: SettledReceipt) {
        let mut store = self.inner.lock().unwrap();
        let queue = store.entry(receipt.session_id.clone()).or_default();
        // A replayed response (duplicate terminal frame relayed as a retry, or a
        // response observed twice) must not double-count its money.
        if queue
            .iter()
            .any(|queued| queued.provider_response_id == receipt.provider_response_id)
        {
            return;
        }
        queue.push(receipt);
    }

    fn discard(&self, session_id: &str) {
        let dropped = self.inner.lock().unwrap().remove(session_id);
        if let Some(dropped) = dropped {
            if !dropped.is_empty() {
                crate::platform::logging::report("credential_broker", "eprintln", format!(
                    "synth-desktop: dropped {} settled receipt(s) undrained at close of session {session_id}",
                    dropped.len()
                ));
            }
        }
    }
}

/// Accounting parsed out of one usage-bearing provider payload.
#[derive(Clone, Debug, Default, PartialEq)]
struct ResponseAccounting {
    response_id: Option<String>,
    model: Option<String>,
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    cached_tokens: Option<i64>,
    reasoning_tokens: Option<i64>,
    cost_usd: Option<f64>,
}

impl ResponseAccounting {
    fn into_receipt(self, session_id: &str, turn_scope: Option<String>) -> SettledReceipt {
        SettledReceipt {
            session_id: session_id.to_owned(),
            turn_scope,
            // A response object without an id still settles once: the fallback
            // id is unique, so insert-time dedupe simply never collapses it.
            provider_response_id: self
                .response_id
                .unwrap_or_else(|| format!("unidentified-{}", Uuid::new_v4().simple())),
            model: self.model,
            prompt_tokens: self.prompt_tokens,
            completion_tokens: self.completion_tokens,
            cached_tokens: self.cached_tokens,
            reasoning_tokens: self.reasoning_tokens,
            cost_usd: self.cost_usd,
            completed_at_ms: chrono::Utc::now().timestamp_millis(),
        }
    }
}

/// Extract accounting from one JSON payload if it carries a `usage` object.
///
/// Covers both upstream shapes: the usage-bearing object is either the payload
/// itself (chat completions, non-streamed Responses) or the `response` field
/// of a Responses API terminal event. Token field names differ per shape;
/// settled money is `usage.cost` (OpenRouter) with `usage.cost_usd` accepted
/// as a known backend variant.
fn accounting_from_json(value: &Value) -> Option<ResponseAccounting> {
    let nested = value.get("response");
    let object = [Some(value), nested]
        .into_iter()
        .flatten()
        .find(|candidate| candidate.get("usage").is_some_and(Value::is_object))?;
    let usage = object.get("usage")?;
    let int = |keys: &[&str]| keys.iter().find_map(|key| usage.get(*key)?.as_i64());
    let detail = |outer: &[&str], inner: &str| {
        outer
            .iter()
            .find_map(|key| usage.get(*key)?.get(inner)?.as_i64())
    };
    Some(ResponseAccounting {
        response_id: object.get("id").and_then(Value::as_str).map(str::to_owned),
        model: object
            .get("model")
            .and_then(Value::as_str)
            .map(str::to_owned),
        prompt_tokens: int(&["prompt_tokens", "input_tokens"]),
        completion_tokens: int(&["completion_tokens", "output_tokens"]),
        cached_tokens: detail(
            &["prompt_tokens_details", "input_tokens_details"],
            "cached_tokens",
        )
        .or_else(|| int(&["cached_tokens"])),
        reasoning_tokens: detail(
            &["completion_tokens_details", "output_tokens_details"],
            "reasoning_tokens",
        )
        .or_else(|| int(&["reasoning_tokens"])),
        cost_usd: ["cost", "cost_usd"]
            .iter()
            .find_map(|key| usage.get(*key)?.as_f64()),
    })
}

/// Incremental server-sent-events reader. Chunk boundaries fall anywhere —
/// mid-line, mid-event — so lines are reassembled byte-wise and events on
/// blank-line separators. Only the *last* usage-bearing event counts, which
/// also collapses duplicated terminal frames into one receipt.
#[derive(Default)]
struct SseAccountingScanner {
    partial_line: Vec<u8>,
    event_data: Vec<u8>,
    last: Option<ResponseAccounting>,
}

impl SseAccountingScanner {
    fn observe(&mut self, chunk: &[u8]) {
        for byte in chunk {
            if *byte == b'\n' {
                let line = std::mem::take(&mut self.partial_line);
                self.take_line(&line);
            } else {
                self.partial_line.push(*byte);
            }
        }
    }

    fn take_line(&mut self, line: &[u8]) {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            self.complete_event();
            return;
        }
        if let Some(data) = line.strip_prefix(b"data:") {
            let data = data.strip_prefix(b" ").unwrap_or(data);
            // Multi-line data fields join with a newline, per the SSE spec.
            if !self.event_data.is_empty() {
                self.event_data.push(b'\n');
            }
            self.event_data.extend_from_slice(data);
        }
        // `event:`/`id:`/`retry:` fields and comments carry no accounting.
    }

    fn complete_event(&mut self) {
        let data = std::mem::take(&mut self.event_data);
        if data.is_empty() || data.as_slice() == b"[DONE]" {
            return;
        }
        if let Ok(value) = serde_json::from_slice::<Value>(&data) {
            if let Some(accounting) = accounting_from_json(&value) {
                self.last = Some(accounting);
            }
        }
    }

    fn finish(mut self) -> Option<ResponseAccounting> {
        // Tolerate a stream that ends without its final newline or blank line.
        let line = std::mem::take(&mut self.partial_line);
        if !line.is_empty() {
            self.take_line(&line);
        }
        self.complete_event();
        self.last
    }
}

/// A JSON body larger than this is relayed unread: buffering it for
/// accounting would let a hostile upstream balloon Desktop memory.
const JSON_ACCOUNTING_CAP: usize = 16 * 1024 * 1024;

struct JsonAccountingScanner {
    buffer: Vec<u8>,
    overflowed: bool,
}

impl JsonAccountingScanner {
    fn observe(&mut self, chunk: &[u8]) {
        if self.overflowed {
            return;
        }
        if self.buffer.len() + chunk.len() > JSON_ACCOUNTING_CAP {
            self.overflowed = true;
            self.buffer = Vec::new();
            return;
        }
        self.buffer.extend_from_slice(chunk);
    }

    fn finish(self) -> Option<ResponseAccounting> {
        if self.overflowed {
            return None;
        }
        // `from_slice` accepts trailing whitespace, so a body with or without
        // a final newline parses the same.
        let value = serde_json::from_slice::<Value>(&self.buffer).ok()?;
        accounting_from_json(&value)
    }
}

enum AccountingScanner {
    Sse(SseAccountingScanner),
    Json(JsonAccountingScanner),
}

impl AccountingScanner {
    /// A scanner for the response, or `None` when it cannot carry a receipt:
    /// non-2xx responses settle nothing, and content types that are neither
    /// SSE nor JSON have no accounting to read.
    fn for_response(status: StatusCode, content_type: Option<&str>) -> Option<Self> {
        if !status.is_success() {
            return None;
        }
        let content_type = content_type.unwrap_or("").to_ascii_lowercase();
        if content_type.starts_with("text/event-stream") {
            Some(Self::Sse(SseAccountingScanner::default()))
        } else if content_type.contains("json") {
            Some(Self::Json(JsonAccountingScanner {
                buffer: Vec::new(),
                overflowed: false,
            }))
        } else {
            None
        }
    }

    fn observe(&mut self, chunk: &[u8]) {
        match self {
            Self::Sse(scanner) => scanner.observe(chunk),
            Self::Json(scanner) => scanner.observe(chunk),
        }
    }

    fn finish(self) -> Option<ResponseAccounting> {
        match self {
            Self::Sse(scanner) => scanner.finish(),
            Self::Json(scanner) => scanner.finish(),
        }
    }
}

/// The relayed response body: bytes pass through untouched while the scanner
/// reads settled accounting off to the side. The receipt is emitted exactly
/// once — on clean end of stream, or on drop for a body the client abandoned
/// after the terminal frame had already passed.
/// `hyper`'s boxed body requires `Sync`, which `futures_util`'s `BoxStream`
/// alias deliberately drops; the reqwest byte stream itself is `Sync`.
type RelayedBytes = Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send + Sync>>;

struct MeteredRelay {
    inner: RelayedBytes,
    accounting: Option<(AccountingScanner, String, Option<String>)>,
    receipts: Arc<ReceiptStore>,
}

impl MeteredRelay {
    fn settle(&mut self) {
        if let Some((scanner, session_id, turn_scope)) = self.accounting.take() {
            if let Some(accounting) = scanner.finish() {
                self.receipts
                    .push(accounting.into_receipt(&session_id, turn_scope));
            }
        }
    }
}

impl Stream for MeteredRelay {
    type Item = std::result::Result<Frame<Bytes>, std::io::Error>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        match this.inner.poll_next_unpin(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                if let Some((scanner, _, _)) = this.accounting.as_mut() {
                    scanner.observe(&bytes);
                }
                Poll::Ready(Some(Ok(Frame::data(bytes))))
            }
            Poll::Ready(Some(Err(_))) => Poll::Ready(Some(Err(std::io::Error::other(
                "Synth Cloud ended the response early.",
            )))),
            Poll::Ready(None) => {
                this.settle();
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl Drop for MeteredRelay {
    fn drop(&mut self) {
        self.settle();
    }
}

async fn serve(state: Arc<BrokerState>, listener: tokio::net::TcpListener) -> Result<()> {
    crate::ipc::serve_connections(listener, move |request, _peer| {
        let state = state.clone();
        async move { proxy(state, request).await }
    })
    .await
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

    let status = upstream.status();
    let mut response = Response::builder().status(status.as_u16());
    for (name, value) in upstream.headers().iter() {
        if is_hop_by_hop(name.as_str()) {
            continue;
        }
        response = response.header(name.as_str(), value.as_bytes());
    }
    // Settled accounting rides the same bytes the child receives; the scanner
    // reads them in passing and never delays, reorders, or rewrites them.
    let content_type = upstream
        .headers()
        .get(hyper::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let turn_scope = state
        .turn_scopes
        .read()
        .unwrap()
        .get(&lease.session_id)
        .cloned();
    let accounting = AccountingScanner::for_response(status, content_type.as_deref())
        .map(|scanner| (scanner, lease.session_id.clone(), turn_scope));
    // Stream rather than buffer: a governed Responses call is server-sent events
    // and must reach the agent token by token.
    let stream = MeteredRelay {
        inner: Box::pin(upstream.bytes_stream()),
        accounting,
        receipts: state.receipts.clone(),
    };
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

    fn empty_receipts() -> Arc<ReceiptStore> {
        Arc::new(ReceiptStore::new())
    }

    /// Minimal upstream that records the `Authorization` it was given.
    fn spawn_upstream(body: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
        spawn_upstream_as("200 OK", "application/json", body)
    }

    /// Upstream with a caller-chosen status line and content type, so tests
    /// can exercise the SSE scanner and the non-2xx no-receipt rule.
    fn spawn_upstream_as(
        status: &'static str,
        content_type: &'static str,
        body: &'static str,
    ) -> (String, Arc<Mutex<Vec<String>>>) {
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
                    "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(payload.as_bytes());
            }
        });
        (origin, seen)
    }

    #[test]
    fn a_lease_hands_back_a_token_that_is_not_the_credential() {
        let (broker, _listener) = CredentialBroker::bind(empty_receipts()).unwrap();
        let handle = broker.lease("session-a", "http://127.0.0.1:41209", SENTINEL);
        assert_ne!(handle.token, SENTINEL);
        assert!(!handle.token.contains(SENTINEL));
        assert!(handle.origin.starts_with("http://127.0.0.1:"));
        assert!(broker.resolves(&handle.token));
    }

    #[test]
    fn re_leasing_a_session_replaces_and_invalidates_its_previous_token() {
        let (broker, _listener) = CredentialBroker::bind(empty_receipts()).unwrap();
        let first = broker.lease("session-a", "http://127.0.0.1:41209", SENTINEL);
        let second = broker.lease("session-a", "http://127.0.0.1:41209", SENTINEL);
        assert_ne!(first.token, second.token);
        assert!(!broker.resolves(&first.token));
        assert!(broker.resolves(&second.token));
        assert_eq!(broker.token_for("session-a"), Some(second.token));
    }

    #[test]
    fn revoking_a_session_makes_its_token_inert() {
        let (broker, _listener) = CredentialBroker::bind(empty_receipts()).unwrap();
        let handle = broker.lease("session-a", "http://127.0.0.1:41209", SENTINEL);
        broker.revoke("session-a");
        assert!(!broker.resolves(&handle.token));
    }

    #[test]
    fn debug_formatting_never_reproduces_the_credential() {
        let (broker, _listener) = CredentialBroker::bind(empty_receipts()).unwrap();
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
        let broker = CredentialBroker::start(empty_receipts()).unwrap();
        let handle = broker.lease("session-a", &upstream, SENTINEL);

        let response = crate::http::http_client()
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
        let broker = CredentialBroker::start(empty_receipts()).unwrap();
        let response = crate::http::http_client()
            .post(format!("{}/api/v1/responses", broker.origin()))
            .bearer_auth("sdl_not_a_real_lease")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 401);
        assert!(response
            .text()
            .await
            .unwrap()
            .contains("lease is not valid"));
    }

    #[tokio::test]
    async fn a_revoked_lease_stops_reaching_the_backend() {
        let (upstream, seen) = spawn_upstream(r#"{"ok":true}"#);
        let broker = CredentialBroker::start(empty_receipts()).unwrap();
        let handle = broker.lease("session-a", &upstream, SENTINEL);
        broker.revoke("session-a");

        let response = crate::http::http_client()
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
        let broker = CredentialBroker::start(empty_receipts()).unwrap();
        let handle = broker.lease("session-a", &origin, SENTINEL);
        let response = crate::http::http_client()
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
        assert!(!std::fs::read_to_string(&snapshot)
            .unwrap()
            .contains(SENTINEL));
        assert!(std::fs::read_to_string(&user_file)
            .unwrap()
            .contains(SENTINEL));
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
        let (broker, _listener) = CredentialBroker::bind(empty_receipts()).unwrap();
        let a = broker.lease("session-a", "http://127.0.0.1:1", SENTINEL);
        let b = broker.lease("session-b", "http://127.0.0.1:2", SENTINEL);
        broker.revoke("session-a");
        assert!(!broker.resolves(&a.token));
        assert!(broker.resolves(&b.token));
    }

    // ---- settled accounting capture ----
    // Receipt tests use unique session ids: the receipt store is deliberately
    // process-wide and these tests run in parallel.

    const RESPONSES_COMPLETED: &str = r#"{"type":"response.completed","response":{"id":"resp_settled","model":"openrouter/poolside/laguna-s-2.1","usage":{"input_tokens":1000,"output_tokens":200,"input_tokens_details":{"cached_tokens":400},"output_tokens_details":{"reasoning_tokens":50},"cost":0.0123}}}"#;

    fn scan_sse(chunks: &[&[u8]]) -> Option<ResponseAccounting> {
        let mut scanner = SseAccountingScanner::default();
        for chunk in chunks {
            scanner.observe(chunk);
        }
        scanner.finish()
    }

    #[test]
    fn sse_accounting_survives_chunk_splits_mid_line_and_mid_event() {
        let body = format!(
            "event: response.output_text.delta\ndata: {{\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}}\n\nevent: response.completed\ndata: {RESPONSES_COMPLETED}\n\n"
        );
        // Feed the same stream byte by byte: every line and event boundary is
        // split across chunk boundaries somewhere.
        let bytes = body.as_bytes();
        let single: Vec<&[u8]> = vec![bytes];
        let shredded: Vec<&[u8]> = bytes.chunks(1).collect();
        for chunks in [single, shredded] {
            let accounting = scan_sse(&chunks).expect("the terminal frame carries usage");
            assert_eq!(accounting.response_id.as_deref(), Some("resp_settled"));
            assert_eq!(
                accounting.model.as_deref(),
                Some("openrouter/poolside/laguna-s-2.1")
            );
            assert_eq!(accounting.prompt_tokens, Some(1000));
            assert_eq!(accounting.completion_tokens, Some(200));
            assert_eq!(accounting.cached_tokens, Some(400));
            assert_eq!(accounting.reasoning_tokens, Some(50));
            assert_eq!(accounting.cost_usd, Some(0.0123));
        }
    }

    #[test]
    fn sse_accounting_reads_the_last_usage_frame_and_tolerates_a_missing_final_newline() {
        // An interim usage frame is superseded by the terminal one, [DONE] is
        // ignored, and a stream cut off without its final blank line still
        // settles the frame that already arrived.
        let body = format!(
            "data: {{\"id\":\"resp_interim\",\"usage\":{{\"input_tokens\":1,\"output_tokens\":1}}}}\n\ndata: {RESPONSES_COMPLETED}\n\ndata: [DONE]"
        );
        let accounting = scan_sse(&[body.as_bytes()]).expect("usage frames arrived");
        assert_eq!(accounting.response_id.as_deref(), Some("resp_settled"));
        assert_eq!(accounting.cost_usd, Some(0.0123));
    }

    #[test]
    fn a_duplicated_terminal_usage_frame_settles_exactly_once() {
        let body = format!("data: {RESPONSES_COMPLETED}\n\ndata: {RESPONSES_COMPLETED}\n\n");
        let accounting = scan_sse(&[body.as_bytes()]).expect("usage frames arrived");
        let store = ReceiptStore::new();
        store.push(accounting.clone().into_receipt("session-dup-frame", None));
        // Even a receipt replayed at insert (retry path) collapses on its
        // provider response id.
        store.push(accounting.into_receipt("session-dup-frame", None));
        let receipts = store.drain("session-dup-frame");
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].cost_usd, Some(0.0123));
        assert!(store.drain("session-dup-frame").is_empty());
    }

    #[test]
    fn json_accounting_parses_with_and_without_a_trailing_newline() {
        let body = r#"{"id":"gen_1","model":"laguna-s","usage":{"prompt_tokens":10,"completion_tokens":5,"cost":0.001}}"#;
        for variant in [body.to_owned(), format!("{body}\n")] {
            let mut scanner = JsonAccountingScanner {
                buffer: Vec::new(),
                overflowed: false,
            };
            scanner.observe(variant.as_bytes());
            let accounting = scanner.finish().expect("the body carries usage");
            assert_eq!(accounting.response_id.as_deref(), Some("gen_1"));
            assert_eq!(accounting.prompt_tokens, Some(10));
            assert_eq!(accounting.completion_tokens, Some(5));
            assert_eq!(accounting.cost_usd, Some(0.001));
        }
    }

    #[test]
    fn settled_money_reads_usage_cost_with_cost_usd_as_the_known_variant() {
        let with_cost: Value =
            serde_json::from_str(r#"{"id":"a","usage":{"prompt_tokens":1,"cost":0.5}}"#).unwrap();
        let with_cost_usd: Value =
            serde_json::from_str(r#"{"id":"b","usage":{"prompt_tokens":1,"cost_usd":0.7}}"#)
                .unwrap();
        let with_both: Value = serde_json::from_str(
            r#"{"id":"c","usage":{"prompt_tokens":1,"cost":0.5,"cost_usd":0.7}}"#,
        )
        .unwrap();
        let with_neither: Value =
            serde_json::from_str(r#"{"id":"d","usage":{"prompt_tokens":9,"completion_tokens":3}}"#)
                .unwrap();
        assert_eq!(
            accounting_from_json(&with_cost).unwrap().cost_usd,
            Some(0.5)
        );
        assert_eq!(
            accounting_from_json(&with_cost_usd).unwrap().cost_usd,
            Some(0.7)
        );
        assert_eq!(
            accounting_from_json(&with_both).unwrap().cost_usd,
            Some(0.5)
        );
        // Usage without money is still a receipt: tokens are real, cost stays
        // None rather than an invented zero.
        let tokens_only = accounting_from_json(&with_neither).unwrap();
        assert_eq!(tokens_only.cost_usd, None);
        assert_eq!(tokens_only.prompt_tokens, Some(9));
    }

    #[test]
    fn only_successful_sse_or_json_responses_are_scanned() {
        assert!(
            AccountingScanner::for_response(StatusCode::BAD_GATEWAY, Some("application/json"))
                .is_none()
        );
        assert!(AccountingScanner::for_response(
            StatusCode::UNAUTHORIZED,
            Some("text/event-stream")
        )
        .is_none());
        assert!(
            AccountingScanner::for_response(StatusCode::OK, Some("application/octet-stream"))
                .is_none()
        );
        assert!(AccountingScanner::for_response(StatusCode::OK, None).is_none());
        assert!(matches!(
            AccountingScanner::for_response(
                StatusCode::OK,
                Some("text/event-stream; charset=utf-8")
            ),
            Some(AccountingScanner::Sse(_))
        ));
        assert!(matches!(
            AccountingScanner::for_response(
                StatusCode::OK,
                Some("application/json; charset=utf-8")
            ),
            Some(AccountingScanner::Json(_))
        ));
    }

    #[test]
    fn an_oversized_json_body_is_relayed_without_a_receipt() {
        let mut scanner = JsonAccountingScanner {
            buffer: Vec::new(),
            overflowed: false,
        };
        scanner.observe(&vec![b'x'; JSON_ACCOUNTING_CAP]);
        scanner.observe(b"y");
        assert!(scanner.finish().is_none());
    }

    #[tokio::test]
    async fn the_proxy_relays_json_bytes_untouched_while_capturing_a_settled_receipt() {
        let body = r#"{"id":"resp_json_e2e","model":"openrouter/poolside/laguna-s-2.1","status":"completed","usage":{"input_tokens":120,"output_tokens":30,"cost":0.004}}"#;
        let (upstream, _seen) = spawn_upstream(body);
        let store = empty_receipts();
        let broker = CredentialBroker::start(store.clone()).unwrap();
        let handle = broker.lease("session-json-e2e", &upstream, SENTINEL);

        let response = crate::http::http_client()
            .post(format!("{}/api/v1/responses", handle.origin))
            .bearer_auth(&handle.token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        // The relayed bytes are exactly the upstream's bytes.
        assert_eq!(response.text().await.unwrap(), body);

        let receipts = store.drain("session-json-e2e");
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].provider_response_id, "resp_json_e2e");
        assert_eq!(receipts[0].session_id, "session-json-e2e");
        assert_eq!(receipts[0].prompt_tokens, Some(120));
        assert_eq!(receipts[0].completion_tokens, Some(30));
        assert_eq!(receipts[0].cost_usd, Some(0.004));
        assert!(receipts[0].completed_at_ms > 0);
    }

    #[tokio::test]
    async fn the_proxy_relays_sse_bytes_untouched_while_capturing_a_settled_receipt() {
        let body = "event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}\n\nevent: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_sse_e2e\",\"model\":\"openrouter/poolside/laguna-s-2.1\",\"usage\":{\"input_tokens\":11,\"output_tokens\":7,\"cost\":0.002}}}\n\n";
        let (upstream, _seen) = spawn_upstream_as("200 OK", "text/event-stream", body);
        let store = empty_receipts();
        let broker = CredentialBroker::start(store.clone()).unwrap();
        let handle = broker.lease("session-sse-e2e", &upstream, SENTINEL);

        let response = crate::http::http_client()
            .post(format!("{}/api/v1/responses", handle.origin))
            .bearer_auth(&handle.token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.text().await.unwrap(), body);

        let receipts = store.drain("session-sse-e2e");
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].provider_response_id, "resp_sse_e2e");
        assert_eq!(receipts[0].cost_usd, Some(0.002));
    }

    #[tokio::test]
    async fn a_failing_upstream_response_settles_nothing() {
        // The body even carries a plausible usage object; a non-2xx response
        // must still produce no receipt.
        let body = r#"{"id":"resp_err","error":{"message":"overloaded"},"usage":{"input_tokens":5,"cost":0.9}}"#;
        let (upstream, _seen) =
            spawn_upstream_as("503 Service Unavailable", "application/json", body);
        let store = empty_receipts();
        let broker = CredentialBroker::start(store.clone()).unwrap();
        let handle = broker.lease("session-non2xx", &upstream, SENTINEL);

        let response = crate::http::http_client()
            .post(format!("{}/api/v1/responses", handle.origin))
            .bearer_auth(&handle.token)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 503);
        assert_eq!(response.text().await.unwrap(), body);
        assert!(store.drain("session-non2xx").is_empty());
    }

    #[test]
    fn revoking_a_session_drops_its_undrained_receipts() {
        let store = empty_receipts();
        let (broker, _listener) = CredentialBroker::bind(store.clone()).unwrap();
        broker.lease("session-undrained", "http://127.0.0.1:1", SENTINEL);
        store.push(SettledReceipt {
            session_id: "session-undrained".into(),
            turn_scope: None,
            provider_response_id: "resp_orphan".into(),
            model: None,
            prompt_tokens: Some(5),
            completion_tokens: Some(1),
            cached_tokens: None,
            reasoning_tokens: None,
            cost_usd: Some(0.01),
            completed_at_ms: 1,
        });
        broker.revoke("session-undrained");
        // Dropped, not re-queued: a closed session has no finalize left that
        // could truthfully absorb the money.
        assert!(store.drain("session-undrained").is_empty());
    }
}
