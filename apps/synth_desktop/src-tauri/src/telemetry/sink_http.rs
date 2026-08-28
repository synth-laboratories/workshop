//! Profile-routed HTTP sink for the Synth backend.
//!
//! Routing and credentials resolve from `synth_config` on every flush, so a
//! profile switch (local slot, staging, prod) takes effect without a restart.
//! Signed-in flushes carry the API key and the backend attributes user/org;
//! signed-out flushes are attributed by install_id alone.

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::future::Future;
use std::pin::Pin;

use super::flush::TelemetrySink;

const USAGE_EVENTS_PATH: &str = "/api/v1/product/usage-events";

pub struct HttpSink {
    http: reqwest::Client,
    /// Test seam: a pinned `(base_url, api_key)` endpoint. Production always
    /// resolves from `synth_config` per flush.
    fixed_endpoint: Option<(String, Option<String>)>,
}

impl HttpSink {
    pub fn new() -> Self {
        Self {
            http: crate::http::http_client(),
            fixed_endpoint: None,
        }
    }

    #[cfg(test)]
    pub fn with_endpoint(base_url: &str, api_key: Option<&str>) -> Self {
        Self {
            http: crate::http::http_client(),
            fixed_endpoint: Some((base_url.into(), api_key.map(str::to_owned))),
        }
    }

    fn endpoint(&self) -> Result<(String, Option<String>)> {
        if let Some(fixed) = self.fixed_endpoint.clone() {
            return Ok(fixed);
        }
        let resolved = crate::synth_config::resolve().context("resolve telemetry backend")?;
        Ok((resolved.backend_url, resolved.api_key))
    }

    async fn post(&self, batch: &[Value]) -> Result<()> {
        let (base_url, api_key) = self.endpoint()?;
        let mut request = self
            .http
            .post(format!(
                "{}{USAGE_EVENTS_PATH}",
                base_url.trim_end_matches('/')
            ))
            .json(&json!({
                "schema_version": 1,
                "product": "workshop",
                "events": batch,
            }));
        if let Some(key) = api_key.as_deref() {
            request = request.bearer_auth(key);
        }
        let response = request
            .send()
            .await
            .context("reach the telemetry ingestion service")?;
        if !response.status().is_success() {
            return Err(anyhow!(
                "telemetry ingestion refused the batch ({})",
                response.status()
            ));
        }
        Ok(())
    }
}

impl TelemetrySink for HttpSink {
    fn send<'a>(
        &'a self,
        batch: &'a [Value],
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(self.post(batch))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Storage;
    use crate::telemetry::{consent, flush::Flusher, store::TelemetryStore};
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;

    /// One-request HTTP server that captures the full request (headers and
    /// body, honoring Content-Length) and answers with `status`.
    fn spawn_ingestion_server(status: u16) -> (String, std::thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut raw = Vec::new();
            let mut buf = [0u8; 8192];
            loop {
                let n = stream.read(&mut buf).unwrap();
                raw.extend_from_slice(&buf[..n]);
                let text = String::from_utf8_lossy(&raw);
                if let Some(split) = text.find("\r\n\r\n") {
                    let content_length = text
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .and_then(|v| v.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if raw.len() >= split + 4 + content_length {
                        break;
                    }
                }
                if n == 0 {
                    break;
                }
            }
            let body = r#"{"ok":true,"accepted":1,"recorded":1,"duplicates":0,"event_ids":[]}"#;
            let response = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream.write_all(response.as_bytes()).unwrap();
            String::from_utf8_lossy(&raw).into_owned()
        });
        (origin, handle)
    }

    fn consented_store_with_events() -> (tempfile::TempDir, TelemetryStore) {
        let dir = tempfile::tempdir().unwrap();
        let storage = Storage::open(dir.path()).unwrap();
        let store = TelemetryStore::new(storage.database().clone());
        consent::record_choice(&store, consent::ConsentChoice::Granted).unwrap();
        for name in ["signin_completed", "workflow_started"] {
            let spec = crate::telemetry::contract::spec(name).unwrap();
            store
                .insert(
                    name,
                    spec.sensitivity,
                    &json!({"install_id": "ins_sinktest", "outcome": "success"}),
                )
                .unwrap();
        }
        (dir, store)
    }

    #[tokio::test]
    async fn flush_through_http_sink_ships_the_backend_wire_contract() {
        let (origin, server) = spawn_ingestion_server(200);
        let (_dir, store) = consented_store_with_events();
        let sink = Arc::new(HttpSink::with_endpoint(
            &origin,
            Some("sk_synth_user_sinktest"),
        ));
        let flusher = Flusher::new(store.clone(), sink);

        let outcome = flusher.flush_once().await.unwrap();
        assert_eq!(
            outcome,
            crate::telemetry::flush::FlushOutcome::Sent { events: 2 }
        );

        let request = server.join().unwrap();
        let head = request.to_ascii_lowercase();
        assert!(head.starts_with("post /api/v1/product/usage-events"));
        assert!(head.contains("authorization: bearer sk_synth_user_sinktest"));
        let body: Value = serde_json::from_str(request.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["schema_version"], 1);
        assert_eq!(body["product"], "workshop");
        let events = body["events"].as_array().unwrap();
        assert_eq!(events.len(), 2);
        for event in events {
            assert!(event["event_id"].as_str().unwrap().starts_with("pte_"));
            assert!(event["owner"].as_str().unwrap().starts_with("workshop-"));
            assert!(matches!(
                event["class"].as_str().unwrap(),
                "funnel" | "product" | "reliability"
            ));
            assert!(event["observed_at"].as_str().is_some());
            // The flusher stamps the store's real install id, not whatever
            // the recorded payload carried.
            assert!(event["install_id"].as_str().unwrap().starts_with("ins_"));
        }
        // The watermark advanced: nothing left to ship.
        assert_eq!(
            flusher.flush_once().await.unwrap(),
            crate::telemetry::flush::FlushOutcome::Empty
        );
    }

    #[tokio::test]
    async fn refused_batch_errors_and_keeps_the_outbox_for_retry() {
        let (origin, server) = spawn_ingestion_server(400);
        let (_dir, store) = consented_store_with_events();
        let sink = Arc::new(HttpSink::with_endpoint(&origin, None));
        let flusher = Flusher::new(store.clone(), sink);

        let error = flusher.flush_once().await.unwrap_err();
        assert!(error.to_string().contains("refused the batch"));
        server.join().unwrap();
        // Watermark did not advance; the batch stays queued.
        assert_eq!(store.watermark().unwrap(), 0);
        assert_eq!(store.batch_for_sync(10).unwrap().len(), 2);
    }

    #[tokio::test]
    async fn signed_out_flush_sends_no_authorization_header() {
        let (origin, server) = spawn_ingestion_server(200);
        let (_dir, store) = consented_store_with_events();
        let sink = Arc::new(HttpSink::with_endpoint(&origin, None));
        let flusher = Flusher::new(store, sink);
        flusher.flush_once().await.unwrap();
        let request = server.join().unwrap().to_ascii_lowercase();
        assert!(!request.contains("authorization:"));
    }
}
