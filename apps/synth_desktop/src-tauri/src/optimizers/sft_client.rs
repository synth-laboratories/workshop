//! Public `synth-optimizers` SFT control-plane client.
//!
//! Workshop never contacts Optimizers-beta directly. The public SFT service owns
//! canonical runs and proxies beta only as its internal training executor.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone)]
pub(super) struct SftOptimizerClient {
    client: reqwest::Client,
    pub(super) base_url: String,
    token: String,
}

impl SftOptimizerClient {
    pub(super) fn from_env() -> Result<Self> {
        let token = std::env::var("SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN")
            .map_err(|_| anyhow!("SFT service requires SYNTH_OPTIMIZERS_SFT_SERVICE_TOKEN"))?;
        let base_url = std::env::var("SYNTH_OPTIMIZERS_SFT_SERVICE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:8878".into());
        if base_url.trim().is_empty() || token.trim().is_empty() {
            bail!("public SFT service URL/token must be non-empty");
        }
        Ok(Self {
            client: crate::http::http_client_with_timeout(crate::limits::OPTIMIZERS_CLOUD_TIMEOUT),
            base_url: base_url.trim().trim_end_matches('/').to_string(),
            token: token.trim().to_string(),
        })
    }

    pub(super) async fn submit_toml(&self, run_id: &str, config_toml: &str) -> Result<Value> {
        self.post_json(
            "/v1/runs",
            json!({
                "algorithm": "sft",
                "idempotency_key": run_id,
                "config_toml": config_toml,
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

    pub(super) async fn infer_checkpoint(
        &self,
        family: &str,
        sampler_path: &str,
        run_id: &str,
        checkpoint_id: &str,
        body: &Value,
    ) -> Result<Value> {
        let path = match family {
            "chat_completions" | "chat" => "/v1/checkpoints/infer/chat/completions",
            "responses" => "/v1/checkpoints/infer/responses",
            other => bail!("unsupported inference family {other}"),
        };
        self.post_json(
            path,
            json!({
                "sampler_path": sampler_path,
                "run_id": run_id,
                "checkpoint_id": checkpoint_id,
                "body": body,
            }),
        )
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
        decode_response(response, "GET public SFT service").await
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
        decode_response(response, "POST public SFT service").await
    }
}

async fn decode_response(response: reqwest::Response, operation: &str) -> Result<Value> {
    let status = response.status();
    let text = response
        .text()
        .await
        .context("read public SFT service body")?;
    if !status.is_success() {
        bail!("{operation} failed ({status}): {text}");
    }
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(&text).context("decode public SFT service JSON")
}
