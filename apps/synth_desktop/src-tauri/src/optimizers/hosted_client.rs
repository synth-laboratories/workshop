//! Shared optimizers-beta client used by hosted GELO and standalone SFT.
//! Algorithm modules own bounded recipes; this module owns transport only.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::time::Duration;

#[derive(Clone)]
pub(super) struct HostedOptimizerClient {
    client: reqwest::Client,
    pub(super) base_url: String,
    token: String,
}

impl HostedOptimizerClient {
    pub(super) fn from_env() -> Result<Self> {
        let token = std::env::var("OPTIMIZERS_BETA_SERVICE_TOKEN")
            .map_err(|_| anyhow!("hosted optimizer requires OPTIMIZERS_BETA_SERVICE_TOKEN"))?;
        let base_url = std::env::var("SYNTH_OPTIMIZERS_BETA_URL")
            .or_else(|_| std::env::var("OPTIMIZERS_BETA_URL"))
            .unwrap_or_else(|_| "http://127.0.0.1:8879".into());
        if base_url.trim().is_empty() || token.trim().is_empty() {
            bail!("hosted optimizer beta URL/token must be non-empty");
        }
        Ok(Self {
            client: crate::http::http_client_with_timeout(crate::limits::OPTIMIZERS_CLOUD_TIMEOUT),
            base_url: base_url.trim().trim_end_matches('/').to_string(),
            token: token.trim().to_string(),
        })
    }

    pub(super) async fn submit_toml(
        &self,
        algorithm: &str,
        run_id: &str,
        config_toml: &str,
    ) -> Result<Value> {
        self.post_json(
            "/v1/runs",
            json!({
                "algorithm": algorithm,
                "idempotency_key": run_id,
                "config_toml": config_toml,
            }),
        )
        .await
    }

    pub(super) async fn submit_json(
        &self,
        algorithm: &str,
        run_id: &str,
        config_json: Value,
    ) -> Result<Value> {
        self.post_json(
            "/v1/runs",
            json!({
                "algorithm": algorithm,
                "idempotency_key": run_id,
                "config_json": config_json,
            }),
        )
        .await
    }

    /// Canonical GELO `optimizer_event.v1` page. The beta service exposes
    /// NDJSON over the durable GoEx event artifact; Workshop wraps that payload
    /// in the shared fail-closed page contract before ingest.
    pub(super) async fn goex_optimizer_events_after(
        &self,
        run_id: &str,
        after_sequence: u64,
        limit: usize,
    ) -> Result<Value> {
        let url = format!(
            "{}/v1/runs/{run_id}/goex-events?after_seq={after_sequence}&limit={}",
            self.base_url,
            limit.clamp(1, 5_000)
        );
        let response = self
            .client
            .get(&url)
            .bearer_auth(&self.token)
            .header("Accept", "application/x-ndjson")
            .send()
            .await
            .context("hosted GELO optimizer event request")?;
        let status = response.status();
        let text = response.text().await.context("read hosted GELO events")?;
        if !status.is_success() {
            bail!("hosted GELO optimizer events failed ({status}): {text}");
        }
        let mut events = Vec::new();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            events.push(
                serde_json::from_str::<Value>(line)
                    .with_context(|| format!("parse hosted GELO event line {}", index + 1))?,
            );
        }
        Ok(json!({"run_id": run_id, "events": events}))
    }

    pub(super) async fn optimizer_events_after(
        &self,
        run_id: &str,
        after_sequence: u64,
        limit: usize,
    ) -> Result<Value> {
        self.get_json(&format!(
            "/runs/{run_id}/optimizer-events?after_sequence={after_sequence}&limit={}",
            limit.clamp(1, 5_000)
        ))
        .await
    }

    pub(super) async fn state_batch(&self, run_id: &str, slices: &[&str]) -> Result<Value> {
        let url = format!(
            "{}/v1/runs/{run_id}/state/batch?slices={}",
            self.base_url,
            slices.join(",")
        );
        self.get_url(&url).await
    }

    pub(super) async fn get_run(&self, run_id: &str) -> Result<Value> {
        self.get_json(&format!("/v1/runs/{run_id}")).await
    }

    pub(super) async fn cancel(&self, run_id: &str) -> Result<Value> {
        self.post_json(&format!("/v1/runs/{run_id}/cancel"), json!({}))
            .await
    }

    async fn get_json(&self, path: &str) -> Result<Value> {
        self.get_url(&format!("{}{path}", self.base_url)).await
    }

    async fn get_url(&self, url: &str) -> Result<Value> {
        let response = self
            .client
            .get(url)
            .bearer_auth(&self.token)
            .header("Accept", "application/json")
            .send()
            .await
            .with_context(|| format!("GET {url}"))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .context("read hosted optimizer body")?;
        if !status.is_success() {
            bail!("hosted optimizer GET failed ({status}): {text}");
        }
        serde_json::from_str(&text).context("decode hosted optimizer JSON")
    }

    async fn post_json(&self, path: &str, body: Value) -> Result<Value> {
        let url = format!("{}{path}", self.base_url);
        let response = self
            .client
            .post(&url)
            // Hosted submission includes producer-side credential and model
            // probes before the run is accepted. Keep streaming reads short,
            // but give this one idempotent control request room to finish.
            .timeout(Duration::from_secs(300))
            .bearer_auth(&self.token)
            .header("Accept", "application/json")
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .context("read hosted optimizer body")?;
        if !status.is_success() {
            bail!("hosted optimizer POST failed ({status}): {text}");
        }
        if text.trim().is_empty() {
            return Ok(json!({}));
        }
        serde_json::from_str(&text).context("decode hosted optimizer JSON")
    }
}
