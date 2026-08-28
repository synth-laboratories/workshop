//! Host-owned provider proxy. Agents authenticate with a run capability;
//! the proxy injects the real credential and never returns it.

use anyhow::{anyhow, bail, Context, Result};
use bytes::Bytes;
use http_body_util::{combinators::BoxBody, BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use serde_json::Value;
use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Syntactic stand-in for SDKs that require `OPENAI_API_KEY`. Not a credential.
pub const API_KEY_SENTINEL: &str = "workshop-proxy";

use super::audit::{self, SecretAuditEvent};
use super::backend::SecretBackend;
use super::capability::{self, CapabilityStore, MeasuredUsage};
use super::providers::{self, inject_auth, parse_usage, request_effort, request_model, route_for};
use super::vault;
use crate::ipc::constant_time_eq;
use crate::storage::Database;

type ProxyBody = BoxBody<Bytes, std::io::Error>;

pub struct ProviderProxy {
    pub origin: String,
    addr: SocketAddr,
    socket_path: Option<PathBuf>,
}

pub struct ProxyState {
    pub db: Arc<Database>,
    pub backend: Arc<dyn SecretBackend>,
    pub env_sources: Arc<super::lease::EnvSourceStore>,
    pub capabilities: Arc<CapabilityStore>,
}

impl ProviderProxy {
    pub fn start(state: Arc<ProxyState>) -> Result<Self> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .context("bind the Workshop provider proxy to loopback")?;
        listener
            .set_nonblocking(true)
            .context("make the provider proxy listener non-blocking")?;
        let addr = listener.local_addr()?;
        let port = addr.port();
        let origin = format!("http://127.0.0.1:{port}");
        // Linux Docker `extra_hosts=host.docker.internal:host-gateway` hits
        // docker0 (172.17.0.1), not 127.0.0.1. Bind that gateway when it is a
        // local address. Docker Desktop on Mac already maps the hostname to
        // loopback, so the extra bind is a no-op there.
        let extra = container_gateway_listeners(port);
        #[cfg_attr(not(unix), allow(unused_mut))]
        let mut socket_path = unix_socket_path_for(&state.db, port);
        spawn_tcp_proxy(listener, state.clone());
        for extra_listener in extra {
            spawn_tcp_proxy(extra_listener, state.clone());
        }
        #[cfg(unix)]
        {
            match socket_path
                .as_ref()
                .and_then(|path| bind_unix_listener(path).ok())
            {
                Some(unix) => spawn_unix_proxy(unix, state),
                None => socket_path = None,
            }
        }
        Ok(Self {
            origin,
            addr,
            socket_path,
        })
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn socket_path(&self) -> Option<&Path> {
        self.socket_path.as_deref()
    }
}

impl Drop for ProviderProxy {
    fn drop(&mut self) {
        if let Some(path) = &self.socket_path {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn spawn_tcp_proxy(listener: std::net::TcpListener, state: Arc<ProxyState>) {
    tauri::async_runtime::spawn(async move {
        let listener = match tokio::net::TcpListener::from_std(listener) {
            Ok(listener) => listener,
            Err(error) => {
                crate::platform::logging::report(
                    "secrets",
                    "eprintln",
                    format!("synth-desktop: adopt provider proxy listener: {error:#}"),
                );
                return;
            }
        };
        if let Err(error) = crate::ipc::serve_connections_allowing(
            listener,
            allowed_proxy_peer,
            move |request, peer| {
                let state = state.clone();
                async move { handle(state, request, Some(peer)).await }
            },
        )
        .await
        {
            crate::platform::logging::report(
                "secrets",
                "eprintln",
                format!("synth-desktop: provider proxy stopped serving: {error:#}"),
            );
        }
    });
}

#[cfg(unix)]
fn spawn_unix_proxy(listener: std::os::unix::net::UnixListener, state: Arc<ProxyState>) {
    tauri::async_runtime::spawn(async move {
        let listener = match tokio::net::UnixListener::from_std(listener) {
            Ok(listener) => listener,
            Err(error) => {
                crate::platform::logging::report(
                    "secrets",
                    "eprintln",
                    format!("synth-desktop: adopt provider proxy unix listener: {error:#}"),
                );
                return;
            }
        };
        if let Err(error) = crate::ipc::serve_unix_connections(listener, move |request| {
            let state = state.clone();
            async move { handle(state, request, None).await }
        })
        .await
        {
            crate::platform::logging::report(
                "secrets",
                "eprintln",
                format!("synth-desktop: provider proxy unix socket stopped serving: {error:#}"),
            );
        }
    });
}

/// Peers that may talk to the provider proxy: host loopback, Docker bridge
/// ranges, Docker Desktop's VM NAT, and Podman. Not the operator LAN.
pub(crate) fn allowed_proxy_peer(ip: IpAddr) -> bool {
    if ip.is_loopback() {
        return true;
    }
    let IpAddr::V4(v4) = ip else {
        return false;
    };
    let octets = v4.octets();
    matches!(octets, [172, second, ..] if (16..=31).contains(&second))
        || matches!(octets, [192, 168, 65, _])
        || matches!(octets, [10, 88, ..])
}

fn container_gateway_listeners(port: u16) -> Vec<std::net::TcpListener> {
    container_gateway_ips()
        .into_iter()
        .filter_map(|ip| {
            let listener = std::net::TcpListener::bind((ip, port)).ok()?;
            listener.set_nonblocking(true).ok()?;
            Some(listener)
        })
        .collect()
}

fn container_gateway_ips() -> Vec<Ipv4Addr> {
    let mut ips = Vec::new();
    if let Ok(raw) = std::env::var("WORKSHOP_PROXY_BIND") {
        for part in raw.split(',') {
            if let Ok(ip) = part.trim().parse::<Ipv4Addr>() {
                if allowed_proxy_peer(IpAddr::V4(ip)) && ipv4_is_local(ip) {
                    ips.push(ip);
                }
            }
        }
    }
    for candidate in [Ipv4Addr::new(172, 17, 0, 1), Ipv4Addr::new(10, 88, 0, 1)] {
        if !ips.contains(&candidate) && ipv4_is_local(candidate) {
            ips.push(candidate);
        }
    }
    ips
}

fn ipv4_is_local(ip: Ipv4Addr) -> bool {
    std::net::TcpListener::bind((ip, 0)).is_ok()
}

fn unix_socket_path_for(db: &crate::storage::Database, port: u16) -> Option<PathBuf> {
    #[cfg(unix)]
    {
        Some(
            db.path()
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("secrets")
                .join(format!("workshop-proxy-{port}.sock")),
        )
    }
    #[cfg(not(unix))]
    {
        let _ = (db, port);
        None
    }
}

#[cfg(unix)]
fn bind_unix_listener(path: &Path) -> Result<std::os::unix::net::UnixListener> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).context("create provider proxy unix socket directory")?;
    }
    let _ = std::fs::remove_file(path);
    let listener = std::os::unix::net::UnixListener::bind(path)
        .with_context(|| format!("bind provider proxy unix socket {}", path.display()))?;
    listener
        .set_nonblocking(true)
        .context("make the provider proxy unix listener non-blocking")?;
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    Ok(listener)
}

/// Marks who produced a response: the proxy itself, or the upstream provider.
/// Without this, a proxy-generated 502 and a relayed upstream 502 are
/// indistinguishable to the caller. Never carries a handle, key, or URL.
pub const RELAY_ORIGIN_HEADER: &str = "x-workshop-proxy-origin";
pub const RELAY_ORIGIN_PROXY: &str = "proxy";
pub const RELAY_ORIGIN_UPSTREAM: &str = "upstream";

/// Secret-free classification of a transport failure. `reqwest::Error`'s
/// `Display` embeds the request URL — which carries the capability handle — so
/// the error is classified into a fixed vocabulary and the value itself is
/// never rendered into a response, an audit row, or a log line.
fn classify_transport_error(error: &reqwest::Error) -> (StatusCode, &'static str, &'static str) {
    if error.is_timeout() {
        (
            StatusCode::GATEWAY_TIMEOUT,
            "upstream_timeout",
            "the approved provider endpoint did not answer in time",
        )
    } else if error.is_connect() {
        (
            StatusCode::BAD_GATEWAY,
            "upstream_unreachable",
            "the approved provider endpoint could not be reached",
        )
    } else if error.is_body() || error.is_decode() {
        (
            StatusCode::BAD_GATEWAY,
            "upstream_body_unreadable",
            "the approved provider endpoint returned an unreadable body",
        )
    } else if error.is_request() || error.is_builder() {
        (
            StatusCode::BAD_GATEWAY,
            "upstream_request_rejected",
            "the provider request could not be issued",
        )
    } else {
        (
            StatusCode::BAD_GATEWAY,
            "upstream_unavailable",
            "the approved provider endpoint is unavailable",
        )
    }
}

/// Record a provider call that never produced billable usage. Without this the
/// audit ledger holds a `capability.issue` row and nothing else, so a failed
/// run is indistinguishable from a run that was never attempted. Carries the
/// classification code only — never a handle, key, URL, prompt, or response.
fn audit_provider_failure(
    state: &ProxyState,
    live: &capability::LiveCapability,
    operation: &str,
    model: Option<&str>,
    code: &str,
    upstream_status: Option<u16>,
) {
    let _ = state.db.with_conn(|conn| {
        let mut event = SecretAuditEvent::new("run", &live.run_id, "provider.use", "failed");
        event.secret_id = Some(live.secret_id.clone());
        event.provider = Some(live.provider.clone());
        event.operation = Some(operation.to_owned());
        event.model = model.map(str::to_owned);
        event.capability_id = Some(live.id.clone());
        event.detail = Some(match upstream_status {
            Some(status) => format!("{code}:{status}"),
            None => code.to_owned(),
        });
        audit::append(conn, &event)
    });
}

fn json_error(status: StatusCode, code: &str, message: &str) -> Response<ProxyBody> {
    let body = serde_json::json!({
        "error": { "code": code, "message": providers::sanitize_error_message(message) }
    })
    .to_string();
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header(RELAY_ORIGIN_HEADER, RELAY_ORIGIN_PROXY)
        .body(
            Full::new(Bytes::from(body))
                .map_err(|never| match never {})
                .boxed(),
        )
        .expect("static provider proxy error")
}

fn sanitize_json_strings(value: &mut Value) {
    match value {
        Value::String(text) => *text = providers::sanitize_error_message(text),
        Value::Array(items) => items.iter_mut().for_each(sanitize_json_strings),
        Value::Object(map) => map.values_mut().for_each(sanitize_json_strings),
        _ => {}
    }
}

fn sanitize_upstream_body(status: reqwest::StatusCode, bytes: Bytes) -> Bytes {
    if status.is_success() {
        return bytes;
    }
    if let Ok(mut value) = serde_json::from_slice::<Value>(&bytes) {
        sanitize_json_strings(&mut value);
        return Bytes::from(value.to_string());
    }
    Bytes::from(providers::sanitize_error_message(&String::from_utf8_lossy(
        &bytes,
    )))
}

fn bearer(request: &Request<Incoming>) -> Option<String> {
    request
        .headers()
        .get(hyper::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::trim)
        .filter(|value| value.starts_with("wcap_"))
        .map(str::to_owned)
        .or_else(|| {
            request
                .headers()
                .get("x-workshop-capability")
                .and_then(|value| value.to_str().ok())
                .map(str::trim)
                .filter(|value| value.starts_with("wcap_"))
                .map(str::to_owned)
        })
}

/// `/cap/{wcap_…}/v1/providers/openai/chat/completions` → (handle, stripped path).
pub fn split_capability_path(path: &str) -> (Option<String>, String) {
    let rest = match path.strip_prefix("/cap/") {
        Some(rest) => rest,
        None => return (None, path.to_owned()),
    };
    let Some((handle, remainder)) = rest.split_once('/') else {
        return (None, path.to_owned());
    };
    if handle.starts_with("wcap_") && handle.len() > 8 {
        (Some(handle.to_owned()), format!("/{remainder}"))
    } else {
        (None, path.to_owned())
    }
}

pub fn capability_base_url(origin: &str, handle: &str, provider: &str) -> String {
    format!("{origin}/cap/{handle}/v1/providers/{provider}")
}

pub fn capability_chat_completions_url(origin: &str, handle: &str, provider: &str) -> String {
    format!(
        "{}/chat/completions",
        capability_base_url(origin, handle, provider)
    )
}

/// Hostname containers use to reach the host-owned proxy. Never `127.0.0.1`:
/// that is the container itself. Override with `WORKSHOP_PROXY_CONTAINER_HOST`.
pub fn container_proxy_host() -> String {
    std::env::var("WORKSHOP_PROXY_CONTAINER_HOST")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "host.docker.internal".into())
}

/// Rewrite a loopback proxy origin so Docker/Podman trial containers can
/// reach it. The advertised hostname stays `host.docker.internal`; on Linux
/// the proxy also binds the docker0/podman gateway so that hostname resolves.
pub fn rewrite_origin_for_containers(origin: &str) -> String {
    let host = container_proxy_host();
    origin
        .replace("127.0.0.1", &host)
        .replace("[::1]", &host)
        .replace("localhost", &host)
}

fn is_forbidden_header(name: &str) -> bool {
    matches!(
        name,
        "authorization"
            | "proxy-authorization"
            | "proxy-authenticate"
            | "x-api-key"
            | "api-key"
            | "cookie"
            | "set-cookie"
            | "host"
            | "x-forwarded-for"
            | "x-forwarded-host"
            | "x-forwarded-proto"
            | "forwarded"
            | "connection"
            | "keep-alive"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "content-length"
    ) || name.eq_ignore_ascii_case(RELAY_ORIGIN_HEADER)
}

async fn handle(
    state: Arc<ProxyState>,
    request: Request<Incoming>,
    peer: Option<SocketAddr>,
) -> Result<Response<ProxyBody>, Infallible> {
    if let Some(peer) = peer {
        if !allowed_proxy_peer(peer.ip()) {
            return Ok(json_error(
                StatusCode::FORBIDDEN,
                "forbidden",
                "provider proxy refuses this peer",
            ));
        }
    }
    if request.method() == hyper::Method::CONNECT {
        return Ok(json_error(
            StatusCode::FORBIDDEN,
            "forbidden",
            "CONNECT tunneling is not allowed",
        ));
    }
    let raw_path = request.uri().path().to_owned();
    let (path_handle, path) = split_capability_path(&raw_path);
    if request.method() == hyper::Method::GET && path == "/v1/capabilities/self" {
        return Ok(capability_self(&state, &request));
    }
    let Some(route) = route_for(request.method().as_str(), &path) else {
        return Ok(json_error(
            StatusCode::NOT_FOUND,
            "unknown_operation",
            "this provider proxy does not forward arbitrary URLs",
        ));
    };
    let Some(handle) = bearer(&request).or(path_handle) else {
        return Ok(json_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "a Workshop run capability is required",
        ));
    };
    let live = match state.capabilities.lookup(&handle) {
        Some(live) => live,
        None => {
            return Ok(json_error(
                StatusCode::UNAUTHORIZED,
                "unauthorized",
                "capability is not valid",
            ))
        }
    };
    if !constant_time_eq(live.handle.as_bytes(), handle.as_bytes()) {
        return Ok(json_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "capability is not valid",
        ));
    }
    if live.provider != route.provider {
        return Ok(json_error(
            StatusCode::FORBIDDEN,
            "provider_mismatch",
            "capability is bound to a different provider",
        ));
    }
    if let Err(error) = capability::authorize_request(&live, route.operation, None, None) {
        return Ok(json_error(
            StatusCode::FORBIDDEN,
            "operation_denied",
            &error.to_string(),
        ));
    }
    let reserved = match state.capabilities.reserve_call(&handle) {
        Ok(live) => live,
        Err(error) => {
            let text = error.to_string();
            let (status, code) = if text.contains("expired") {
                (StatusCode::UNAUTHORIZED, "capability_expired")
            } else if text.contains("revoked") {
                (StatusCode::UNAUTHORIZED, "unauthorized")
            } else if text.contains("exhausted") || text.contains("ceiling") {
                (StatusCode::TOO_MANY_REQUESTS, "budget_exhausted")
            } else {
                (StatusCode::UNAUTHORIZED, "unauthorized")
            };
            return Ok(json_error(status, code, &text));
        }
    };

    let (parts, body) = request.into_parts();
    let collected = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => {
            return Ok(json_error(
                StatusCode::BAD_REQUEST,
                "invalid_body",
                "could not read the provider request",
            ))
        }
    };
    if collected.len() > crate::limits::SECRETS_PROXY_MAX_BODY_BYTES {
        return Ok(json_error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "payload_too_large",
            "request exceeds the approved body size",
        ));
    }
    let parsed: Value = serde_json::from_slice(&collected).unwrap_or(Value::Null);
    let model = request_model(&parsed).map(str::to_owned);
    let effort = request_effort(&parsed).map(str::to_owned);
    if let Err(error) = capability::authorize_request(
        &reserved,
        route.operation,
        model.as_deref(),
        effort.as_deref(),
    ) {
        return Ok(json_error(
            StatusCode::FORBIDDEN,
            "policy_denied",
            &error.to_string(),
        ));
    }

    let secret = match state.db.with_conn(|conn| {
        vault::resolve_for_proxy(
            conn,
            state.backend.as_ref(),
            Some(state.env_sources.as_ref()),
            &reserved.secret_id,
        )
    }) {
        Ok(secret) => secret,
        Err(error) => {
            return Ok(json_error(
                StatusCode::FAILED_DEPENDENCY,
                "secret_unavailable",
                &error.to_string(),
            ))
        }
    };

    let http = crate::http::http_client_builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(crate::limits::CREDENTIAL_UPSTREAM_TIMEOUT)
        .build()
        .expect("provider proxy HTTP client");
    let mut outbound = http.post(route.upstream_url).body(collected.clone());
    for (name, value) in parts.headers.iter() {
        if is_forbidden_header(name.as_str()) {
            continue;
        }
        outbound = outbound.header(name.as_str(), value.as_bytes());
    }
    outbound = match inject_auth(outbound, route, &secret) {
        Ok(builder) => builder,
        Err(error) => {
            return Ok(json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "inject_failed",
                &error.to_string(),
            ))
        }
    };
    drop(secret);

    let upstream = match outbound.send().await {
        Ok(response) => response,
        Err(error) => {
            let (status, code, message) = classify_transport_error(&error);
            audit_provider_failure(
                &state,
                &reserved,
                route.operation,
                model.as_deref(),
                code,
                None,
            );
            return Ok(json_error(status, code, message));
        }
    };
    let status = upstream.status();
    let content_type = upstream
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("application/json")
        .to_owned();
    let mut builder = Response::builder().status(status.as_u16());
    for (name, value) in upstream.headers().iter() {
        if is_forbidden_header(name.as_str()) {
            continue;
        }
        builder = builder.header(name.as_str(), value.as_bytes());
    }

    let bytes = match upstream.bytes().await {
        Ok(bytes) => bytes,
        Err(error) => {
            let (relay_status, code, message) = classify_transport_error(&error);
            audit_provider_failure(
                &state,
                &reserved,
                route.operation,
                model.as_deref(),
                code,
                Some(status.as_u16()),
            );
            return Ok(json_error(relay_status, code, message));
        }
    };
    let bytes = sanitize_upstream_body(status, bytes);
    let usage = if content_type.contains("json") {
        serde_json::from_slice::<Value>(&bytes)
            .ok()
            .map(|body| parse_usage(&body))
            .unwrap_or(MeasuredUsage {
                calls: 1,
                input_tokens: 0,
                output_tokens: 0,
                cost_usd: None,
            })
    } else {
        MeasuredUsage {
            calls: 1,
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: None,
        }
    };
    if status.is_success() {
        if let Ok(live) = state.capabilities.debit_usage(&handle, &usage) {
            let _ = state.db.with_conn(|conn| {
                capability::persist_usage(conn, &live)?;
                let mut event =
                    SecretAuditEvent::new("run", &live.run_id, "provider.use", "allowed");
                event.secret_id = Some(live.secret_id.clone());
                event.provider = Some(live.provider.clone());
                event.operation = Some(route.operation.into());
                event.model = model.clone();
                event.capability_id = Some(live.id.clone());
                event.usage = Some(serde_json::json!({
                    "calls": usage.calls,
                    "input_tokens": usage.input_tokens,
                    "output_tokens": usage.output_tokens,
                    "cost_usd": usage.cost_usd,
                }));
                audit::append(conn, &event)
            });
        }
    }

    if !status.is_success() {
        audit_provider_failure(
            &state,
            &reserved,
            route.operation,
            model.as_deref(),
            "upstream_status",
            Some(status.as_u16()),
        );
    }
    Ok(builder
        .header("content-type", content_type)
        .header(RELAY_ORIGIN_HEADER, RELAY_ORIGIN_UPSTREAM)
        .body(Full::new(bytes).map_err(|never| match never {}).boxed())
        .unwrap_or_else(|_| {
            json_error(
                StatusCode::BAD_GATEWAY,
                "relay_failed",
                "could not relay the provider response",
            )
        }))
}

fn capability_self(state: &ProxyState, request: &Request<Incoming>) -> Response<ProxyBody> {
    let Some(handle) = bearer(request) else {
        return json_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "a Workshop run capability is required",
        );
    };
    let Some(live) = state.capabilities.lookup(&handle) else {
        return json_error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "capability is not valid",
        );
    };
    let body = serde_json::json!({
        "id": live.id,
        "provider": live.provider,
        "runId": live.run_id,
        "recipeId": live.recipe_id,
        "operations": live.operations,
        "models": live.models,
        "status": live.status,
        "usedCalls": live.used_calls,
        "maxCalls": live.max_calls,
        "usedCostUsd": live.used_cost_usd_micros as f64 / 1_000_000.0,
        "maxCostUsd": live.max_cost_usd_micros as f64 / 1_000_000.0,
    })
    .to_string();
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(
            Full::new(Bytes::from(body))
                .map_err(|never| match never {})
                .boxed(),
        )
        .expect("capability self response")
}

/// Workload environment that points at the proxy without a provider key.
#[derive(Clone, Debug, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct WorkloadEnv {
    pub openai_base_url: Option<String>,
    pub anthropic_base_url: Option<String>,
    /// Always the sentinel `workshop-proxy`, never a provider key or handle.
    pub openai_api_key: String,
    pub capability_handle: String,
    pub capability_file: Option<String>,
    pub workshop_run_id: String,
    pub capability_id: String,
    /// Host-process chat completions URL (`127.0.0.1`). Not for containers.
    pub openai_route: Option<String>,
    /// Container-reachable SDK base (`host.docker.internal`, never loopback).
    pub container_openai_base_url: Option<String>,
    /// Container-reachable chat completions URL. Bind this as `EVAL_LLM_ROUTE`.
    pub container_openai_route: Option<String>,
    /// Host unix socket for tools that can speak HTTP over AF_UNIX.
    pub proxy_socket: Option<String>,
}

impl WorkloadEnv {
    pub fn write_capability_file(&mut self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir).context("create capability file directory")?;
        let path = dir.join("workshop.capability");
        std::fs::write(&path, self.capability_handle.as_bytes())
            .context("write workshop capability file")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        self.capability_file = Some(path.display().to_string());
        Ok(())
    }

    /// Trusted route object for the eval worker manifest. Containers must use
    /// `openai` / `openai_base`, never the recipe's `api.openai.com` route.
    pub fn provider_routes(&self) -> Result<serde_json::Value> {
        let openai = self.container_openai_route.clone().ok_or_else(|| {
            anyhow!("secrets_proxy_route_unbound: Workshop did not bind an OpenAI proxy route")
        })?;
        let openai_base = self.container_openai_base_url.clone().ok_or_else(|| {
            anyhow!("secrets_proxy_route_unbound: Workshop did not bind an OpenAI proxy base URL")
        })?;
        if looks_like_loopback(&openai) || looks_like_loopback(&openai_base) {
            bail!(
                "secrets_proxy_unreachable: container route still points at loopback; set WORKSHOP_PROXY_CONTAINER_HOST"
            );
        }
        if openai.contains("api.openai.com") || openai_base.contains("api.openai.com") {
            bail!("secrets_proxy_route_unbound: container route must not be the upstream provider");
        }
        Ok(serde_json::json!({
            "openai": openai,
            "openai_base": openai_base,
            "auth": "capability_path",
            "api_key_sentinel": API_KEY_SENTINEL,
            "container_host": container_proxy_host(),
            "extra_hosts": ["host.docker.internal:host-gateway"],
            "socket": self.proxy_socket,
        }))
    }

    pub fn as_pairs(&self) -> Vec<(String, String)> {
        let mut pairs = vec![
            ("OPENAI_API_KEY".into(), API_KEY_SENTINEL.to_owned()),
            ("WORKSHOP_RUN_ID".into(), self.workshop_run_id.clone()),
            ("WORKSHOP_CAPABILITY".into(), self.capability_handle.clone()),
            (
                "WORKSHOP_CREDENTIAL_MODE".into(),
                super::lease::CREDENTIAL_MODE_WORKSHOP_PROXY.into(),
            ),
        ];
        if let Some(file) = &self.capability_file {
            pairs.push(("WORKSHOP_CAPABILITY_FILE".into(), file.clone()));
        }
        if let Some(url) = &self.openai_base_url {
            pairs.push(("OPENAI_BASE_URL".into(), url.clone()));
        }
        if let Some(url) = &self.anthropic_base_url {
            pairs.push(("ANTHROPIC_BASE_URL".into(), url.clone()));
        }
        if let Some(url) = &self.container_openai_route {
            pairs.push(("WORKSHOP_OPENAI_ROUTE".into(), url.clone()));
        }
        if let Some(url) = &self.container_openai_base_url {
            pairs.push(("WORKSHOP_OPENAI_BASE_URL".into(), url.clone()));
            pairs.push(("WORKSHOP_INFERENCE_URL".into(), url.clone()));
        }
        if let Some(socket) = &self.proxy_socket {
            pairs.push(("WORKSHOP_PROXY_SOCKET".into(), socket.clone()));
        }
        pairs
    }
}

fn looks_like_loopback(url: &str) -> bool {
    url.contains("127.0.0.1") || url.contains("localhost") || url.contains("[::1]")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Banking77 v0.7 boundary: a proxy-generated 502 and a relayed
    /// upstream 502 carried the same status and the same body, so ten failed
    /// rollouts recorded no attributable cause. The relay marker separates them.
    #[test]
    fn proxy_generated_errors_are_marked_as_proxy_origin() {
        let response = json_error(
            StatusCode::BAD_GATEWAY,
            "upstream_unreachable",
            "the approved provider endpoint could not be reached",
        );
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(
            response
                .headers()
                .get(RELAY_ORIGIN_HEADER)
                .and_then(|value| value.to_str().ok()),
            Some(RELAY_ORIGIN_PROXY)
        );
    }

    /// The relay marker is proxy-owned: an inbound or upstream copy must never
    /// survive into the response, or the marker could be spoofed.
    #[test]
    fn relay_origin_header_is_never_forwarded() {
        assert!(is_forbidden_header(RELAY_ORIGIN_HEADER));
        assert!(is_forbidden_header("X-Workshop-Proxy-Origin"));
    }

    /// A timeout, an unreachable host, and an unreadable body are three
    /// different operator actions. Collapsing them into one 502 was the defect.
    ///
    /// Bind a port, drop the listener, then connect to it: the refusal is
    /// immediate and local. An unroutable address would sit until a timeout
    /// and perturb scheduling for the rest of the suite, which shares a
    /// process-global data root through `isolated_root()`.
    #[tokio::test]
    async fn transport_failures_classify_into_distinct_codes() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve a port");
        let port = listener.local_addr().expect("port").port();
        drop(listener);

        let client = crate::http::http_client_builder()
            .build()
            .expect("test client");
        let connect_error = client
            .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
            .body("{}")
            .send()
            .await
            .expect_err("a closed port must not accept");

        let (status, code, message) = classify_transport_error(&connect_error);
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(code, "upstream_unreachable");

        // Neither the code nor the message may echo the request URL -- under a
        // capability route that string carries the handle.
        assert!(!message.contains(&port.to_string()));
        assert!(!message.contains("127.0.0.1"));
        assert!(!code.contains("127.0.0.1"));
    }

    /// `reqwest::Error`'s own Display embeds the URL. The classifier must not
    /// be the thing that leaks it back to a caller.
    #[test]
    fn classification_vocabulary_is_a_closed_set() {
        for code in [
            "upstream_timeout",
            "upstream_unreachable",
            "upstream_body_unreadable",
            "upstream_request_rejected",
            "upstream_unavailable",
            "upstream_status",
        ] {
            assert_eq!(providers::sanitize_error_message(code), code);
        }
    }
}
