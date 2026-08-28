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

