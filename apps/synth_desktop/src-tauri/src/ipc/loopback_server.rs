//! Shared hyper loopback server (credential_broker `serve()` paragon).
//!
//! Hand-rolled `HTTP/1.1` framing belongs here once, not in each IPC module.

use anyhow::{Context, Result};
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::Value;
use std::convert::Infallible;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};

pub type LoopbackBody = http_body_util::combinators::BoxBody<Bytes, std::io::Error>;

/// Accept loop used by the credential broker and JSON IPC servers.
///
/// IPC stays loopback-only. The provider proxy uses
/// [`serve_connections_allowing`] for Docker-bridge peers.
pub async fn serve_connections<F, Fut>(
    listener: tokio::net::TcpListener,
    on_request: F,
) -> Result<()>
where
    F: Fn(Request<Incoming>, SocketAddr) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<Response<LoopbackBody>, Infallible>> + Send + 'static,
{
    serve_connections_allowing(listener, |ip| ip.is_loopback(), on_request).await
}

/// Accept loop with a caller-supplied peer allowlist.
pub async fn serve_connections_allowing<F, Fut, P>(
    listener: tokio::net::TcpListener,
    peer_ok: P,
    on_request: F,
) -> Result<()>
where
    F: Fn(Request<Incoming>, SocketAddr) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<Response<LoopbackBody>, Infallible>> + Send + 'static,
    P: Fn(IpAddr) -> bool + Clone + Send + Sync + 'static,
{
    loop {
        let (stream, peer) = listener
            .accept()
            .await
            .context("accept a connection on a Synth Desktop loopback server")?;
        if !peer_ok(peer.ip()) {
            eprintln!("synth-desktop: rejected IPC peer {peer}");
            continue;
        }
        let on_request = on_request.clone();
        tauri::async_runtime::spawn(async move {
            let io = TokioIo::new(stream);
            let service = service_fn(move |request| on_request(request, peer));
            if let Err(error) = http1::Builder::new()
                .max_buf_size(crate::limits::LOOPBACK_MAX_HEADER_BYTES)
                .serve_connection(io, service)
                .await
            {
                eprintln!("synth-desktop: loopback connection ended: {error}");
            }
        });
    }
}

/// Unix-domain HTTP accept loop. There is no TCP peer to check; the socket
/// path's mode (0600) is the trust boundary.
#[cfg(unix)]
pub async fn serve_unix_connections<F, Fut>(
    listener: tokio::net::UnixListener,
    on_request: F,
) -> Result<()>
where
    F: Fn(Request<Incoming>) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Result<Response<LoopbackBody>, Infallible>> + Send + 'static,
{
    loop {
        let (stream, _peer) = listener
            .accept()
            .await
            .context("accept a connection on a Synth Desktop unix server")?;
        let on_request = on_request.clone();
        tauri::async_runtime::spawn(async move {
            let io = TokioIo::new(stream);
            let service = service_fn(move |request| on_request(request));
            if let Err(error) = http1::Builder::new()
                .max_buf_size(crate::limits::LOOPBACK_MAX_HEADER_BYTES)
                .serve_connection(io, service)
                .await
            {
                eprintln!("synth-desktop: unix connection ended: {error}");
            }
        });
    }
}

/// Parsed JSON request for thin IPC routers (visuals / eval).
#[derive(Debug)]
pub struct JsonHttpRequest {
    pub method: Method,
    pub path: String,
    pub authorization: Option<String>,
    pub body: Value,
    pub raw_headers: hyper::HeaderMap,
    /// Accepted socket peer; `None` only for requests built directly in tests.
    pub peer: Option<std::net::SocketAddr>,
}

/// Length-safe token equality: rejects on length without inspecting bytes,
/// then OR-folds every byte so match position never shapes timing.
pub fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right.iter())
        .fold(0u8, |acc, (a, b)| acc | (a ^ b))
        == 0
}

/// JSON response with optional extra headers (e.g. protocol version).
#[derive(Debug)]
pub struct JsonHttpResponse {
    pub status: StatusCode,
    pub body: Value,
    pub extra_headers: Vec<(&'static str, String)>,
}

impl JsonHttpResponse {
    pub fn ok(body: Value) -> Self {
        Self {
            status: StatusCode::OK,
            body,
            extra_headers: Vec::new(),
        }
    }

    pub fn error(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            body: serde_json::json!({ "error": message.into() }),
            extra_headers: Vec::new(),
        }
    }
}

pub fn json_response(status: StatusCode, body: Value) -> Response<LoopbackBody> {
    let payload = serde_json::to_vec(&body).unwrap_or_else(|_| b"{\"error\":\"encode\"}".to_vec());
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(
            Full::new(Bytes::from(payload))
                .map_err(|never| match never {})
                .boxed(),
        )
        .expect("static JSON response")
}

/// Hyper accept loop that parses JSON bodies and dispatches to a router.
pub async fn serve_json<F, Fut>(listener: tokio::net::TcpListener, handler: F) -> Result<()>
where
    F: Fn(JsonHttpRequest) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = JsonHttpResponse> + Send + 'static,
{
    serve_json_with_limit(listener, crate::limits::VISUALS_IPC_MAX_BODY_BYTES, handler).await
}

/// JSON server variant for protocols with a different request-body cap.
pub async fn serve_json_with_limit<F, Fut>(
    listener: tokio::net::TcpListener,
    max_body_bytes: usize,
    handler: F,
) -> Result<()>
where
    F: Fn(JsonHttpRequest) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = JsonHttpResponse> + Send + 'static,
{
    serve_connections(listener, move |request, peer| {
        let handler = handler.clone();
        async move {
            let parsed = match parse_json_request(request, peer, max_body_bytes).await {
                Ok(parsed) => parsed,
                Err(response) => return Ok(response),
            };
            let JsonHttpResponse {
                status,
                body,
                extra_headers,
            } = handler(parsed).await;
            let mut response = json_response(status, body);
            for (name, value) in extra_headers {
                if let Ok(header) = hyper::header::HeaderValue::from_str(&value) {
                    response.headers_mut().insert(name, header);
                }
            }
            Ok(response)
        }
    })
    .await
}

async fn parse_json_request(
    request: Request<Incoming>,
    peer: std::net::SocketAddr,
    max_body_bytes: usize,
) -> Result<JsonHttpRequest, Response<LoopbackBody>> {
    let method = request.method().clone();
    let path = request
        .uri()
        .path_and_query()
        .map(|value| value.as_str().to_owned())
        .unwrap_or_else(|| "/".to_owned());
    let authorization = request
        .headers()
        .get(hyper::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.trim().to_owned());
    let raw_headers = request.headers().clone();
    let collected = http_body_util::Limited::new(request.into_body(), max_body_bytes)
        .collect()
        .await
        .map_err(|error| {
            let (status, message) = if error.is::<http_body_util::LengthLimitError>() {
                (StatusCode::PAYLOAD_TOO_LARGE, "request body exceeds limit")
            } else {
                (StatusCode::BAD_GATEWAY, "could not read request body")
            };
            json_response(status, serde_json::json!({"error":message}))
        })?;
    let bytes = collected.to_bytes();
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).map_err(|_| {
            json_response(
                StatusCode::BAD_REQUEST,
                serde_json::json!({"error":"request body must be JSON"}),
            )
        })?
    };
    Ok(JsonHttpRequest {
        method,
        path,
        authorization,
        body,
        raw_headers,
        peer: Some(peer),
    })
}

#[cfg(test)]
mod tests {
    use super::constant_time_eq;

    #[test]
    fn constant_time_eq_matches_semantics_of_eq() {
        assert!(constant_time_eq(b"synth_eval_abc", b"synth_eval_abc"));
        assert!(!constant_time_eq(b"synth_eval_abc", b"synth_eval_abd"));
        assert!(!constant_time_eq(b"synth_eval_abc", b"synth_eval_ab"));
        assert!(constant_time_eq(b"", b""));
        assert!(!constant_time_eq(b"", b"x"));
    }
}
