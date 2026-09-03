//! Public `synth-optimizers` CISPO control-plane client.
//!
//! Workshop never contacts Optimizers-beta directly. The public CISPO service
//! owns canonical runs and executes `cispo.slime.v1` locally against Tinker.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone)]
pub(super) struct CispoOptimizerClient {
    client: reqwest::Client,
    pub(super) base_url: String,
    token: String,
}

impl CispoOptimizerClient {
    pub(super) fn from_env() -> Result<Self> {
        let token = std::env::var("SYNTH_OPTIMIZERS_CISPO_SERVICE_TOKEN")
            .map_err(|_| anyhow!("CISPO service requires SYNTH_OPTIMIZERS_CISPO_SERVICE_TOKEN"))?;
        let base_url = std::env::var("SYNTH_OPTIMIZERS_CISPO_SERVICE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8880".into());
        if base_url.trim().is_empty() || token.trim().is_empty() {
            bail!("public CISPO service URL/token must be non-empty");
        }
        Ok(Self {
            client: crate::http::http_client_with_timeout(crate::limits::OPTIMIZERS_CLOUD_TIMEOUT),
            base_url: base_url.trim().trim_end_matches('/').to_string(),
            token: token.trim().to_string(),
        })
    }

    #[allow(dead_code)]
    pub(super) async fn health(&self) -> Result<Value> {
        self.get_json("/health").await
    }

    pub(super) async fn submit(&self, run_id: &str, config_json: &Value) -> Result<Value> {
        self.post_json(
            "/v1/runs",
            json!({
                "algorithm": "cispo",
                "idempotency_key": run_id,
                "run_id": run_id,
                "config_json": config_json,
            }),
        )
        .await
    }

    pub(super) async fn optimizer_events_after(
        &self,
        run_id: &str,
        after_sequence: u64,
        limit: usize,
    ) -> Result<Value> {
        self.get_json(&format!(
            "/v1/runs/{run_id}/optimizer-events?after_sequence={after_sequence}&limit={}",
            limit.clamp(1, 5_000)
        ))
        .await
    }

    pub(super) async fn get_run(&self, run_id: &str) -> Result<Value> {
        self.get_json(&format!("/v1/runs/{run_id}")).await
    }

    pub(super) async fn cancel(&self, run_id: &str) -> Result<Value> {
        self.post_json(&format!("/v1/runs/{run_id}/cancel"), json!({}))
            .await
    }

    async fn get_json(&self, path: &str) -> Result<Value> {
        let url = format!("{}{path}", self.base_url);
        let response = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/json")
            .send()
            .await
            .with_context(|| format!("GET {url}"))?;
        decode_response(response, "GET public CISPO service").await
    }

    async fn post_json(&self, path: &str, body: Value) -> Result<Value> {
        let url = format!("{}{path}", self.base_url);
        let response = self
            .client
            .post(&url)
            .timeout(Duration::from_secs(300))
            .bearer_auth(&self.token)
            .header("Accept", "application/json")
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        decode_response(response, "POST public CISPO service").await
    }
}

async fn decode_response(response: reqwest::Response, operation: &str) -> Result<Value> {
    let status = response.status();
    let text = response
        .text()
        .await
        .context("read public CISPO service body")?;
    if !status.is_success() {
        bail!("{operation} failed ({status}): {text}");
    }
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(&text).context("decode public CISPO service JSON")
}
